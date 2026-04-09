use core::ffi::{c_int, c_uint};
use headers::{
    errno::Errno,
    syscall_types::{
        _NSIG, SIG_BLOCK, SIG_SETMASK, SIG_UNBLOCK, SIGKILL, SIGSTOP, sigaction, sigset_t, stack_t,
    },
};

use crate::{processes::process_table, syscalls::linux_validator::LinuxUserspaceArg};
use common::pid::Tid;

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
}
