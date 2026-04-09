use alloc::vec::Vec;
use core::ffi::{c_int, c_ulong};
use headers::{
    errno::Errno,
    syscall_types::{F_GETFL, F_SETFL, O_CLOEXEC, iovec},
};

use crate::{
    io::pipe,
    klibc::util::UsizeExt,
    processes::fd_table::{FdFlags, FileDescriptor},
    syscalls::linux_validator::LinuxUserspaceArg,
};

use super::linux::{LinuxSyscallHandler, LinuxSyscalls};

impl LinuxSyscallHandler {
    pub(super) async fn do_read(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*mut u8>,
        count: usize,
    ) -> Result<isize, Errno> {
        let (descriptor, flags) = self
            .current_process
            .with_lock(|p| p.fd_table().get_descriptor_and_flags(fd))?;

        let data = if flags.is_nonblocking() {
            descriptor.try_read(count)?
        } else {
            descriptor
                .read(count, self.current_process.clone(), self.current_tid)
                .await?
        };
        assert!(data.len() <= count, "Read more than requested");
        buf.write_slice(&data)?;

        Ok(data.len() as isize)
    }

    pub(super) async fn do_write(
        &self,
        fd: c_int,
        buf: LinuxUserspaceArg<*const u8>,
        count: usize,
    ) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get_descriptor(fd))?;
        let data = buf.validate_slice(count)?;
        descriptor.write(&data).await?;
        Ok(count as isize)
    }

    pub(super) async fn do_readv(
        &mut self,
        fd: c_int,
        iov: LinuxUserspaceArg<*const iovec>,
        iovcnt: c_int,
    ) -> Result<isize, Errno> {
        let (descriptor, flags) = self
            .current_process
            .with_lock(|p| p.fd_table().get_descriptor_and_flags(fd))?;

        let buffers: Vec<(usize, usize)> = {
            let iov = iov.validate_slice(usize::try_from(iovcnt).map_err(|_| Errno::EINVAL)?)?;
            iov.iter()
                .filter(|io| io.iov_len != 0)
                .map(|io| (io.iov_base as usize, io.iov_len.as_usize()))
                .collect()
        };
        let mut total = 0isize;

        for (base, count) in buffers {
            let data = if flags.is_nonblocking() {
                descriptor.try_read(count)?
            } else {
                descriptor
                    .read(count, self.current_process.clone(), self.current_tid)
                    .await?
            };
            let buf = LinuxUserspaceArg::<*mut u8>::new(base, self.get_process());
            buf.write_slice(&data)?;
            total += data.len() as isize;
            if data.len() < count {
                break;
            }
        }

        Ok(total)
    }

    pub(super) async fn do_writev(
        &self,
        fd: c_int,
        iov: LinuxUserspaceArg<*const iovec>,
        iovcnt: c_int,
    ) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get_descriptor(fd))?;

        let iov = iov.validate_slice(usize::try_from(iovcnt).map_err(|_| Errno::EINVAL)?)?;
        let mut data = Vec::new();

        for io in iov {
            if io.iov_len == 0 {
                continue;
            }
            let buf = LinuxUserspaceArg::<*const u8>::new(io.iov_base as usize, self.get_process());
            let mut buf = buf.validate_slice(io.iov_len.as_usize())?;
            data.append(&mut buf);
        }

        let len = data.len();
        descriptor.write(&data).await?;
        Ok(len as isize)
    }

    pub(super) async fn do_pread64(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*mut u8>,
        count: usize,
        offset: isize,
    ) -> Result<isize, Errno> {
        if offset < 0 {
            return Err(Errno::EINVAL);
        }
        let file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(fd))?;
        let mut tmp = alloc::vec![0u8; count];
        let n = file.lock().pread(offset.cast_unsigned(), &mut tmp)?;
        tmp.truncate(n);
        buf.write_slice(&tmp)?;
        Ok(n as isize)
    }

    pub(super) async fn do_pwrite64(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*const u8>,
        count: usize,
        offset: isize,
    ) -> Result<isize, Errno> {
        if offset < 0 {
            return Err(Errno::EINVAL);
        }
        let file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(fd))?;
        let data = buf.validate_slice(count)?;
        let n = file.lock().pwrite(offset.cast_unsigned(), &data)?;
        Ok(n as isize)
    }

    pub(super) async fn do_sendfile(
        &mut self,
        out_fd: c_int,
        in_fd: c_int,
        offset: LinuxUserspaceArg<Option<*mut isize>>,
        count: usize,
    ) -> Result<isize, Errno> {
        let in_file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(in_fd))?;
        let out_desc = self
            .current_process
            .with_lock(|p| p.fd_table().get_descriptor(out_fd))?;

        let mut total = 0usize;
        let buf_size = count.min(4096);
        let mut buf = alloc::vec![0u8; buf_size];

        if offset.raw_arg() != 0 {
            let offset_ptr =
                LinuxUserspaceArg::<*mut isize>::new(offset.raw_arg(), self.get_process());
            let mut off = offset_ptr.validate_slice(1)?[0];
            if off < 0 {
                return Err(Errno::EINVAL);
            }

            while total < count {
                let chunk = (count - total).min(buf_size);
                let n = in_file
                    .lock()
                    .pread(off.cast_unsigned(), &mut buf[..chunk])?;
                if n == 0 {
                    break;
                }
                out_desc.write(&buf[..n]).await?;
                off += n as isize;
                total += n;
            }

            offset_ptr.write_slice(&[off])?;
        } else {
            while total < count {
                let chunk = (count - total).min(buf_size);
                let n = in_file.lock().read(&mut buf[..chunk])?;
                if n == 0 {
                    break;
                }
                out_desc.write(&buf[..n]).await?;
                total += n;
            }
        }

        Ok(total as isize)
    }

    pub(super) fn do_pipe2(&self, fds: LinuxUserspaceArg<*mut c_int>) -> Result<isize, Errno> {
        let (reader, writer) = pipe::new_pipe();
        let (read_fd, write_fd) = self.current_process.with_lock(|p| {
            let r = p.fd_table().allocate(FileDescriptor::PipeRead(reader))?;
            let w = p.fd_table().allocate(FileDescriptor::PipeWrite(writer))?;
            Ok::<_, Errno>((r, w))
        })?;
        fds.write_slice(&[read_fd, write_fd])?;
        Ok(0)
    }

    pub(super) fn do_fcntl(&self, fd: c_int, cmd: c_int, arg: c_ulong) -> Result<isize, Errno> {
        const F_DUPFD_CMD: u32 = 0;
        const F_GETFD_CMD: u32 = 1;
        const F_SETFD_CMD: u32 = 2;
        const FD_CLOEXEC_FLAG: i32 = 1;
        const F_DUPFD_CLOEXEC_CMD: u32 = 1030;

        match cmd.cast_unsigned() {
            F_DUPFD_CMD => {
                let min_fd = i32::try_from(arg).map_err(|_| Errno::EINVAL)?;
                let new_fd = self
                    .current_process
                    .with_lock(|p| p.fd_table().dup_from(fd, min_fd, FdFlags::default()))?;
                Ok(new_fd as isize)
            }
            F_DUPFD_CLOEXEC_CMD => {
                let min_fd = i32::try_from(arg).map_err(|_| Errno::EINVAL)?;
                let new_fd = self.current_process.with_lock(|p| {
                    p.fd_table()
                        .dup_from(fd, min_fd, FdFlags::from_raw(O_CLOEXEC as i32))
                })?;
                Ok(new_fd as isize)
            }
            F_GETFD_CMD => {
                let flags = self
                    .current_process
                    .with_lock(|p| p.fd_table().get_flags(fd))?;
                let cloexec = if flags.is_cloexec() {
                    FD_CLOEXEC_FLAG
                } else {
                    0
                };
                Ok(cloexec as isize)
            }
            F_SETFD_CMD => {
                let raw = i32::try_from(arg).map_err(|_| Errno::EINVAL)?;
                let current = self
                    .current_process
                    .with_lock(|p| p.fd_table().get_flags(fd))?;
                let new_raw = if (raw & FD_CLOEXEC_FLAG) != 0 {
                    current.as_raw() | O_CLOEXEC as i32
                } else {
                    current.as_raw() & !(O_CLOEXEC as i32)
                };
                self.current_process
                    .with_lock(|p| p.fd_table().set_flags(fd, FdFlags::from_raw(new_raw)))?;
                Ok(0)
            }
            F_GETFL => {
                let flags = self
                    .current_process
                    .with_lock(|p| p.fd_table().get_flags(fd))?;
                Ok(flags.as_raw() as isize)
            }
            F_SETFL => {
                let raw = i32::try_from(arg).map_err(|_| Errno::EINVAL)?;
                let flags = FdFlags::from_raw(raw);
                self.current_process
                    .with_lock(|p| p.fd_table().set_flags(fd, flags))?;
                Ok(0)
            }
            _ => Err(Errno::EINVAL),
        }
    }
}
