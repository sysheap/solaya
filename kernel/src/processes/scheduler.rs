use super::{
    process::{POWERSAVE_TID, ProcessRef},
    process_table::{self},
    thread::{ThreadRef, ThreadState},
};
use crate::{
    cpu::Cpu,
    debug, info,
    memory::VirtAddr,
    processes::{
        thread::{SyscallTask, Thread},
        timer,
        waker::ThreadWaker,
    },
    test::qemu_exit,
};
use alloc::sync::Arc;
use common::syscalls::trap_frame::Register;
use core::task::{Context, Poll};
use headers::errno::Errno;
pub struct CpuScheduler {
    current_thread: ThreadRef,
    powersave_thread: ThreadRef,
}

enum ProcessMode {
    KernelSyscallTask(SyscallTask),
    Userspace,
}

impl CpuScheduler {
    pub fn new() -> Self {
        let powersave_thread = Thread::create_powersave_thread();

        Self {
            current_thread: powersave_thread.clone(),
            powersave_thread,
        }
    }

    pub fn get_current_thread(&self) -> &ThreadRef {
        &self.current_thread
    }

    pub fn get_current_process(&self) -> ProcessRef {
        self.current_thread.lock().process()
    }

    pub fn is_current_process_energy_saver(&self) -> bool {
        Arc::ptr_eq(&self.current_thread, &self.powersave_thread)
    }

    pub fn schedule(&mut self) {
        debug!("Schedule next process");
        while let ProcessMode::KernelSyscallTask(task) = self.prepare_next_process() {
            debug!("Running syscall task");
            if self.run_syscall_task(task) {
                // we completed the syscall, lets give the process the chance to run directly
                break;
            }
        }

        debug!("Scheduling userspace process");
        if self.is_current_process_energy_saver() {
            timer::set_timer(50);
        } else {
            timer::set_timer(10);
        }
    }

    // Resumes a previously-suspended async syscall task. Returns true if the
    // syscall completed and the thread is ready to return to userspace, false
    // if it yielded again (Pending) or the thread was killed.
    pub fn run_syscall_task(&mut self, mut task: SyscallTask) -> bool {
        let waker = ThreadWaker::new_waker(Arc::downgrade(&self.current_thread));
        let mut cx = Context::from_waker(&waker);
        if let Poll::Ready(result) = task.poll(&mut cx) {
            // Same dual return path as handle_syscall: if execve replaced the
            // registers, skip the normal a0/PC return logic.
            let replaced = self.current_thread.with_lock(|mut t| {
                let r = t.registers_replaced();
                if r {
                    t.set_registers_replaced(false);
                }
                r
            });
            if replaced {
                self.current_thread.lock().clear_wakeup_pending();
                if !self.set_cpu_reg_for_current_thread() {
                    return false;
                }
            } else {
                let ret = match result {
                    Ok(ret) => ret,
                    Err(errno) => -(errno as isize),
                };
                let signal_result = self.current_thread.with_lock(|mut t| {
                    t.clear_wakeup_pending();
                    let trap_frame = t.get_register_state_mut();
                    trap_frame[Register::a0] = ret.cast_unsigned();
                    let pc = t.get_program_counter();
                    t.set_program_counter(pc + 4); // Skip the ecall instruction
                    super::signal::deliver_signal(&mut t)
                });
                match signal_result {
                    super::signal::SignalDeliveryResult::Terminate(exit_status) => {
                        self.kill_current_process(exit_status);
                        return false;
                    }
                    super::signal::SignalDeliveryResult::Stop(sig) => {
                        self.stop_current_process(sig);
                        return false;
                    }
                    super::signal::SignalDeliveryResult::Continue => {}
                }
                if !self.set_cpu_reg_for_current_thread() {
                    return false;
                }
            }
            true
        } else {
            // Still pending — check if a terminating, stop, or handler signal
            // arrived while we were blocked.
            enum BlockedAction {
                None,
                Terminate(super::signal::ExitStatus),
                Stop(u32),
                Interrupt,
            }
            let action = self.current_thread.with_lock(|mut t| {
                loop {
                    let Some(sig) = t.peek_first_unblocked_signal() else {
                        return BlockedAction::None;
                    };
                    let sa = *t.get_sigaction_raw(sig);
                    match sa.sa_handler {
                        None => match super::signal::default_action(sig) {
                            super::signal::DefaultAction::Terminate => {
                                t.take_next_pending_signal();
                                return BlockedAction::Terminate(
                                    super::signal::ExitStatus::Signaled(
                                        u8::try_from(sig).expect("signal number fits in u8"),
                                    ),
                                );
                            }
                            super::signal::DefaultAction::Stop => {
                                t.take_next_pending_signal();
                                return BlockedAction::Stop(sig);
                            }
                            _ => {
                                t.take_next_pending_signal();
                                continue;
                            }
                        },
                        Some(f) if f as usize == 1 => {
                            // SIG_IGN — consume and check for more
                            t.take_next_pending_signal();
                            continue;
                        }
                        Some(_) => {
                            // Custom handler — interrupt the syscall with EINTR
                            return BlockedAction::Interrupt;
                        }
                    }
                }
            });
            match action {
                BlockedAction::Terminate(exit_status) => {
                    self.kill_current_process(exit_status);
                    return false;
                }
                BlockedAction::Stop(sig) => {
                    // Store the task back so it can be resumed when SIGCONT arrives
                    self.current_thread.lock().store_syscall_task(task);
                    self.stop_current_process(sig);
                    return false;
                }
                BlockedAction::Interrupt => {
                    // Drop the syscall task and return -EINTR to userspace
                    drop(task);
                    let signal_result = self.current_thread.with_lock(|mut t| {
                        let trap_frame = t.get_register_state_mut();
                        trap_frame[Register::a0] = (-(Errno::EINTR as isize)).cast_unsigned();
                        let pc = t.get_program_counter();
                        t.set_program_counter(pc + 4);
                        super::signal::deliver_signal(&mut t)
                    });
                    match signal_result {
                        super::signal::SignalDeliveryResult::Terminate(exit_status) => {
                            self.kill_current_process(exit_status);
                            return false;
                        }
                        super::signal::SignalDeliveryResult::Stop(sig) => {
                            self.stop_current_process(sig);
                            return false;
                        }
                        super::signal::SignalDeliveryResult::Continue => {}
                    }
                    if !self.set_cpu_reg_for_current_thread() {
                        return false;
                    }
                    return true;
                }
                BlockedAction::None => {}
            }
            self.current_thread
                .lock()
                .set_syscall_task_and_suspend(task);
            false
        }
    }

