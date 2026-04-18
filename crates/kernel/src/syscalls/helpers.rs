use alloc::string::String;
use headers::{errno::Errno, syscall_types::timespec};

use crate::{fs, syscalls::linux_validator::LinuxUserspaceArg};
use klib::parser::ConsumableBuffer;

use super::linux::LinuxSyscallHandler;

/// Compose `base_abs` + `/` + `path` into a canonicalized absolute
/// path. If `path` is already absolute it wins, matching openat(2)
/// semantics (dirfd is ignored for absolute paths).
pub(super) fn compose_abs(base_abs: &str, path: &str) -> String {
    let raw = if path.starts_with('/') {
        String::from(path)
    } else if base_abs.ends_with('/') {
        alloc::format!("{base_abs}{path}")
    } else {
        alloc::format!("{base_abs}/{path}")
    };
    fs::vfs::canonicalize_path(&raw)
}

impl LinuxSyscallHandler {
    /// Resolve a dirfd to both the directory node and the absolute
    /// path it was opened with. Callers need the path so
    /// [`fs::resolve_relative`] can rebase `..` and relative symlinks
    /// against the dirfd's real location, not against `/`.
    pub(super) fn resolve_dirfd_node(
        &self,
        dirfd: i32,
    ) -> Result<(fs::vfs::VfsNodeRef, String), Errno> {
        let file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(dirfd))?;
        let inner = file.lock();
        Ok((inner.node().clone(), String::from(inner.abs_path())))
    }

    /// Resolve the base directory for an `*at`-style syscall: either
    /// the process cwd (AT_FDCWD) or the directory referred to by
    /// `dirfd`. Returns the node and its absolute path.
    pub(super) fn resolve_openat_base(
        &self,
        dirfd: i32,
    ) -> Result<(fs::vfs::VfsNodeRef, String), Errno> {
        if dirfd == headers::fs::AT_FDCWD {
            let cwd = self.current_process.with_lock(|p| String::from(p.cwd()));
            let node = fs::resolve_path(&cwd)?;
            Ok((node, cwd))
        } else {
            self.resolve_dirfd_node(dirfd)
        }
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
