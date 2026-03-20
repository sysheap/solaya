#![allow(unsafe_code)]
use crate::{
    cpu::Cpu,
    debug, info,
    interrupts::plic::{self, PLIC},
    memory::VirtAddr,
    processes::{task::Task, thread::ThreadState, timer, waker::ThreadWaker},
    syscalls::linux::LinuxSyscallHandler,
};
use arch::trap_cause::{InterruptCause, exception::ENVIRONMENT_CALL_FROM_U_MODE, interrupt};
use common::syscalls::trap_frame::{Register, TrapFrame};
use core::{
    panic,
    task::{Context, Poll},
};
use headers::syscall_types::SIGSEGV;

pub extern "C" fn get_process_satp_value() -> usize {
    Cpu::with_current_process(|p| p.get_satp_value())
}

fn assert_sepc_not_in_trap_handler() {
    let sepc = arch::cpu::read_sepc();
    // SAFETY: asm_handle_trap is defined in trap.S
    unsafe extern "C" {
        fn asm_handle_trap();
    }
    let trap_base = asm_handle_trap as *const () as usize;
    assert!(
        !(trap_base..trap_base + 64).contains(&sepc),
        "BUG: sepc {sepc:#x} points into trap handler {trap_base:#x}"
    );
}

pub extern "C" fn handle_trap() {
    let cause = InterruptCause::from_scause();
    if cause.is_interrupt() {
        match cause.get_exception_code() {
            interrupt::SUPERVISOR_TIMER_INTERRUPT => handle_timer_interrupt(),
            interrupt::SUPERVISOR_SOFTWARE_INTERRUPT => handle_supervisor_software_interrupt(),
            interrupt::SUPERVISOR_EXTERNAL_INTERRUPT => handle_external_interrupt(),
            _ => handle_unimplemented(),
        }
    } else {
        handle_exception();
    }
}

fn handle_timer_interrupt() {
    timer::wakeup_wakers();
    Cpu::with_scheduler(|mut s| s.schedule());
    assert_sepc_not_in_trap_handler();
}

fn handle_external_interrupt() {
    debug!("External interrupt occurred!");
    let mut plic_guard = PLIC.lock();
    let irq = match plic_guard.claim() {
        Some(i) => i,
        None => return,
    };
    plic_guard.complete(irq);
    drop(plic_guard);
    plic::dispatch_interrupt(irq);
}

// Check if we still own the thread (syscall might have set it to Waiting or another CPU
// might have stolen it). If we don't own it, save state and reschedule. Returns true if
// we should continue executing on this CPU.
fn check_thread_ownership_and_reschedule_if_needed(trap_frame: TrapFrame) -> bool {
    Cpu::with_scheduler(|mut s| {
        let cpu_id = Cpu::cpu_id();
        let should_reschedule = s.get_current_thread().with_lock(|mut t| {
            match t.get_state() {
                ThreadState::Running {
                    cpu_id: running_cpu,
                } if running_cpu == cpu_id => {
                    // We still own the thread, continue on this CPU
                    false
                }
                ThreadState::Running { cpu_id: other_cpu } => {
                    // Another CPU stole this thread - indicates a race condition bug.
                    // The other CPU is running with stale register state.
                    panic!(
                        "Thread {} was stolen by CPU {} while CPU {} was still in syscall handler",
                        t.get_tid(),
                        other_cpu,
                        cpu_id
                    );
                }
                ThreadState::Waiting | ThreadState::Runnable | ThreadState::Stopped => {
                    // Syscall put us in Waiting/Stopped (and possibly got woken to Runnable).
                    // Save state before rescheduling.
                    let sepc = arch::cpu::read_sepc() + 4; // Skip ecall
                    t.set_register_state(trap_frame);
                    t.set_program_counter(VirtAddr::new(sepc));
                    true
                }
                ThreadState::Zombie(_) => {
                    // Thread was killed by another CPU while we were in a syscall.
                    // No need to save state — just reschedule.
                    true
                }
            }
        });

        if should_reschedule {
            s.schedule();
            false
        } else {
            true
        }
    })
}

