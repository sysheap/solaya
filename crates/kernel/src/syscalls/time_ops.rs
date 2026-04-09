use core::ffi::{c_int, c_uint};
use headers::{
    errno::Errno,
    syscall_types::{CLOCK_MONOTONIC, CLOCK_REALTIME, pollfd, sigset_t, timespec},
};

use crate::{processes::timer, syscalls::linux_validator::LinuxUserspaceArg};

use super::linux::LinuxSyscallHandler;

impl LinuxSyscallHandler {
    pub(super) async fn do_nanosleep(
        &self,
        duration: LinuxUserspaceArg<*const timespec>,
    ) -> Result<isize, Errno> {
        let duration = duration.validate_ptr()?;
        if duration.tv_sec < 0 || !(0..=999999999).contains(&duration.tv_nsec) {
            return Err(Errno::EINVAL);
        }
        timer::sleep(&duration)?.await;
        Ok(0)
    }

    pub(super) async fn do_clock_nanosleep(
        &self,
        clockid: c_int,
        flags: c_int,
        request: LinuxUserspaceArg<*const timespec>,
    ) -> Result<isize, Errno> {
        assert!(
            clockid == CLOCK_MONOTONIC as c_int || clockid == CLOCK_REALTIME as c_int,
            "clock_nanosleep: unsupported clockid {clockid}"
        );
        assert!(flags == 0, "clock_nanosleep: unsupported flags {flags}");
        let duration = request.validate_ptr()?;
        if duration.tv_sec < 0 || !(0..=999999999).contains(&duration.tv_nsec) {
            return Err(Errno::EINVAL);
        }
        timer::sleep(&duration)?.await;
        Ok(0)
    }

    pub(super) fn do_ppoll(
        &self,
        fds: LinuxUserspaceArg<*mut pollfd>,
        n: c_uint,
        to: LinuxUserspaceArg<Option<*const timespec>>,
        mask: LinuxUserspaceArg<Option<*const sigset_t>>,
    ) -> Result<isize, Errno> {
        let mask = mask.validate_ptr()?;

        let old_mask = mask.map(|mask| self.current_thread.with_lock(|mut t| t.set_sigset(mask)));

        Self::validate_poll_timeout(to.validate_ptr()?);

        let poll_fds = fds.validate_slice(n as usize)?;
        for pfd in &poll_fds {
            self.current_process
                .with_lock(|p| p.fd_table().get_descriptor(pfd.fd))?;
            assert_eq!(
                pfd.events, 0,
                "No further events are supported at the moment"
            );
        }

        if let Some(mask) = old_mask {
            self.current_thread.with_lock(|mut t| t.set_sigset(mask));
        }

        Ok(0)
    }
}
