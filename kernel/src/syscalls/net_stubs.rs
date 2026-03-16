use core::ffi::{c_int, c_uint};
use headers::errno::Errno;

use super::{linux::LinuxSyscallHandler, linux_validator::LinuxUserspaceArg};

impl LinuxSyscallHandler {
    pub(super) fn do_socket(&self, _: c_int, _: c_int, _: c_int) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) fn do_bind(
        &self,
        _: c_int,
        _: LinuxUserspaceArg<*const u8>,
        _: c_uint,
    ) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) fn do_sendto(
        &self,
        _: c_int,
        _: LinuxUserspaceArg<*const u8>,
        _: usize,
        _: c_int,
        _: LinuxUserspaceArg<*const u8>,
        _: c_uint,
    ) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) async fn do_recvfrom(
        &self,
        _: c_int,
        _: LinuxUserspaceArg<*mut u8>,
        _: usize,
        _: c_int,
        _: LinuxUserspaceArg<Option<*mut u8>>,
        _: LinuxUserspaceArg<Option<*mut c_uint>>,
    ) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) async fn do_connect(
        &self,
        _: c_int,
        _: LinuxUserspaceArg<*const u8>,
        _: c_uint,
    ) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) fn do_listen(&self, _: c_int) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) async fn do_accept(
        &self,
        _: c_int,
        _: LinuxUserspaceArg<Option<*mut u8>>,
        _: LinuxUserspaceArg<Option<*mut c_uint>>,
    ) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) fn do_getsockname(
        &self,
        _: c_int,
        _: LinuxUserspaceArg<*mut u8>,
        _: LinuxUserspaceArg<*mut c_uint>,
    ) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) fn do_getpeername(
        &self,
        _: c_int,
        _: LinuxUserspaceArg<*mut u8>,
        _: LinuxUserspaceArg<*mut c_uint>,
    ) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }

    pub(super) fn do_shutdown(&self, _: c_int) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }
}
