use alloc::{string::String, vec};
use core::ffi::{c_int, c_uint};
use headers::{
    errno::Errno,
    syscall_types::{O_CREAT, O_DIRECTORY, O_EXCL, O_TRUNC},
};

use crate::{
    fs, klibc::util::ByteInterpretable, processes::fd_table::FileDescriptor,
    syscalls::linux_validator::LinuxUserspaceArg,
};

use super::linux::LinuxSyscallHandler;

impl LinuxSyscallHandler {
    pub(super) fn do_openat(
        &self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        flags: c_int,
        _mode: c_uint,
    ) -> Result<isize, Errno> {
        let raw_path = self.read_cstring(&pathname)?;
        let flags_u32 = flags.cast_unsigned();

        let resolve = |path: &str| -> Result<fs::vfs::VfsNodeRef, Errno> {
            if dirfd == headers::fs::AT_FDCWD {
                let abs = self.make_absolute(path);
                fs::resolve_path(&abs)
            } else {
                fs::resolve_relative(self.resolve_dirfd_node(dirfd)?, path)
            }
        };

        let node = match resolve(&raw_path) {
            Ok(n) => {
                if (flags_u32 & O_EXCL) != 0 && (flags_u32 & O_CREAT) != 0 {
                    return Err(Errno::EEXIST);
                }
                if (flags_u32 & O_TRUNC) != 0 {
                    n.truncate()?;
                }
                n
            }
            Err(Errno::ENOENT) if (flags_u32 & O_CREAT) != 0 => {
                if dirfd == headers::fs::AT_FDCWD {
                    let abs = self.make_absolute(&raw_path);
                    let (parent, name) = fs::resolve_parent(&abs)?;
                    parent.create(name, fs::vfs::NodeType::File)?
                } else {
                    let trimmed = raw_path.trim_end_matches('/');
                    let (base, name) = if let Some(slash) = trimmed.rfind('/') {
                        let dir_node = fs::resolve_relative(
                            self.resolve_dirfd_node(dirfd)?,
                            &trimmed[..slash],
                        )?;
                        (dir_node, &trimmed[slash + 1..])
                    } else {
                        (self.resolve_dirfd_node(dirfd)?, trimmed)
                    };
                    base.create(name, fs::vfs::NodeType::File)?
                }
            }
            Err(e) => return Err(e),
        };

        if (flags_u32 & O_DIRECTORY) != 0 && node.node_type() != fs::vfs::NodeType::Directory {
            return Err(Errno::ENOTDIR);
        }

        let open_file = fs::open_file::open(node, flags);
        let fd = self
            .current_process
            .with_lock(|p| p.fd_table().allocate(FileDescriptor::VfsFile(open_file)))?;
        Ok(fd as isize)
    }

    pub(super) fn do_fstat(
        &self,
        fd: c_int,
        statbuf: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get_descriptor(fd))?;

        let st = match &descriptor {
            FileDescriptor::VfsFile(file) => {
                let node = file.lock().node().clone();
                fs::stat_from_node(&node)
            }
            _ => headers::fs::stat {
                st_mode: headers::fs::S_IFCHR | 0o666,
                st_nlink: 1,
                st_blksize: 4096,
                ..headers::fs::stat::default()
            },
        };

