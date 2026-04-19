use core::{
    ffi::{c_int, c_uint},
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use headers::{
    errno::Errno,
    syscall_types::{
        _NSIG, SIG_BLOCK, SIG_SETMASK, SIG_UNBLOCK, SIGKILL, SIGSTOP, sigaction, sigset_t, stack_t,
        timespec,
    },
};

use crate::{
    processes::{process_table, thread::ThreadRef},
    syscalls::linux_validator::LinuxUserspaceArg,
};
use abi::pid::Tid;

use super::linux::LinuxSyscallHandler;

impl LinuxSyscallHandler {
    pub(super) fn do_rt_sigaction(
        &self,
        sig: c_uint,
        act: LinuxUserspaceArg<Option<*const sigaction>>,
        oact: LinuxUserspaceArg<Option<*mut sigaction>>,
        sigsetsize: usize,
    ) -> Result<isize, Errno> {
        if sigsetsize != core::mem::size_of::<sigset_t>()
            || matches!(sig, SIGKILL | SIGSTOP)
            || sig >= _NSIG
        {
            return Err(Errno::EINVAL);
        }

        let old_act = if let Some(act) = act.validate_ptr()? {
            self.current_thread
                .with_lock(|mut t| t.set_sigaction(sig, act))
        } else {
            self.current_thread.with_lock(|t| t.get_sigaction(sig))
        }?;

        oact.write_if_not_none(old_act)?;
        Ok(0)
    }

    pub(super) fn do_rt_sigprocmask(
        &self,
        how: c_uint,
        set: LinuxUserspaceArg<Option<*const sigset_t>>,
        oldset: LinuxUserspaceArg<Option<*mut sigset_t>>,
        sigsetsize: usize,
    ) -> Result<isize, Errno> {
        if sigsetsize != core::mem::size_of::<sigset_t>() {
            return Err(Errno::EINVAL);
        }

        let new_set = set.validate_ptr()?;

        let old_set_in_thread = if let Some(new_set) = new_set {
            self.current_thread.with_lock(|mut t| {
                let mut current_set = t.get_sigset();
                match how {
                    SIG_BLOCK => current_set.sig[0] |= new_set.sig[0],
                    SIG_UNBLOCK => current_set.sig[0] &= !new_set.sig[0],
                    SIG_SETMASK => current_set.sig[0] = new_set.sig[0],
                    _ => {
                        return Err(Errno::EINVAL);
                    }
                }
                Ok(t.set_sigset(current_set))
            })?
        } else {
            self.current_thread.with_lock(|t| t.get_sigset())
        };

        oldset.write_if_not_none(old_set_in_thread)?;
        Ok(0)
    }

    pub(super) fn do_sigaltstack(
        &self,
        uss: LinuxUserspaceArg<Option<*const stack_t>>,
        uoss: LinuxUserspaceArg<Option<*mut stack_t>>,
    ) -> Result<isize, Errno> {
        let uss = uss.validate_ptr()?;
        self.current_thread.with_lock(|mut t| {
            let old = t.get_sigaltstack();
            if let Some(uss) = uss {
                t.set_sigaltstack(&uss);
            }
            uoss.write_if_not_none(old)?;
            Ok::<(), Errno>(())
        })?;
        Ok(0)
    }

    pub(super) fn do_kill(&self, pid: c_int, sig: c_int) -> Result<isize, Errno> {
        let sig = crate::processes::signal::validate_signal(sig)?;
        if pid > 0 {
            let target_tid = Tid::try_from_i32(pid).ok_or(Errno::ESRCH)?;
            if let Some(sig) = sig {
                process_table::THE
                    .lock()
                    .send_signal_to_process(target_tid, sig);
            }
        } else if pid == 0 {
            let pgid = self.current_process.with_lock(|p| p.pgid());
            if let Some(sig) = sig {
                process_table::THE.lock().send_signal_to_pgid(pgid, sig);
            }
        } else if pid == -1 {
            return Err(Errno::ESRCH);
        } else {
            // pid < -1: send to process group abs(pid)
            let pgid = Tid::try_from_i32(-pid).ok_or(Errno::ESRCH)?;
            if let Some(sig) = sig {
                process_table::THE.lock().send_signal_to_pgid(pgid, sig);
            }
        }
        Ok(0)
    }

    pub(super) fn do_rt_sigreturn(&self) -> Result<isize, Errno> {
        self.current_thread.with_lock(|mut t| {
            crate::processes::signal::restore_signal_frame(&mut t)?;
            t.set_registers_replaced(true);
            Ok::<_, Errno>(())
        })?;
        Ok(0)
    }

    pub(super) async fn do_rt_sigtimedwait(
        &self,
        set: LinuxUserspaceArg<*const sigset_t>,
        info: LinuxUserspaceArg<Option<*mut u8>>,
        timeout: LinuxUserspaceArg<Option<*const timespec>>,
        sigsetsize: usize,
    ) -> Result<isize, Errno> {
        if sigsetsize != core::mem::size_of::<sigset_t>() {
            return Err(Errno::EINVAL);
        }
        // NULL-info is the only supported caller path for now.
        if info.arg_nonzero() {
            return Err(Errno::EINVAL);
        }
        let set = set.validate_ptr()?;
        // SIGKILL/SIGSTOP cannot be waited for — strip them from the wait set.
        let wait_mask = set.sig[0] & !(1u64 << SIGKILL) & !(1u64 << SIGSTOP);

        if let Some(t) = timeout.validate_ptr()? {
            if t.tv_sec == 0 && t.tv_nsec == 0 {
                // Poll: dequeue a matching pending signal, or EAGAIN.
                return self.current_thread.with_lock(|mut th| {
                    match th.first_pending_in_set(wait_mask) {
                        Some(sig) => {
                            th.clear_pending(sig);
                            Ok(sig as isize)
                        }
                        None => Err(Errno::EAGAIN),
                    }
                });
            }
            // Finite non-zero timeouts are out of scope for now.
            return Err(Errno::EINVAL);
        }

        SigTimedWait {
            thread: self.current_thread.clone(),
            wait_mask,
        }
        .await
    }
}

struct SigTimedWait {
    thread: ThreadRef,
    wait_mask: u64,
}

impl Future for SigTimedWait {
    type Output = Result<isize, Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.thread.with_lock(|mut t| {
            if let Some(sig) = t.first_pending_in_set(self.wait_mask) {
                t.clear_pending(sig);
                return Poll::Ready(Ok(sig as isize));
            }
            // If an unblocked signal not in set is pending, the scheduler's
            // Interrupt path will deliver EINTR after we return Pending.
            // Register our waker so send_signal wakes us for blocked signals
            // in `set`.
            t.register_signal_waker(cx.waker().clone());
            Poll::Pending
        })
    }
}