    pub fn kill_current_thread(&mut self, exit_status: super::signal::ExitStatus) {
        let tid = self.current_thread.lock().get_tid();
        process_table::THE.lock().kill(tid, exit_status);
    }

    pub fn kill_current_process(&mut self, exit_status: super::signal::ExitStatus) {
        let all_tids = self.current_thread.lock().process().lock().thread_tids();
        let mut pt = process_table::THE.lock();
        for tid in all_tids {
            pt.kill(tid, exit_status);
        }
    }

    pub fn stop_current_process(&mut self, stop_sig: u32) {
        let (parent_tid, all_tids) = self
            .current_thread
            .with_lock(|t| (t.parent_tid(), t.process().lock().thread_tids()));
        debug!("stop_current_process: sig={stop_sig} parent={parent_tid} tids={all_tids:?}");
        process_table::THE.with_lock(|mut pt| {
            for &tid in &all_tids {
                if let Some(thread) = pt.get_thread(tid) {
                    thread.with_lock(|mut t| {
                        if !matches!(t.get_state(), ThreadState::Zombie(_)) {
                            t.set_state(ThreadState::Stopped);
                            t.stopped_notified = false;
                            t.stop_signal = stop_sig;
                        }
                    });
                }
            }
            pt.send_signal(parent_tid, headers::syscall_types::SIGCHLD);
            pt.wake_wait_wakers();
        });
        Cpu::current().ipi_to_all_but_me();
    }

    pub fn send_tty_signal(&mut self, sig: u32, fg_pgid: common::pid::Tid) {
        debug!("send_tty_signal: sig={sig} fg_pgid={fg_pgid}");
        process_table::THE.with_lock(|mut pt| {
            pt.send_signal_to_pgid(fg_pgid, sig);
        });
        self.schedule();
    }

    fn queue_current_process_back(&mut self) {
        if self.current_thread.lock().get_tid() == POWERSAVE_TID {
            debug!("Current thread is already powersave thread - don't queuing back");
            return;
        }
        let cpu_id = Cpu::cpu_id();
        let old = self.swap_current_with_powersave();
        let should_requeue = old.with_lock(|mut t| {
            if t.get_state() == (ThreadState::Running { cpu_id }) {
                t.set_state(ThreadState::Runnable);
                t.set_program_counter(VirtAddr::new(sys::cpu::read_sepc()));
                t.set_register_state(Cpu::read_trap_frame());
                debug!("Saved thread {} back", *t);
                true
            } else {
                false
            }
        });
        if should_requeue {
            process_table::RUN_QUEUE.lock().push_back(old);
        }
    }