        statbuf.write_slice(st.as_slice())?;
        Ok(0)
    }

    fn resolve_stat_node(
        &self,
        dirfd: c_int,
        pathname: &LinuxUserspaceArg<*const u8>,
        flags: c_int,
    ) -> Result<fs::vfs::VfsNodeRef, Errno> {
        if (flags & headers::fs::AT_EMPTY_PATH) != 0 && !pathname.arg_nonzero() {
            let file = self
                .current_process
                .with_lock(|p| {
                    p.fd_table().get(dirfd).and_then(|e| match &e.descriptor {
                        FileDescriptor::VfsFile(f) => Some(f.clone()),
                        _ => None,
                    })
                })
                .ok_or(Errno::EBADF)?;
            Ok(file.lock().node().clone())
        } else if dirfd == headers::fs::AT_FDCWD {
            let path = self.read_path(pathname)?;
            fs::resolve_path(&path)
        } else {
            let path = self.read_cstring(pathname)?;
            fs::resolve_relative(self.resolve_dirfd_node(dirfd)?, &path)
        }
    }

    pub(super) fn do_newfstatat(
        &self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        statbuf: LinuxUserspaceArg<*mut u8>,
        flags: c_int,
    ) -> Result<isize, Errno> {
        let node = self.resolve_stat_node(dirfd, &pathname, flags)?;
        statbuf.write_slice(fs::stat_from_node(&node).as_slice())?;
        Ok(0)
    }

    pub(super) fn do_statx(
        &self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        flags: c_int,
        _mask: c_uint,
        statxbuf: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let node = self.resolve_stat_node(dirfd, &pathname, flags)?;
        statxbuf.write_slice(fs::statx_from_node(&node).as_slice())?;
        Ok(0)
    }

    pub(super) fn do_getdents64(
        &self,
        fd: c_int,
        dirp: LinuxUserspaceArg<*mut u8>,
        count: usize,
    ) -> Result<isize, Errno> {
        let file = self
            .current_process
            .with_lock(|p| {
                p.fd_table().get(fd).and_then(|e| match &e.descriptor {
                    FileDescriptor::VfsFile(f) => Some(f.clone()),
                    _ => None,
                })
            })
            .ok_or(Errno::EBADF)?;

        let mut inner = file.lock();
        if !inner.is_directory() {
            return Err(Errno::ENOTDIR);
        }

        let entries = inner.node().readdir()?;
        let start_offset = inner.offset();

        let mut out = vec![0u8; count];
        let mut pos = 0usize;
        let mut entry_idx = start_offset;

        for entry in entries.iter().skip(start_offset) {
            let name_bytes = entry.name.as_bytes();
            let name_len = name_bytes.len() + 1; // +1 for null terminator
            let header_size = core::mem::size_of::<headers::fs::linux_dirent64>();
            let reclen = (header_size + name_len + 7) & !7; // align to 8

            if pos + reclen > count {
                break;
            }

            let d_type = match entry.node_type {
                fs::vfs::NodeType::File => headers::fs::DT_REG,
                fs::vfs::NodeType::Directory => headers::fs::DT_DIR,
            };

            entry_idx += 1;

            out[pos..pos + 8].copy_from_slice(&entry.ino.to_le_bytes());
            out[pos + 8..pos + 16].copy_from_slice(&(entry_idx as i64).to_le_bytes());
            let reclen_u16 = reclen as u16;
            out[pos + 16..pos + 18].copy_from_slice(&reclen_u16.to_le_bytes());
            out[pos + 18] = d_type;
            out[pos + 19..pos + 19 + name_bytes.len()].copy_from_slice(name_bytes);
            out[pos + 19 + name_bytes.len()] = 0;

            pos += reclen;
        }

        inner.seek(entry_idx as isize, headers::fs::SEEK_SET)?;

        if pos > 0 {
            dirp.write_slice(&out[..pos])?;
        }
        Ok(pos as isize)
    }

    pub(super) fn do_faccessat(
        &self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        _mode: c_int,
    ) -> Result<isize, Errno> {
        let _node = if dirfd == headers::fs::AT_FDCWD {
            let path = self.read_path(&pathname)?;
            fs::resolve_path(&path)?
        } else {
            let path = self.read_cstring(&pathname)?;
            fs::resolve_relative(self.resolve_dirfd_node(dirfd)?, &path)?
        };
        Ok(0)
    }

    pub(super) fn do_mkdirat(
        &self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        _mode: c_uint,
    ) -> Result<isize, Errno> {
        if dirfd != headers::fs::AT_FDCWD {
            return Err(Errno::ENOSYS);
        }
        let path = self.read_path(&pathname)?;
        let (parent, name) = fs::resolve_parent(&path)?;
        parent.create(name, fs::vfs::NodeType::Directory)?;
        Ok(0)
    }

    pub(super) fn do_unlinkat(
        &self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        flags: c_int,
    ) -> Result<isize, Errno> {
        let path = self.read_cstring(&pathname)?;
        let path = path.trim_end_matches('/');

        let (parent, name) = if dirfd == headers::fs::AT_FDCWD {
            let abs = self.read_path(&pathname)?;
            let (p, n) = fs::resolve_parent(&abs)?;
            (p, String::from(n))
        } else if let Some(slash) = path.rfind('/') {
            let parent_path = &path[..slash];
            let name = &path[slash + 1..];
            let base = self.resolve_dirfd_node(dirfd)?;
            (fs::resolve_relative(base, parent_path)?, String::from(name))
        } else {
            (self.resolve_dirfd_node(dirfd)?, String::from(path))
        };

        if (flags & headers::fs::AT_REMOVEDIR) != 0 {
            let node = parent.lookup(&name)?;
            if node.node_type() != fs::vfs::NodeType::Directory {
                return Err(Errno::ENOTDIR);
            }
            if !node.readdir()?.is_empty() {
                return Err(Errno::ENOTEMPTY);
            }
        } else {
            let node = parent.lookup(&name)?;
            if node.node_type() == fs::vfs::NodeType::Directory {
                return Err(Errno::EISDIR);
            }
        }

        parent.unlink(&name)?;
        Ok(0)
    }

    pub(super) fn do_getcwd(
        &self,
        buf: LinuxUserspaceArg<*mut u8>,
        size: usize,
    ) -> Result<isize, Errno> {
        let cwd = self.current_process.with_lock(|p| String::from(p.cwd()));
        let needed = cwd.len() + 1;
        if size < needed {
            return Err(Errno::ERANGE);
        }
        let mut bytes: alloc::vec::Vec<u8> = cwd.into_bytes();
        bytes.push(0);
        buf.write_slice(&bytes)?;
        Ok(needed as isize)
    }

    fn make_statfs_reply() -> headers::fs::statfs {
        headers::fs::statfs {
            f_type: 0x01021994, // TMPFS_MAGIC
            f_bsize: 4096,
            f_namelen: 255,
            f_frsize: 4096,
            ..headers::fs::statfs::default()
        }
    }

    pub(super) fn do_statfs(
        &self,
        pathname: LinuxUserspaceArg<*const u8>,
        buf: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let path = self.read_path(&pathname)?;
        let _node = fs::resolve_path(&path)?;
        buf.write_slice(Self::make_statfs_reply().as_slice())?;
        Ok(0)
    }

    pub(super) fn do_fstatfs(
        &self,
        fd: c_int,
        buf: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let _file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(fd))?;
        buf.write_slice(Self::make_statfs_reply().as_slice())?;
        Ok(0)
    }
}
