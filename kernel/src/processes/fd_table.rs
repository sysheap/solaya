use alloc::collections::BTreeMap;
use common::pid::Tid;
use core::fmt;
use headers::{
    errno::Errno,
    syscall_types::{O_CLOEXEC, O_NONBLOCK},
};

use crate::{
    fs::VfsOpenFile,
    io::{
        pipe::{PipeReader, PipeWriter, ReadPipe},
        tty_device::{ReadTty, TtyDevice},
        uart::QEMU_UART,
    },
    processes::process::ProcessRef,
};

#[cfg(feature = "udp")]
use crate::net::sockets::SharedAssignedSocket;
#[cfg(feature = "tcp")]
use crate::net::tcp_connection::{SharedTcpConnection, SharedTcpListener};

pub type RawFd = i32;

#[derive(Clone, Copy, Debug, Default)]
pub struct FdFlags(i32);

impl FdFlags {
    pub fn is_nonblocking(self) -> bool {
        (self.0 & O_NONBLOCK as i32) != 0
    }

    pub fn as_raw(self) -> i32 {
        self.0
    }

    pub fn from_raw(raw: i32) -> Self {
        Self(raw & ((O_NONBLOCK | O_CLOEXEC) as i32))
    }

    pub fn is_cloexec(self) -> bool {
        (self.0 & O_CLOEXEC as i32) != 0
    }
}

pub enum FileDescriptor {
    Tty(TtyDevice),
    #[cfg(feature = "udp")]
    UnboundUdpSocket,
    #[cfg(feature = "udp")]
    UdpSocket(SharedAssignedSocket),
    #[cfg(feature = "tcp")]
    UnboundTcpSocket,
    #[cfg(feature = "tcp")]
    TcpStream(SharedTcpConnection),
    #[cfg(feature = "tcp")]
    TcpListener(SharedTcpListener),
    PipeRead(PipeReader),
    PipeWrite(PipeWriter),
    VfsFile(VfsOpenFile),
}

impl Clone for FileDescriptor {
    fn clone(&self) -> Self {
        match self {
            Self::Tty(dev) => Self::Tty(dev.clone()),
            Self::PipeRead(r) => Self::PipeRead(r.clone()),
            Self::PipeWrite(w) => Self::PipeWrite(w.clone()),
            #[cfg(feature = "udp")]
            Self::UnboundUdpSocket => Self::UnboundUdpSocket,
            #[cfg(feature = "udp")]
            Self::UdpSocket(s) => Self::UdpSocket(s.clone()),
            #[cfg(feature = "tcp")]
            Self::UnboundTcpSocket => Self::UnboundTcpSocket,
            #[cfg(feature = "tcp")]
            Self::TcpStream(s) => Self::TcpStream(s.clone()),
            #[cfg(feature = "tcp")]
            Self::TcpListener(l) => Self::TcpListener(l.clone()),
            Self::VfsFile(f) => Self::VfsFile(f.clone()),
        }
    }
}

impl fmt::Debug for FileDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileDescriptor::Tty(_) => write!(f, "Tty"),
            #[cfg(feature = "udp")]
            FileDescriptor::UnboundUdpSocket => write!(f, "UnboundUdpSocket"),
            #[cfg(feature = "udp")]
            FileDescriptor::UdpSocket(_) => write!(f, "UdpSocket(..)"),
            #[cfg(feature = "tcp")]
            FileDescriptor::UnboundTcpSocket => write!(f, "UnboundTcpSocket"),
            #[cfg(feature = "tcp")]
            FileDescriptor::TcpStream(_) => write!(f, "TcpStream(..)"),
            #[cfg(feature = "tcp")]
            FileDescriptor::TcpListener(_) => write!(f, "TcpListener(..)"),
            FileDescriptor::PipeRead(_) => write!(f, "PipeRead(..)"),
            FileDescriptor::PipeWrite(_) => write!(f, "PipeWrite(..)"),
            FileDescriptor::VfsFile(_) => write!(f, "VfsFile(..)"),
        }
    }
}