fn handle_syscall() {
    let mut trap_frame = Cpu::read_trap_frame();

    // Create an async task for the syscall and poll it once. If it completes
    // synchronously (Poll::Ready), handle the result inline. If it yields
    // (Poll::Pending), save state and reschedule — it will be resumed later
    // by run_syscall_task when the waker fires.
    let task_trap_frame = trap_frame.clone();
    let mut task = Task::new(async move {
        let mut handler = LinuxSyscallHandler::new();
        crate::syscalls::tracer::trace_syscall(&task_trap_frame, &mut handler).await
    });
    let waker = ThreadWaker::new_waker(Cpu::current_thread_weak());
    let mut cx = Context::from_waker(&waker);
    if let Poll::Ready(result) = task.poll(&mut cx) {
        // execve replaces the thread's entire register state (PC, SP, a0, etc.)
        // with the new program's entry point. The normal return path — writing
        // the result to a0 and advancing PC past ecall — must be skipped because
        // the registers already contain the new program's state.
        let replaced = Cpu::with_scheduler(|s| {
            s.get_current_thread().with_lock(|mut t| {
                let r = t.registers_replaced();
                if r {
                    t.set_registers_replaced(false);
                }
                r
            })
        });
        if replaced {
            // Load the pre-set registers (from execve) into the CPU.
            // If the thread was killed between execve and now, reschedule.
            Cpu::with_scheduler(|mut s| {
                if !s.set_cpu_reg_for_current_thread() {
                    s.schedule();
                }
            });
        } else {
            // Normal syscall return: write result to a0 and advance PC by 4
            // to skip the ecall instruction.
            let ret = match result {
                Ok(ret) => ret,
                Err(errno) => -(errno as isize),
            };
            trap_frame[Register::a0] = ret.cast_unsigned();

            if check_thread_ownership_and_reschedule_if_needed(trap_frame.clone()) {
                // Save updated registers to thread, deliver signals, then load back.
                // Read sepc from hardware — the thread's stored PC is stale for
                // the synchronous path (only updated on reschedule).
                let sepc = arch::cpu::read_sepc();
                let signal_result = Cpu::with_scheduler(|s| {
                    s.get_current_thread().with_lock(|mut t| {
                        t.set_register_state(trap_frame);
                        t.set_program_counter(VirtAddr::new(sepc + 4)); // Skip ecall
                        crate::processes::signal::deliver_signal(&mut t)
                    })
                });
                use crate::processes::signal::SignalDeliveryResult;
                match signal_result {
                    SignalDeliveryResult::Terminate(exit_status) => {
                        Cpu::with_scheduler(|mut s| {
                            s.kill_current_process(exit_status);
                            s.schedule();
                        });
                    }
                    SignalDeliveryResult::Stop(sig) => {
                        Cpu::with_scheduler(|mut s| {
                            s.stop_current_process(sig);
                            s.schedule();
                        });
                    }
                    SignalDeliveryResult::Continue => {
                        Cpu::with_scheduler(|mut s| {
                            if !s.set_cpu_reg_for_current_thread() {
                                s.schedule();
                            }
                        });
                    }
                }
            }
        }
    } else {
        // Syscall yielded (Pending) — suspend and reschedule atomically.
        // We must hold the scheduler lock across suspend+schedule to prevent
        // another CPU from waking and stealing this thread before we reschedule.
        Cpu::with_scheduler(|mut s| {
            // Save register state BEFORE suspending.
            // When thread suspends, queue_current_process_back won't save registers
            // (since state is Waiting, not Running), so we must save them here.
            let sepc = arch::cpu::read_sepc();
            s.get_current_thread().with_lock(|mut t| {
                t.set_register_state(trap_frame);
                t.set_program_counter(VirtAddr::new(sepc));
                t.set_syscall_task_and_suspend(task);
            });
            s.schedule();
        });
    }
}

fn handle_unhandled_exception() {
    let cause = InterruptCause::from_scause();
    let stval = arch::cpu::read_stval();
    let sepc = arch::cpu::read_sepc();
    info!(
        "handle_unhandled_exception: cause={} sepc={:#x} stval={:#x}",
        cause.get_reason(),
        sepc,
        stval
    );
    let cpu = Cpu::current();
    let mut scheduler = cpu.scheduler().lock();
    let (message, from_userspace) = scheduler.get_current_process().with_lock(|p| {
        let from_userspace =
            p.get_page_table().is_userspace_address(VirtAddr::new(sepc));
        (format!(
            "Unhandled exception!\nName: {}\nException code: {}\nstval: 0x{:x}\nsepc: 0x{:x}\nFrom Userspace: {}\nProcess name: {}\n{:?}",
            cause.get_reason(),
            cause.get_exception_code(),
            stval,
            sepc,
            from_userspace,
            p.get_name(),
            Cpu::read_trap_frame()
        ), from_userspace)
    });
    if from_userspace {
        info!("{}", message);
        scheduler.kill_current_process(crate::processes::signal::ExitStatus::Signaled(
            u8::try_from(SIGSEGV).expect("signal number fits in u8"),
        ));
        scheduler.schedule();
        return;
    }
    panic!("{}", message);
}

fn handle_exception() {
    let cause = InterruptCause::from_scause();
    let code = cause.get_exception_code();
    match code {
        ENVIRONMENT_CALL_FROM_U_MODE => handle_syscall(),
        _ => handle_unhandled_exception(),
    }

    assert_sepc_not_in_trap_handler();
}

fn handle_supervisor_software_interrupt() {
    let sleep_requested = crate::processes::kernel_tasks::take_sleep_request();

    Cpu::with_scheduler(|mut s| {
        if sleep_requested {
            let is_worker = s
                .get_current_thread()
                .with_lock(|t| crate::processes::kernel_tasks::is_current_worker_tid(t.get_tid()));
            if is_worker {
                s.get_current_thread().with_lock(|mut t| {
                    t.set_register_state(Cpu::read_trap_frame());
                    t.set_program_counter(VirtAddr::new(arch::cpu::read_sepc()));
                    t.suspend_unless_wakeup_pending();
                });
            }
        }
        s.schedule();
    });

    arch::cpu::clear_supervisor_software_interrupt();
    assert_sepc_not_in_trap_handler();
}

fn handle_unimplemented() {
    let sepc = arch::cpu::read_sepc();
    let cause = InterruptCause::from_scause();
    panic!(
        "Unimplemented trap occurred! (sepc: {:x?}) (cause: {:?})",
        sepc,
        cause.get_reason(),
    );
}
