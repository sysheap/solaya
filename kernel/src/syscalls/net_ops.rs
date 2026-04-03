use core::ffi::{c_int, c_uint};
use headers::{
    errno::Errno,
    socket::{AF_INET, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_STREAM, sockaddr_in},
};

use crate::{
    klibc::util::ByteInterpretable,
    net::{
        self, arp,
        sockets::{Port, SharedAssignedSocket},
        tcp_connection,
        udp::UdpHeader,
    },
    processes::fd_table::FileDescriptor,
    syscalls::linux_validator::LinuxUserspaceArg,
};

use super::linux::{LinuxSyscallHandler, LinuxSyscalls};

impl LinuxSyscallHandler {
    pub(super) fn do_socket(
        &self,
        domain: c_int,
        typ: c_int,
        _protocol: c_int,
    ) -> Result<isize, Errno> {
        assert!(
            domain == AF_INET,
            "socket: only AF_INET supported (got domain={domain})"
        );
        let masked_type = typ & !SOCK_CLOEXEC;
        let descriptor = match masked_type {
            SOCK_DGRAM => FileDescriptor::UnboundUdpSocket,
            SOCK_STREAM => FileDescriptor::UnboundTcpSocket,
            _ => panic!("socket: unsupported type {typ:#x}"),
        };
        let fd = self
            .current_process
            .with_lock(|p| p.fd_table().allocate(descriptor))?;
        Ok(fd as isize)
    }

    pub(super) fn do_bind(
        &self,
        fd: c_int,
        addr: LinuxUserspaceArg<*const u8>,
        addrlen: c_uint,
    ) -> Result<isize, Errno> {
        assert!(
            addrlen as usize >= core::mem::size_of::<sockaddr_in>(),
            "bind: addrlen too small ({addrlen})"
        );

        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get(fd).map(|e| e.descriptor.clone()))
            .ok_or(Errno::EBADF)?;

        if !net::has_network_device() {
            return Err(Errno::ENETDOWN);
        }

        let sin_arg =
            LinuxUserspaceArg::<*const sockaddr_in>::new(addr.raw_arg(), self.get_process());
        let sin = sin_arg.validate_ptr()?;
        let port = u16::from_be(sin.sin_port);

        match descriptor {
            FileDescriptor::UnboundUdpSocket => {
                let socket = net::open_sockets()
                    .lock()
                    .try_get_socket(Port::new(port))
                    .ok_or(Errno::EADDRINUSE)?;
                self.current_process.with_lock(|p| {
                    p.fd_table()
                        .replace_descriptor(fd, FileDescriptor::UdpSocket(socket))
                })?;
            }
            FileDescriptor::UnboundTcpSocket => {
                let listener = tcp_connection::create_listener(port);
                self.current_process.with_lock(|p| {
                    p.fd_table()
                        .replace_descriptor(fd, FileDescriptor::TcpListener(listener))
                })?;
            }
            _ => return Err(Errno::EINVAL),
        }