impl FileDescriptor {
    pub async fn read(
        &self,
        count: usize,
        process: ProcessRef,
        tid: Tid,
    ) -> Result<alloc::vec::Vec<u8>, Errno> {
        match self {
            FileDescriptor::Tty(dev) => ReadTty::new(dev.clone(), count, process, tid).await,
            FileDescriptor::PipeRead(buf) => Ok(ReadPipe::new(buf.shared_buffer(), count).await),
            #[cfg(feature = "tcp")]
            FileDescriptor::TcpStream(conn) => {
                use crate::net::tcp_connection::wait_for_recv_data;
                Ok(wait_for_recv_data(conn, count).await)
            }
            FileDescriptor::VfsFile(file) => {
                #[cfg(feature = "virtio-blk")]
                {
                    let block_info = {
                        let inner = file.lock();
                        inner
                            .node()
                            .block_device_index()
                            .map(|idx| (idx, inner.offset()))
                    };
                    if let Some((idx, offset)) = block_info {
                        let mut tmp = alloc::vec![0u8; count];
                        let n = crate::drivers::virtio::block::read(idx, offset, &mut tmp).await?;
                        file.lock().advance_offset(n);
                        tmp.truncate(n);
                        return Ok(tmp);
                    }
                }
                let mut tmp = alloc::vec![0u8; count];
                let n = file.lock().read(&mut tmp)?;
                tmp.truncate(n);
                Ok(tmp)
            }
            _ => Err(Errno::EBADF),
        }
    }

    pub fn try_read(&self, count: usize) -> Result<alloc::vec::Vec<u8>, Errno> {
        match self {
            FileDescriptor::Tty(dev) => {
                let mut d = dev.lock();
                let data = d.get_input(count);
                if data.is_empty() {
                    Err(Errno::EAGAIN)
                } else {
                    Ok(data)
                }
            }
            FileDescriptor::PipeRead(buf) => buf.shared_buffer().lock().try_read(count),
            #[cfg(feature = "tcp")]
            FileDescriptor::TcpStream(conn) => {
                let mut c = conn.lock();
                if c.has_recv_data() {
                    Ok(c.recv_data(count))
                } else {
                    Err(Errno::EAGAIN)
                }
            }
            FileDescriptor::VfsFile(file) => {
                let mut tmp = alloc::vec![0u8; count];
                let n = file.lock().read(&mut tmp)?;
                tmp.truncate(n);
                Ok(tmp)
            }
            _ => Err(Errno::EBADF),
        }
    }

