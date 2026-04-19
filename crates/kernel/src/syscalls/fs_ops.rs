use alloc::{string::String, vec};
use core::ffi::{c_int, c_uint};
use headers::{
    errno::Errno,
    syscall_types::{O_CREAT, O_DIRECTORY, O_EXCL, O_TRUNC},
};

use crate::{
    byte_interpretable::ByteInterpretable, fs, processes::fd_table::FileDescriptor,
    syscalls::linux_validator::LinuxUserspaceArg,
};

use super::{helpers::compose_abs, linux::LinuxSyscallHandler};

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

        let (base_node, base_abs) = self.resolve_openat_base(dirfd)?;

        let resolve = |path: &str| -> Result<fs::vfs::VfsNodeRef, Errno> {
            if path.starts_with('/') {
                fs::resolve_path(path)
            } else {
                fs::resolve_relative(base_node.clone(), &base_abs, path)
            }
        };

        let node = match resolve(&raw_path) {
            Ok(n) => {
                if (flags_u32 & O_EXCL) != 0 && (flags_u32 & O_CREAT) != 0 {
                    return Err(Errno::EEXIST);
                }
                if (flags_u32 & O_TRUNC) != 0 {
                    n.truncate(0)?;
                }
                n
            }
            Err(Errno::ENOENT) if (flags_u32 & O_CREAT) != 0 => {
                let trimmed = raw_path.trim_end_matches('/');
                let (parent, name) = if trimmed.starts_with('/') {
                    let (p, n) = fs::resolve_parent(trimmed)?;
                    (p, String::from(n))
                } else if let Some(slash) = trimmed.rfind('/') {
                    let dir_node =
                        fs::resolve_relative(base_node.clone(), &base_abs, &trimmed[..slash])?;
                    (dir_node, String::from(&trimmed[slash + 1..]))
                } else {
                    (base_node.clone(), String::from(trimmed))
                };
                parent.create(&name, fs::vfs::NodeType::File)?
            }
            Err(e) => return Err(e),
        };

        if (flags_u32 & O_DIRECTORY) != 0 && node.node_type() != fs::vfs::NodeType::Directory {
            return Err(Errno::ENOTDIR);
        }

        let descriptor = if let Some(dev) = node.char_device()
            && dev.is_tty()
        {
            // Implicit-ctty stop-gap: grant the opener's pgid the console's
            // fg_pgid so dash's job-control startup doesn't self-stop via
            // SIGTTIN. Proper TIOCSCTTY-on-open is tracked in issue #262.
            let caller_pgid = self.current_process.with_lock(|p| p.pgid());
            crate::io::tty_device::console_tty()
                .lock()
                .set_fg_pgid(caller_pgid);
            FileDescriptor::Tty(crate::io::tty_device::console_tty().clone())
        } else {
            let fd_abs = compose_abs(&base_abs, &raw_path);
            FileDescriptor::VfsFile(fs::open_file::open(node, flags, fd_abs))
        };
        let fd = self
            .current_process
            .with_lock(|p| p.fd_table().allocate(descriptor))?;
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
        // TODO: We should have a better rust abstractions to check for set bitfields.
        let nofollow = (flags & headers::fs::AT_SYMLINK_NOFOLLOW) != 0;
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
            if nofollow {
                fs::resolve_path_nofollow(&path)
            } else {
                fs::resolve_path(&path)
            }
        } else {
            let path = self.read_cstring(pathname)?;
            let (base, base_abs) = self.resolve_dirfd_node(dirfd)?;
            if nofollow {
                let (parent_part, name) = if let Some(slash) = path.rfind('/') {
                    (Some(&path[..slash]), &path[slash + 1..])
                } else {
                    (None, path.as_str())
                };
                let parent = if let Some(pp) = parent_part {
                    fs::resolve_relative(base, &base_abs, pp)?
                } else {
                    base
                };
                parent.lookup(name)
            } else {
                fs::resolve_relative(base, &base_abs, &path)
            }
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
                fs::vfs::NodeType::Symlink => headers::fs::DT_LNK,
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
            let (base, base_abs) = self.resolve_dirfd_node(dirfd)?;
            fs::resolve_relative(base, &base_abs, &path)?
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
            let (base, base_abs) = self.resolve_dirfd_node(dirfd)?;
            (
                fs::resolve_relative(base, &base_abs, parent_path)?,
                String::from(name),
            )
        } else {
            let (base, _base_abs) = self.resolve_dirfd_node(dirfd)?;
            (base, String::from(path))
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

    pub(super) fn do_truncate(
        &self,
        pathname: LinuxUserspaceArg<*const u8>,
        length: isize,
    ) -> Result<isize, Errno> {
        if length < 0 {
            return Err(Errno::EINVAL);
        }
        let path = self.read_path(&pathname)?;
        let node = fs::resolve_path(&path)?;
        node.truncate(length.cast_unsigned())?;
        Ok(0)
    }

    pub(super) fn do_ftruncate(&self, fd: c_int, length: isize) -> Result<isize, Errno> {
        if length < 0 {
            return Err(Errno::EINVAL);
        }
        let file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(fd))?;
        file.lock().node().truncate(length.cast_unsigned())?;
        Ok(0)
    }

    pub(super) fn do_fchmod(&self, fd: c_int, mode: c_uint) -> Result<isize, Errno> {
        let file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(fd))?;
        let node = file.lock().node().clone();
        let current = node.mode();
        node.set_mode((current & !0o7777) | (mode & 0o7777))?;
        Ok(0)
    }

    pub(super) fn do_fchmodat(
        &self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        mode: c_uint,
    ) -> Result<isize, Errno> {
        let node = self.resolve_path_from_dirfd(dirfd, &pathname)?;
        let current = node.mode();
        node.set_mode((current & !0o7777) | (mode & 0o7777))?;
        Ok(0)
    }

    pub(super) fn do_fchown(&self, fd: c_int, uid: c_uint, gid: c_uint) -> Result<isize, Errno> {
        let file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(fd))?;
        let node = file.lock().node().clone();
        let mut actual_uid = node.uid();
        let mut actual_gid = node.gid();
        if uid != u32::MAX {
            actual_uid = uid;
        }
        if gid != u32::MAX {
            actual_gid = gid;
        }
        node.set_owner(actual_uid, actual_gid)?;
        Ok(0)
    }

    pub(super) fn do_fchownat(
        &self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        uid: c_uint,
        gid: c_uint,
    ) -> Result<isize, Errno> {
        let node = self.resolve_path_from_dirfd(dirfd, &pathname)?;
        let mut actual_uid = node.uid();
        let mut actual_gid = node.gid();
        if uid != u32::MAX {
            actual_uid = uid;
        }
        if gid != u32::MAX {
            actual_gid = gid;
        }
        node.set_owner(actual_uid, actual_gid)?;
        Ok(0)
    }

    fn resolve_path_from_dirfd(
        &self,
        dirfd: c_int,
        pathname: &LinuxUserspaceArg<*const u8>,
    ) -> Result<fs::vfs::VfsNodeRef, Errno> {
        if dirfd == headers::fs::AT_FDCWD {
            let path = self.read_path(pathname)?;
            fs::resolve_path(&path)
        } else {
            let path = self.read_cstring(pathname)?;
            let (base, base_abs) = self.resolve_dirfd_node(dirfd)?;
            fs::resolve_relative(base, &base_abs, &path)
        }
    }

    pub(super) fn do_readlinkat(
        &self,
        _dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        buf: LinuxUserspaceArg<*mut u8>,
        bufsiz: usize,
    ) -> Result<isize, Errno> {
        let path = self.read_path(&pathname)?;
        let node = fs::resolve_path_nofollow(&path)?;
        let target = node.readlink()?;
        let bytes = target.as_bytes();
        let n = bytes.len().min(bufsiz);
        buf.write_slice(&bytes[..n])?;
        Ok(n as isize)
    }

    pub(super) fn do_symlinkat(
        &self,
        target: LinuxUserspaceArg<*const u8>,
        linkpath: LinuxUserspaceArg<*const u8>,
    ) -> Result<isize, Errno> {
        let target_str = self.read_cstring(&target)?;
        let link_path = self.read_path(&linkpath)?;
        let (parent, name) = fs::resolve_parent(&link_path)?;
        let symlink = crate::fs::tmpfs::TmpfsSymlink::new(target_str);
        parent.link(name, symlink)?;
        Ok(0)
    }

    pub(super) fn do_linkat(
        &self,
        oldpath: LinuxUserspaceArg<*const u8>,
        newpath: LinuxUserspaceArg<*const u8>,
    ) -> Result<isize, Errno> {
        let old_path = self.read_path(&oldpath)?;
        let target_node = fs::resolve_path(&old_path)?;
        if target_node.node_type() == fs::vfs::NodeType::Directory {
            return Err(Errno::EPERM);
        }
        let new_path = self.read_path(&newpath)?;
        let (new_parent, new_name) = fs::resolve_parent(&new_path)?;
        new_parent.link(new_name, target_node.clone())?;
        target_node.inc_nlink();
        Ok(0)
    }

    pub(super) fn do_renameat2(
        &self,
        oldpath: LinuxUserspaceArg<*const u8>,
        newpath: LinuxUserspaceArg<*const u8>,
        flags: c_uint,
    ) -> Result<isize, Errno> {
        let old_path_str = self.read_path(&oldpath)?;
        let new_path_str = self.read_path(&newpath)?;
        let (old_parent, old_name) = fs::resolve_parent(&old_path_str)?;
        let (new_parent, new_name) = fs::resolve_parent(&new_path_str)?;

        if (flags & 1) != 0 && new_parent.lookup(new_name).is_ok() {
            return Err(Errno::EEXIST);
        }

        let node = old_parent.remove_child(old_name)?;
        let _ = new_parent.remove_child(new_name);
        new_parent.link(new_name, node)?;
        Ok(0)
    }
}