        Ok(0)
    }

    pub(super) async fn do_sendto(
        &self,
        fd: c_int,
        buf: LinuxUserspaceArg<*const u8>,
        len: usize,
        _flags: c_int,
        dest_addr: LinuxUserspaceArg<*const u8>,
        addrlen: c_uint,
    ) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get_descriptor(fd))
            .map_err(|_| Errno::EBADF)?;

        match descriptor {
            FileDescriptor::TcpStream(conn) => {
                let data = buf.validate_slice(len)?;
                let written = tcp_connection::tcp_write(&conn, &data).await;
                Ok(written as isize)
            }
            FileDescriptor::UdpSocket(socket) => {
                assert!(
                    addrlen as usize >= core::mem::size_of::<sockaddr_in>(),
                    "sendto: addrlen too small ({addrlen})"
                );

                let data = buf.validate_slice(len)?;
                let sin_arg = LinuxUserspaceArg::<*const sockaddr_in>::new(
                    dest_addr.raw_arg(),
                    self.get_process(),
                );
                let sin = sin_arg.validate_ptr()?;

                let dest_ip = core::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                let dest_port = u16::from_be(sin.sin_port);

                if !net::has_network_device() {
                    return Err(Errno::ENETDOWN);
                }

                let destination_mac = if dest_ip == core::net::Ipv4Addr::BROADCAST {
                    net::mac::MacAddress::new([0xff, 0xff, 0xff, 0xff, 0xff, 0xff])
                } else {
                    arp::cache_lookup(&dest_ip)
                        .expect("sendto: destination MAC must be in ARP cache")
                };

                let source_port = socket.lock().get_port().as_u16();
                let packet = UdpHeader::create_udp_packet(
                    dest_ip,
                    dest_port,
                    destination_mac,
                    source_port,
                    &data,
                );
                net::send_packet(packet);

                Ok(len as isize)
            }
            _ => Err(Errno::EBADF),
        }
    }

    pub(super) async fn do_recvfrom(
        &self,
        fd: c_int,
        buf: LinuxUserspaceArg<*mut u8>,
        len: usize,
        _flags: c_int,
        src_addr: LinuxUserspaceArg<Option<*mut u8>>,
        addrlen: LinuxUserspaceArg<Option<*mut c_uint>>,
    ) -> Result<isize, Errno> {
        enum SocketType {
            Udp(SharedAssignedSocket, bool),
            Tcp(tcp_connection::SharedTcpConnection),
        }

        let socket_type = self
            .current_process
            .with_lock(|p| {
                p.fd_table().get(fd).and_then(|e| match &e.descriptor {
                    FileDescriptor::UdpSocket(s) => {
                        Some(SocketType::Udp(s.clone(), e.flags.is_nonblocking()))
                    }
                    FileDescriptor::TcpStream(c) => Some(SocketType::Tcp(c.clone())),
                    _ => None,
                })
            })
            .ok_or(Errno::EBADF)?;

        match socket_type {
            SocketType::Tcp(conn) => {
                let data = tcp_connection::wait_for_recv_data(&conn, len).await;
                if data.is_empty() {
                    return Ok(0);
                }
                buf.write_slice(&data)?;
                Ok(data.len() as isize)
            }
            SocketType::Udp(socket, is_nonblocking) => {
                let mut tmp_buf = alloc::vec![0u8; len];

                let result = loop {
                    let seen = net::sockets::socket_data_counter();
                    if let Some(result) = socket.lock().get_datagram(&mut tmp_buf) {
                        break result;
                    }
                    if is_nonblocking {
                        return Err(Errno::EAGAIN);
                    }
                    net::sockets::SocketDataWait::new(seen).await;
                };

                let (bytes_read, from_ip, from_port) = result;
                buf.write_slice(&tmp_buf[..bytes_read])?;

                if src_addr.arg_nonzero() {
                    let sin = sockaddr_in {
                        sin_family: u16::try_from(AF_INET).expect("AF_INET fits in u16"),
                        sin_port: from_port.as_u16().to_be(),
                        sin_addr: headers::socket::in_addr {
                            s_addr: u32::from(from_ip).to_be(),
                        },
                        sin_zero: [0; 8],
                    };
                    let src_writer =
                        LinuxUserspaceArg::<*mut u8>::new(src_addr.raw_arg(), self.get_process());
                    src_writer.write_slice(sin.as_slice())?;
                    let addrlen_val = c_uint::try_from(core::mem::size_of::<sockaddr_in>())
                        .expect("sockaddr_in size fits in c_uint");
                    addrlen.write_if_not_none(addrlen_val)?;
                }

                Ok(bytes_read as isize)
            }
        }
    }

    pub(super) async fn do_connect(
        &self,
        fd: c_int,
        addr: LinuxUserspaceArg<*const u8>,
        addrlen: c_uint,
    ) -> Result<isize, Errno> {
        assert!(
            addrlen as usize >= core::mem::size_of::<sockaddr_in>(),
            "connect: addrlen too small ({addrlen})"
        );

        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get(fd).map(|e| e.descriptor.clone()))
            .ok_or(Errno::EBADF)?;

        assert!(
            matches!(descriptor, FileDescriptor::UnboundTcpSocket),
            "connect: fd {fd} is not an unbound TCP socket"
        );

        if !net::has_network_device() {
            return Err(Errno::ENETDOWN);
        }

        let sin_arg =
            LinuxUserspaceArg::<*const sockaddr_in>::new(addr.raw_arg(), self.get_process());
        let sin = sin_arg.validate_ptr()?;
        let dest_ip = core::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
        let dest_port = u16::from_be(sin.sin_port);
        let local_port = tcp_connection::allocate_ephemeral_port();

        let conn = tcp_connection::initiate_connect(local_port, dest_ip, dest_port)
            .await
            .ok_or(Errno::ECONNREFUSED)?;

        self.current_process.with_lock(|p| {
            p.fd_table()
                .replace_descriptor(fd, FileDescriptor::TcpStream(conn))
        })?;

        Ok(0)
    }

    pub(super) fn do_listen(&self, fd: c_int) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get(fd).map(|e| e.descriptor.clone()))
            .ok_or(Errno::EBADF)?;
        let listener = match descriptor {
            FileDescriptor::TcpListener(l) => l,
            _ => return Err(Errno::ENOTSOCK),
        };

        tcp_connection::register_listener(listener);
        Ok(0)
    }

    pub(super) async fn do_accept(
        &self,
        fd: c_int,
        addr: LinuxUserspaceArg<Option<*mut u8>>,
        addrlen: LinuxUserspaceArg<Option<*mut c_uint>>,
    ) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get(fd).map(|e| e.descriptor.clone()))
            .ok_or(Errno::EBADF)?;
        let listener = match descriptor {
            FileDescriptor::TcpListener(l) => l,
            _ => return Err(Errno::ENOTSOCK),
        };

        let conn = tcp_connection::wait_for_accept(&listener).await;

        if addr.arg_nonzero() {
            let c = conn.lock();
            let sin = sockaddr_in {
                sin_family: u16::try_from(AF_INET).expect("AF_INET fits in u16"),
                sin_port: c.remote_port().to_be(),
                sin_addr: headers::socket::in_addr {
                    s_addr: u32::from(c.remote_ip()).to_be(),
                },
                sin_zero: [0; 8],
            };
            drop(c);
            let addr_writer = LinuxUserspaceArg::<*mut u8>::new(addr.raw_arg(), self.get_process());
            addr_writer.write_slice(sin.as_slice())?;
            let addrlen_val = c_uint::try_from(core::mem::size_of::<sockaddr_in>())
                .expect("sockaddr_in size fits in c_uint");
            addrlen.write_if_not_none(addrlen_val)?;
        }

        let new_fd = self
            .current_process
            .with_lock(|p| p.fd_table().allocate(FileDescriptor::TcpStream(conn)))?;

        Ok(new_fd as isize)
    }

    pub(super) fn do_getsockname(
        &self,
        fd: c_int,
        addr: LinuxUserspaceArg<*mut u8>,
        addrlen: LinuxUserspaceArg<*mut c_uint>,
    ) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get(fd).map(|e| e.descriptor.clone()))
            .ok_or(Errno::EBADF)?;

        let local_port = match &descriptor {
            FileDescriptor::TcpListener(l) => l.lock().port(),
            FileDescriptor::TcpStream(c) => c.lock().local_port(),
            FileDescriptor::UdpSocket(s) => s.lock().get_port().as_u16(),
            _ => return Err(Errno::ENOTSOCK),
        };

        let our_ip = net::ip_addr();
        let sin = sockaddr_in {
            sin_family: u16::try_from(AF_INET).expect("AF_INET fits in u16"),
            sin_port: local_port.to_be(),
            sin_addr: headers::socket::in_addr {
                s_addr: u32::from(our_ip).to_be(),
            },
            sin_zero: [0; 8],
        };

        addr.write_slice(sin.as_slice())?;
        let addrlen_val = c_uint::try_from(core::mem::size_of::<sockaddr_in>())
            .expect("sockaddr_in size fits in c_uint");
        addrlen.write_slice(&[addrlen_val])?;

        Ok(0)
    }

    pub(super) fn do_getpeername(
        &self,
        fd: c_int,
        addr: LinuxUserspaceArg<*mut u8>,
        addrlen: LinuxUserspaceArg<*mut c_uint>,
    ) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get(fd).map(|e| e.descriptor.clone()))
            .ok_or(Errno::EBADF)?;
        let conn = match descriptor {
            FileDescriptor::TcpStream(c) => c,
            _ => return Err(Errno::ENOTSOCK),
        };

        let c = conn.lock();
        let sin = sockaddr_in {
            sin_family: u16::try_from(AF_INET).expect("AF_INET fits in u16"),
            sin_port: c.remote_port().to_be(),
            sin_addr: headers::socket::in_addr {
                s_addr: u32::from(c.remote_ip()).to_be(),
            },
            sin_zero: [0; 8],
        };
        drop(c);

        addr.write_slice(sin.as_slice())?;
        let addrlen_val = c_uint::try_from(core::mem::size_of::<sockaddr_in>())
            .expect("sockaddr_in size fits in c_uint");
        addrlen.write_slice(&[addrlen_val])?;

        Ok(0)
    }

    pub(super) fn do_shutdown(&self, fd: c_int) -> Result<isize, Errno> {
        let descriptor = self
            .current_process
            .with_lock(|p| p.fd_table().get(fd).map(|e| e.descriptor.clone()))
            .ok_or(Errno::EBADF)?;
        let conn = match descriptor {
            FileDescriptor::TcpStream(c) => c,
            _ => return Err(Errno::ENOTSOCK),
        };

        if let Some(w) = conn.lock().request_close() {
            w.wake();
        }
        Ok(0)
    }
}