    pub async fn write(&self, data: &[u8]) -> Result<usize, Errno> {
        match self {
            FileDescriptor::Tty(dev) => {
                let processed = dev.lock().process_output(data);
                let mut uart = QEMU_UART.lock();
                for &b in &processed {
                    uart.write_byte(b);
                }
                Ok(data.len())
            }
            FileDescriptor::PipeWrite(buf) => buf.shared_buffer().lock().write(data),
            #[cfg(feature = "tcp")]
            FileDescriptor::TcpStream(conn) => {
                let waker = conn.lock().queue_send_data(data);
                if let Some(w) = waker {
                    w.wake();
                }
                Ok(data.len())
            }
            FileDescriptor::VfsFile(file) => {
                #[cfg(feature = "virtio-blk")]
                {
                    let block_info = {
                        let mut inner = file.lock();
                        inner
                            .node()
                            .block_device_index()
                            .map(|idx| (idx, inner.effective_write_offset()))
                    };
                    if let Some((idx, offset)) = block_info {
                        let n = crate::drivers::virtio::block::write(idx, offset, data).await?;
                        file.lock().advance_offset(n);
                        return Ok(n);
                    }
                }
                file.lock().write(data)
            }
            _ => Err(Errno::EBADF),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FdEntry {
    pub descriptor: FileDescriptor,
    pub flags: FdFlags,
}

#[derive(Clone)]
pub struct FdTable {
    table: BTreeMap<RawFd, FdEntry>,
}

impl FdTable {
    pub fn new() -> Self {
        use crate::io::tty_device::console_tty;
        let mut table = BTreeMap::new();
        let default_flags = FdFlags::default();
        let tty = console_tty().clone();
        for fd in 0..=2 {
            table.insert(
                fd,
                FdEntry {
                    descriptor: FileDescriptor::Tty(tty.clone()),
                    flags: default_flags,
                },
            );
        }
        FdTable { table }
    }

    pub fn get(&self, fd: RawFd) -> Option<&FdEntry> {
        self.table.get(&fd)
    }

    pub fn allocate(&mut self, descriptor: FileDescriptor) -> Result<RawFd, Errno> {
        let fd = (0..)
            .find(|n| !self.table.contains_key(n))
            .ok_or(Errno::EMFILE)?;
        self.table.insert(
            fd,
            FdEntry {
                descriptor,
                flags: FdFlags::default(),
            },
        );
        Ok(fd)
    }

    pub fn replace_descriptor(
        &mut self,
        fd: RawFd,
        descriptor: FileDescriptor,
    ) -> Result<(), Errno> {
        let entry = self.table.get_mut(&fd).ok_or(Errno::EBADF)?;
        entry.descriptor = descriptor;
        Ok(())
    }

    pub fn dup_from(
        &mut self,
        oldfd: RawFd,
        min_fd: RawFd,
        flags: FdFlags,
    ) -> Result<RawFd, Errno> {
        let entry = self.table.get(&oldfd).ok_or(Errno::EBADF)?.clone();
        let newfd = (min_fd..)
            .find(|n| !self.table.contains_key(n))
            .ok_or(Errno::EMFILE)?;
        self.table.insert(
            newfd,
            FdEntry {
                descriptor: entry.descriptor,
                flags,
            },
        );
        Ok(newfd)
    }

    pub fn dup_to(&mut self, oldfd: RawFd, newfd: RawFd, flags: i32) -> Result<RawFd, Errno> {
        if oldfd == newfd {
            return Err(Errno::EINVAL);
        }
        let entry = self.table.get(&oldfd).ok_or(Errno::EBADF)?.clone();
        self.table.remove(&newfd);
        self.table.insert(
            newfd,
            FdEntry {
                descriptor: entry.descriptor,
                flags: FdFlags::from_raw(flags),
            },
        );
        Ok(newfd)
    }

    pub fn close(&mut self, fd: RawFd) -> Result<FdEntry, Errno> {
        self.table.remove(&fd).ok_or(Errno::EBADF)
    }

    pub fn get_descriptor(&self, fd: RawFd) -> Result<FileDescriptor, Errno> {
        self.table
            .get(&fd)
            .map(|e| e.descriptor.clone())
            .ok_or(Errno::EBADF)
    }

    pub fn get_descriptor_and_flags(&self, fd: RawFd) -> Result<(FileDescriptor, FdFlags), Errno> {
        self.table
            .get(&fd)
            .map(|e| (e.descriptor.clone(), e.flags))
            .ok_or(Errno::EBADF)
    }

    pub fn get_vfs_file(&self, fd: RawFd) -> Result<VfsOpenFile, Errno> {
        self.table
            .get(&fd)
            .and_then(|e| match &e.descriptor {
                FileDescriptor::VfsFile(f) => Some(f.clone()),
                _ => None,
            })
            .ok_or(Errno::EBADF)
    }

    pub fn get_flags(&self, fd: RawFd) -> Result<FdFlags, Errno> {
        self.table.get(&fd).map(|e| e.flags).ok_or(Errno::EBADF)
    }

    pub fn set_flags(&mut self, fd: RawFd, flags: FdFlags) -> Result<(), Errno> {
        self.table
            .get_mut(&fd)
            .map(|e| e.flags = flags)
            .ok_or(Errno::EBADF)
    }

    pub fn close_all(&mut self) {
        self.table.clear();
    }

    pub fn close_cloexec_fds(&mut self) {
        self.table.retain(|_, entry| !entry.flags.is_cloexec());
    }
}
