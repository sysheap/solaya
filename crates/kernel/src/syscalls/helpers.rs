use alloc::string::String;
use headers::{errno::Errno, syscall_types::timespec};

use crate::{
    fs, klibc::consumable_buffer::ConsumableBuffer, syscalls::linux_validator::LinuxUserspaceArg,
};

use super::linux::LinuxSyscallHandler;

impl LinuxSyscallHandler {
    pub(super) fn resolve_dirfd_node(&self, dirfd: i32) -> Result<fs::vfs::VfsNodeRef, Errno> {
        let file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(dirfd))?;
        Ok(file.lock().node().clone())
    }

    pub(super) fn read_path(&self, arg: &LinuxUserspaceArg<*const u8>) -> Result<String, Errno> {
        let path = self.read_cstring(arg)?;
        Ok(self.make_absolute(&path))
    }

    pub(super) fn make_absolute(&self, path: &str) -> String {
        if path.starts_with('/') {
            return String::from(path);
        }
        let cwd = self.current_process.with_lock(|p| String::from(p.cwd()));
        if cwd == "/" {
            alloc::format!("/{path}")
        } else {
            alloc::format!("{cwd}/{path}")
        }
    }

    pub(super) fn read_cstring(&self, arg: &LinuxUserspaceArg<*const u8>) -> Result<String, Errno> {
        let addr = arg.raw_arg();
        let max_len = 256usize.min(usize::MAX - addr + 1);
        let bytes = arg.validate_slice(max_len)?;
        let mut buf = ConsumableBuffer::new(&bytes);
        let s = buf.consume_str().ok_or(Errno::EFAULT)?;
        Ok(String::from(s))
    }

    pub(super) fn validate_poll_timeout(to: Option<timespec>) {
        if let Some(to) = to {
            assert_eq!(to.tv_sec, 0, "ppoll with timeout not yet implemented");
            assert_eq!(to.tv_nsec, 0, "ppoll with timeout not yet implemented");
        }
    }
}