    fn prepare_next_process(&mut self) -> ProcessMode {
        loop {
            self.queue_current_process_back();

            if process_table::is_empty() {
                info!("No more processes to schedule, shutting down system");
                qemu_exit::exit_success();
            }

            debug!("Getting next runnable process");

            assert!(
                self.is_current_process_energy_saver(),
                "Current thread must be powersave thread to not have any dangling references"
            );

            let next = process_table::RUN_QUEUE.lock().pop_front();
            if let Some(candidate) = next {
                let accepted = candidate.with_lock(|mut t| {
                    if t.get_state() == ThreadState::Runnable {
                        t.set_state(ThreadState::Running {
                            cpu_id: Cpu::cpu_id(),
                        });
                        true
                    } else {
                        false
                    }
                });
                if accepted {
                    debug!("Next runnable is {}", *candidate.lock());
                    self.current_thread = candidate;
                } else {
                    // Stale entry (killed/waiting since enqueued), discard and retry
                    continue;
                }
            } else {
                self.powersave_thread.with_lock(|mut t| {
                    t.set_state(ThreadState::Running {
                        cpu_id: Cpu::cpu_id(),
                    });
                });
                debug!("Next runnable is powersave");
            }

            // Acquire the thread lock once for both task check and register load,
            // eliminating the gap where a thread could be killed between the two.
            enum PrepareResult {
                Mode(ProcessMode),
                Terminate(super::signal::ExitStatus),
                Stop(u32),
                Dead,
            }
            let result = self.current_thread.with_lock(|mut t| {
                if let Some(task) = t.take_syscall_task() {
                    return PrepareResult::Mode(ProcessMode::KernelSyscallTask(task));
                }
                if matches!(t.get_state(), ThreadState::Zombie(_) | ThreadState::Stopped) {
                    return PrepareResult::Dead;
                }
                // Deliver pending signals before returning to userspace
                match super::signal::deliver_signal(&mut t) {
                    super::signal::SignalDeliveryResult::Terminate(exit_status) => {
                        return PrepareResult::Terminate(exit_status);
                    }
                    super::signal::SignalDeliveryResult::Stop(sig) => {
                        return PrepareResult::Stop(sig);
                    }
                    super::signal::SignalDeliveryResult::Continue => {}
                }
                let cpu_id = Cpu::cpu_id();
                assert!(
                    t.get_state() == ThreadState::Running { cpu_id },
                    "Thread {} not assigned to this CPU (state: {:?}, expected cpu: {})",
                    t.get_tid(),
                    t.get_state(),
                    cpu_id
                );
                let pc = t.get_program_counter();
                Cpu::write_trap_frame(t.get_register_state().clone());
                sys::cpu::write_sepc(pc.as_usize());
                sys::cpu::set_ret_to_kernel_mode(t.get_in_kernel_mode());
                PrepareResult::Mode(ProcessMode::Userspace)
            });
            match result {
                PrepareResult::Mode(mode) => return mode,
                PrepareResult::Terminate(exit_status) => {
                    let tid = self.current_thread.lock().get_tid();
                    process_table::THE.lock().kill_process_of(tid, exit_status);
                }
                PrepareResult::Stop(sig) => {
                    self.stop_current_process(sig);
                }
                PrepareResult::Dead => {}
            }
        }
    }

    pub fn set_cpu_reg_for_current_thread(&self) -> bool {
        self.current_thread.with_lock(|t| {
            let cpu_id = Cpu::cpu_id();
            if matches!(t.get_state(), ThreadState::Zombie(_) | ThreadState::Stopped) {
                debug!(
                    "Thread {} was killed/stopped during scheduling, rescheduling",
                    t.get_tid()
                );
                return false;
            }
            assert!(
                t.get_state() == ThreadState::Running { cpu_id },
                "Thread {} not assigned to this CPU (state: {:?}, expected cpu: {})",
                t.get_tid(),
                t.get_state(),
                cpu_id
            );

            let pc = t.get_program_counter();
            Cpu::write_trap_frame(t.get_register_state().clone());
            sys::cpu::write_sepc(pc.as_usize());
            sys::cpu::set_ret_to_kernel_mode(t.get_in_kernel_mode());
            true
        })
    }

    fn swap_current_with_powersave(&mut self) -> ThreadRef {
        core::mem::replace(&mut self.current_thread, self.powersave_thread.clone())
    }
}
