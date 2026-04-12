//! Concrete `SyscallDispatcher` implementation.
//!
//! Holds the Task + waker + `LinuxSyscallHandler` glue that used to live
//! inline inside `interrupts::trap::handle_syscall`. Keeping it here lets
//! `trap.rs` stay oblivious to process/task/signal internals.

use abi::syscalls::trap_frame::{Register, TrapFrame};
use core::task::{Context, Poll};

use crate::{
    cpu::{Cpu, cpu_id},
    interrupts::syscall_dispatch::SyscallDispatcher,
    memory::VirtAddr,
    processes::{
        signal::SignalDeliveryResult, task::Task, thread::ThreadState, waker::ThreadWaker,
    },
    syscalls::linux::LinuxSyscallHandler,
};

pub struct LinuxSyscallRunner;

static RUNNER: LinuxSyscallRunner = LinuxSyscallRunner;

pub fn install() {
    crate::interrupts::syscall_dispatch::register(&RUNNER);
}

impl SyscallDispatcher for LinuxSyscallRunner {
    fn dispatch(&self, mut trap_frame: TrapFrame) {
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
                    let sepc = hal::cpu::read_sepc();
                    let signal_result = Cpu::with_scheduler(|s| {
                        s.get_current_thread().with_lock(|mut t| {
                            t.set_register_state(trap_frame);
                            t.set_program_counter(VirtAddr::new(sepc + 4)); // Skip ecall
                            crate::processes::signal::deliver_signal(&mut t)
                        })
                    });
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
                let sepc = hal::cpu::read_sepc();
                s.get_current_thread().with_lock(|mut t| {
                    t.set_register_state(trap_frame);
                    t.set_program_counter(VirtAddr::new(sepc));
                    t.set_syscall_task_and_suspend(task);
                });
                s.schedule();
            });
        }
    }
}

// Check if we still own the thread (syscall might have set it to Waiting or another CPU
// might have stolen it). If we don't own it, save state and reschedule. Returns true if
// we should continue executing on this CPU.
fn check_thread_ownership_and_reschedule_if_needed(trap_frame: TrapFrame) -> bool {
    Cpu::with_scheduler(|mut s| {
        let cpu_id = cpu_id();
        let should_reschedule = s
            .get_current_thread()
            .with_lock(|mut t| match t.get_state() {
                ThreadState::Running {
                    cpu_id: running_cpu,
                } if running_cpu == cpu_id => false,
                ThreadState::Running { cpu_id: other_cpu } => {
                    panic!(
                        "Thread {} was stolen by CPU {} while CPU {} was still in syscall handler",
                        t.get_tid(),
                        other_cpu,
                        cpu_id
                    );
                }
                ThreadState::Waiting | ThreadState::Runnable | ThreadState::Stopped => {
                    let sepc = hal::cpu::read_sepc() + 4;
                    t.set_register_state(trap_frame);
                    t.set_program_counter(VirtAddr::new(sepc));
                    true
                }
                ThreadState::Zombie(_) => true,
            });

        if should_reschedule {
            s.schedule();
            false
        } else {
            true
        }
    })
}
