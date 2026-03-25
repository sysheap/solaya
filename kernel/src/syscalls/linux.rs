use crate::{
    cpu::Cpu,
    debug, fs,
    klibc::util::{ByteInterpretable, UsizeExt},
    memory::{PAGE_SIZE, VirtAddr},
    processes::{fd_table::FdFlags, process::ProcessRef, process_table, thread::ThreadRef},
    syscalls::macros::linux_syscalls,
};
use common::{
    pid::Tid,
    syscalls::trap_frame::{Register, TrapFrame},
};
use core::ffi::{c_int, c_uint, c_ulong};
use headers::{
    errno::Errno,
    socket::sockaddr_in,
    syscall_types::{CLONE_THREAD, iovec, pollfd, sigaction, sigset_t, stack_t, timespec},
};

impl ByteInterpretable for sockaddr_in {}
impl ByteInterpretable for headers::fs::stat {}
impl ByteInterpretable for headers::fs::statx {}
impl ByteInterpretable for headers::syscall_types::termios {}
impl ByteInterpretable for headers::sysinfo_types::utsname {}
impl ByteInterpretable for headers::sysinfo_types::sysinfo {}
impl ByteInterpretable for headers::sysinfo_types::rusage {}
impl ByteInterpretable for headers::sysinfo_types::rlimit {}

linux_syscalls! {
    SYSCALL_NR_BIND => bind(fd: c_int, addr: *const u8, addrlen: c_uint);
    SYSCALL_NR_BRK => brk(brk: c_ulong);
    SYSCALL_NR_CHDIR => chdir(pathname: *const u8);
    SYSCALL_NR_CLONE => clone(flags: c_ulong, stack: usize, ptid: Option<*mut c_int>, tls: c_ulong, ctid: Option<*mut c_int>);
    SYSCALL_NR_CLOSE => close(fd: c_int);
    SYSCALL_NR_DUP => dup(oldfd: c_int);
    SYSCALL_NR_DUP3 => dup3(oldfd: c_int, newfd: c_int, flags: c_int);
    SYSCALL_NR_EXECVE => execve(filename: usize, argv: usize, envp: usize);
    SYSCALL_NR_EXIT => exit(status: c_int);
    SYSCALL_NR_EXIT_GROUP => exit_group(status: c_int);
    SYSCALL_NR_FACCESSAT => faccessat(dirfd: c_int, pathname: *const u8, mode: c_int);
    SYSCALL_NR_FADVISE64 => fadvise64(fd: c_int, offset: isize, len: isize, advice: c_int);
    SYSCALL_NR_FCNTL => fcntl(fd: c_int, cmd: c_int, arg: c_ulong);
    SYSCALL_NR_FSTAT => fstat(fd: c_int, statbuf: *mut u8);
    SYSCALL_NR_FUTEX => futex(uaddr: usize, op: c_int, val: c_uint, timeout: usize, uaddr2: usize, val3: c_uint);
    SYSCALL_NR_GETCWD => getcwd(buf: *mut u8, size: usize);
    SYSCALL_NR_GETDENTS64 => getdents64(fd: c_int, dirp: *mut u8, count: usize);
    SYSCALL_NR_GETEGID => getegid();
    SYSCALL_NR_GETGROUPS => getgroups(size: c_int, list: *mut u8);
    SYSCALL_NR_GETRANDOM => getrandom(buf: *mut u8, buflen: usize, flags: c_uint);
    SYSCALL_NR_GETEUID => geteuid();
    SYSCALL_NR_GETGID => getgid();
    SYSCALL_NR_GETPGID => getpgid(pid: c_int);
    SYSCALL_NR_GETPID => getpid();
    SYSCALL_NR_GETRLIMIT => getrlimit(resource: c_uint, rlim: *mut u8);
    SYSCALL_NR_GETRUSAGE => getrusage(who: c_int, usage: *mut u8);
    SYSCALL_NR_GETPPID => getppid();
    SYSCALL_NR_GETRESGID => getresgid(rgid: *mut u8, egid: *mut u8, sgid: *mut u8);
    SYSCALL_NR_GETRESUID => getresuid(ruid: *mut u8, euid: *mut u8, suid: *mut u8);
    SYSCALL_NR_GETSID => getsid(pid: c_int);
    SYSCALL_NR_GETTID => gettid();
    SYSCALL_NR_GETUID => getuid();
    SYSCALL_NR_IOCTL => ioctl(fd: c_int, op: c_uint, arg: usize);
    SYSCALL_NR_LISTXATTR => listxattr(pathname: *const u8, list: *mut u8, size: usize);
    SYSCALL_NR_LLISTXATTR => llistxattr(pathname: *const u8, list: *mut u8, size: usize);
    SYSCALL_NR_LSEEK => lseek(fd: c_int, offset: isize, whence: c_int);
    SYSCALL_NR_MADVISE => madvise(addr: usize, length: usize, advice: c_int);
    SYSCALL_NR_MKDIRAT => mkdirat(dirfd: c_int, pathname: *const u8, mode: c_uint);
    SYSCALL_NR_MMAP => mmap(addr: usize, length: usize, prot: c_uint, flags: c_uint, fd: c_int, offset: isize);
    SYSCALL_NR_MPROTECT => mprotect(addr: usize, len: usize, prot: c_int);
    SYSCALL_NR_MUNMAP => munmap(addr: usize, length: usize);
    SYSCALL_NR_CLOCK_GETTIME => clock_gettime(clockid: c_int, tp: *mut timespec);
    SYSCALL_NR_CLOCK_NANOSLEEP => clock_nanosleep(clockid: c_int, flags: c_int, request: *const timespec, remain: Option<*mut timespec>);
    SYSCALL_NR_NANOSLEEP => nanosleep(duration: *const timespec, rem: Option<*const timespec>);
    SYSCALL_NR_NEWFSTATAT => newfstatat(dirfd: c_int, pathname: *const u8, statbuf: *mut u8, flags: c_int);
    SYSCALL_NR_OPENAT => openat(dirfd: c_int, pathname: *const u8, flags: c_int, mode: c_uint);
    SYSCALL_NR_PIPE2 => pipe2(fds: *mut c_int, flags: c_int);
    SYSCALL_NR_PPOLL => ppoll(fds: *mut pollfd, n: c_uint, to: Option<*const timespec>, mask: Option<*const sigset_t>);
    SYSCALL_NR_PRCTL => prctl(option: c_int, arg2: c_ulong, arg3: c_ulong, arg4: c_ulong, arg5: c_ulong);
    SYSCALL_NR_PREAD64 => pread64(fd: c_int, buf: *mut u8, count: usize, offset: isize);
    SYSCALL_NR_PRLIMIT64 => prlimit64(pid: c_int, resource: c_uint, new_limit: Option<*const u8>, old_limit: Option<*mut u8>);
    SYSCALL_NR_PWRITE64 => pwrite64(fd: c_int, buf: *const u8, count: usize, offset: isize);
    SYSCALL_NR_READ => read(fd: c_int, buf: *mut u8, count: usize);
    SYSCALL_NR_READV => readv(fd: c_int, iov: *const iovec, iovcnt: c_int);
    SYSCALL_NR_READLINKAT => readlinkat(dirfd: c_int, pathname: *const u8, buf: *mut u8, bufsiz: usize);
    SYSCALL_NR_RECVFROM => recvfrom(fd: c_int, buf: *mut u8, len: usize, flags: c_int, src_addr: Option<*mut u8>, addrlen: Option<*mut c_uint>);
    SYSCALL_NR_RT_SIGACTION => rt_sigaction(sig: c_uint, act: Option<*const sigaction>, oact: Option<*mut sigaction>, sigsetsize: usize);
    SYSCALL_NR_RT_SIGPROCMASK => rt_sigprocmask(how: c_uint, set: Option<*const sigset_t>, oldset: Option<*mut sigset_t>, sigsetsize: usize);
    SYSCALL_NR_RT_SIGRETURN => rt_sigreturn();
    SYSCALL_NR_SENDTO => sendto(fd: c_int, buf: *const u8, len: usize, flags: c_int, dest_addr: *const u8, addrlen: c_uint);
    SYSCALL_NR_SETGID => setgid(gid: c_uint);
    SYSCALL_NR_SETGROUPS => setgroups(size: c_int, list: *const u8);
    SYSCALL_NR_SETPGID => setpgid(pid: c_int, pgid: c_int);
    SYSCALL_NR_SETREGID => setregid(rgid: c_uint, egid: c_uint);
    SYSCALL_NR_SETRESGID => setresgid(rgid: c_uint, egid: c_uint, sgid: c_uint);
    SYSCALL_NR_SETRESUID => setresuid(ruid: c_uint, euid: c_uint, suid: c_uint);
    SYSCALL_NR_SETREUID => setreuid(ruid: c_uint, euid: c_uint);
    SYSCALL_NR_SETRLIMIT => setrlimit(resource: c_uint, rlim: *const u8);
    SYSCALL_NR_SETSID => setsid();
    SYSCALL_NR_SETUID => setuid(uid: c_uint);
    SYSCALL_NR_SET_ROBUST_LIST => set_robust_list(head: usize, len: usize);
    SYSCALL_NR_SET_TID_ADDRESS => set_tid_address(tidptr: *mut c_int);
    SYSCALL_NR_SIGALTSTACK => sigaltstack(uss: Option<*const stack_t>, uoss: Option<*mut stack_t>);
    SYSCALL_NR_SETSOCKOPT => setsockopt(fd: c_int, level: c_int, optname: c_int, optval: *const u8, optlen: c_uint);
    SYSCALL_NR_GETSOCKNAME => getsockname(fd: c_int, addr: *mut u8, addrlen: *mut c_uint);
    SYSCALL_NR_GETPEERNAME => getpeername(fd: c_int, addr: *mut u8, addrlen: *mut c_uint);
    SYSCALL_NR_CONNECT => connect(fd: c_int, addr: *const u8, addrlen: c_uint);
    SYSCALL_NR_LISTEN => listen(fd: c_int, backlog: c_int);
    SYSCALL_NR_ACCEPT4 => accept4(fd: c_int, addr: Option<*mut u8>, addrlen: Option<*mut c_uint>, flags: c_int);
    SYSCALL_NR_SHUTDOWN => shutdown(fd: c_int, how: c_int);
    SYSCALL_NR_SOCKET => socket(domain: c_int, typ: c_int, protocol: c_int);
    SYSCALL_NR_STATX => statx(dirfd: c_int, pathname: *const u8, flags: c_int, mask: c_uint, statxbuf: *mut u8);
    SYSCALL_NR_SYSINFO => sysinfo(info: *mut u8);
    SYSCALL_NR_KILL => kill(pid: c_int, sig: c_int);
    SYSCALL_NR_TGKILL => tgkill(tgid: c_int, tid: c_int, sig: c_int);
    SYSCALL_NR_TKILL => tkill(tid: c_int, sig: c_int);
    SYSCALL_NR_UMASK => umask(mask: c_uint);
    SYSCALL_NR_UNAME => uname(buf: *mut u8);
    SYSCALL_NR_UNLINKAT => unlinkat(dirfd: c_int, pathname: *const u8, flags: c_int);
    SYSCALL_NR_UTIMENSAT => utimensat(dirfd: c_int, pathname: *const u8, times: usize, flags: c_int);
    SYSCALL_NR_WAIT4 => wait4(pid: c_int, status: Option<*mut c_int>, options: c_int, rusage: usize);
    SYSCALL_NR_WRITEV => writev(fd: c_int, iov: *const iovec, iovcnt: c_int);
    SYSCALL_NR_SPLICE => splice(fd_in: c_int, off_in: usize, fd_out: c_int, off_out: usize, len: usize, flags: c_uint);
    SYSCALL_NR_WRITE => write(fd: c_int, buf: *const u8, count: usize);
}

pub struct LinuxSyscallHandler {
    pub(super) current_process: ProcessRef,
    pub(super) current_thread: ThreadRef,
    pub(super) current_tid: Tid,
}

impl LinuxSyscalls for LinuxSyscallHandler {
    async fn read(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*mut u8>,
        count: usize,
    ) -> Result<isize, Errno> {
        self.do_read(fd, buf, count).await
    }

    async fn write(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*const u8>,
        count: usize,
    ) -> Result<isize, Errno> {
        self.do_write(fd, buf, count).await
    }

    async fn readv(
        &mut self,
        fd: c_int,
        iov: LinuxUserspaceArg<*const iovec>,
        iovcnt: c_int,
    ) -> Result<isize, Errno> {
        self.do_readv(fd, iov, iovcnt).await
    }

    async fn writev(
        &mut self,
        fd: c_int,
        iov: LinuxUserspaceArg<*const iovec>,
        iovcnt: c_int,
    ) -> Result<isize, Errno> {
        self.do_writev(fd, iov, iovcnt).await
    }

    async fn pread64(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*mut u8>,
        count: usize,
        offset: isize,
    ) -> Result<isize, Errno> {
        self.do_pread64(fd, buf, count, offset).await
    }

    async fn pwrite64(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*const u8>,
        count: usize,
        offset: isize,
    ) -> Result<isize, Errno> {
        self.do_pwrite64(fd, buf, count, offset).await
    }

    async fn close(&mut self, fd: c_int) -> Result<isize, Errno> {
        self.current_process.with_lock(|p| p.fd_table().close(fd))?;
        Ok(0)
    }

    async fn dup(&mut self, oldfd: c_int) -> Result<isize, Errno> {
        let new_fd = self
            .current_process
            .with_lock(|p| p.fd_table().dup_from(oldfd, 0, FdFlags::default()))?;
        Ok(new_fd as isize)
    }

    async fn dup3(&mut self, oldfd: c_int, newfd: c_int, flags: c_int) -> Result<isize, Errno> {
        let result = self
            .current_process
            .with_lock(|p| p.fd_table().dup_to(oldfd, newfd, flags))?;
        Ok(result as isize)
    }

    async fn pipe2(
        &mut self,
        fds: LinuxUserspaceArg<*mut c_int>,
        _flags: c_int,
    ) -> Result<isize, Errno> {
        self.do_pipe2(fds)
    }

    async fn fcntl(&mut self, fd: c_int, cmd: c_int, arg: c_ulong) -> Result<isize, Errno> {
        self.do_fcntl(fd, cmd, arg)
    }

    async fn lseek(&mut self, fd: c_int, offset: isize, whence: c_int) -> Result<isize, Errno> {
        let file = self
            .current_process
            .with_lock(|p| p.fd_table().get_vfs_file(fd))?;
        Ok(file.lock().seek(offset, whence)? as isize)
    }

    async fn ioctl(&mut self, fd: c_int, op: c_uint, arg: usize) -> Result<isize, Errno> {
        self.do_ioctl(fd, op, arg)
    }

    async fn openat(
        &mut self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        flags: c_int,
        mode: c_uint,
    ) -> Result<isize, Errno> {
        self.do_openat(dirfd, pathname, flags, mode)
    }

    async fn fstat(
        &mut self,
        fd: c_int,
        statbuf: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        self.do_fstat(fd, statbuf)
    }

    async fn newfstatat(
        &mut self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        statbuf: LinuxUserspaceArg<*mut u8>,
        flags: c_int,
    ) -> Result<isize, Errno> {
        self.do_newfstatat(dirfd, pathname, statbuf, flags)
    }

    async fn statx(
        &mut self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        flags: c_int,
        mask: c_uint,
        statxbuf: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        self.do_statx(dirfd, pathname, flags, mask, statxbuf)
    }

    async fn getdents64(
        &mut self,
        fd: c_int,
        dirp: LinuxUserspaceArg<*mut u8>,
        count: usize,
    ) -> Result<isize, Errno> {
        self.do_getdents64(fd, dirp, count)
    }

    async fn faccessat(
        &mut self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        mode: c_int,
    ) -> Result<isize, Errno> {
        self.do_faccessat(dirfd, pathname, mode)
    }

    async fn mkdirat(
        &mut self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        mode: c_uint,
    ) -> Result<isize, Errno> {
        self.do_mkdirat(dirfd, pathname, mode)
    }

    async fn unlinkat(
        &mut self,
        dirfd: c_int,
        pathname: LinuxUserspaceArg<*const u8>,
        flags: c_int,
    ) -> Result<isize, Errno> {
        self.do_unlinkat(dirfd, pathname, flags)
    }

    async fn chdir(&mut self, pathname: LinuxUserspaceArg<*const u8>) -> Result<isize, Errno> {
        let path = self.read_path(&pathname)?;
        let node = fs::resolve_path(&path)?;
        if node.node_type() != fs::vfs::NodeType::Directory {
            return Err(Errno::ENOTDIR);
        }
        self.current_process.with_lock(|mut p| p.set_cwd(path));
        Ok(0)
    }

    async fn getcwd(
        &mut self,
        buf: LinuxUserspaceArg<*mut u8>,
        size: usize,
    ) -> Result<isize, Errno> {
        self.do_getcwd(buf, size)
    }

    async fn umask(&mut self, mask: c_uint) -> Result<isize, Errno> {
        let old = self.current_process.with_lock(|mut p| {
            let old = p.umask();
            p.set_umask(mask & 0o777);
            old
        });
        Ok(old as isize)
    }

    async fn brk(&mut self, brk: c_ulong) -> Result<isize, Errno> {
        self.current_process
            .with_lock(|mut p| Ok(p.brk(VirtAddr::new(brk.as_usize())).as_usize() as isize))
    }

    async fn mmap(
        &mut self,
        addr: usize,
        length: usize,
        prot: c_uint,
        flags: c_uint,
        fd: c_int,
        offset: isize,
    ) -> Result<isize, Errno> {
        self.do_mmap(addr, length, prot, flags, fd, offset)
    }

    async fn mprotect(&mut self, addr: usize, len: usize, prot: c_int) -> Result<isize, Errno> {
        self.do_mprotect(addr, len, prot)
    }

    async fn munmap(&mut self, addr: usize, length: usize) -> Result<isize, Errno> {
        if !addr.is_multiple_of(PAGE_SIZE) || length == 0 {
            return Err(Errno::EINVAL);
        }
        self.current_process
            .with_lock(|mut p| p.munmap_pages(VirtAddr::new(addr), length))?;
        Ok(0)
    }

    async fn exit(&mut self, status: c_int) -> Result<isize, Errno> {
        let exit_status = crate::processes::signal::ExitStatus::Exited(status.to_le_bytes()[0]);
        Cpu::with_scheduler(|mut s| {
            s.kill_current_thread(exit_status);
        });
        debug!("Exit thread with status: {status}\n");
        Ok(0)
    }

    async fn exit_group(&mut self, status: c_int) -> Result<isize, Errno> {
        let exit_status = crate::processes::signal::ExitStatus::Exited(status.to_le_bytes()[0]);
        Cpu::with_scheduler(|mut s| {
            s.kill_current_process(exit_status);
        });
        debug!("Exit process with status: {status}\n");
        Ok(0)
    }

    async fn wait4(
        &mut self,
        pid: c_int,
        status: LinuxUserspaceArg<Option<*mut c_int>>,
        options: c_int,
        _rusage: usize,
    ) -> Result<isize, Errno> {
        self.do_wait4(pid, status, options).await
    }

    async fn clone(
        &mut self,
        flags: c_ulong,
        stack: usize,
        ptid: LinuxUserspaceArg<Option<*mut c_int>>,
        tls: c_ulong,
        ctid: LinuxUserspaceArg<Option<*mut c_int>>,
    ) -> Result<isize, Errno> {
        if (flags & c_ulong::from(CLONE_THREAD)) != 0 {
            self.clone_thread(flags, stack, ptid, tls, ctid)
        } else {
            self.clone_fork(stack).await
        }
    }

    async fn execve(&mut self, filename: usize, argv: usize, envp: usize) -> Result<isize, Errno> {
        self.do_execve(filename, argv, envp)
    }

    async fn rt_sigaction(
        &mut self,
        sig: c_uint,
        act: LinuxUserspaceArg<Option<*const sigaction>>,
        oact: LinuxUserspaceArg<Option<*mut sigaction>>,
        sigsetsize: usize,
    ) -> Result<isize, Errno> {
        self.do_rt_sigaction(sig, act, oact, sigsetsize)
    }

    async fn rt_sigprocmask(
        &mut self,
        how: c_uint,
        set: LinuxUserspaceArg<Option<*const sigset_t>>,
        oldset: LinuxUserspaceArg<Option<*mut sigset_t>>,
        sigsetsize: usize,
    ) -> Result<isize, Errno> {
        self.do_rt_sigprocmask(how, set, oldset, sigsetsize)
    }

    async fn rt_sigreturn(&mut self) -> Result<isize, Errno> {
        self.current_thread.with_lock(|mut t| {
            crate::processes::signal::restore_signal_frame(&mut t)?;
            t.set_registers_replaced(true);
            Ok::<_, Errno>(())
        })?;
        Ok(0)
    }

    async fn sigaltstack(
        &mut self,
        uss: LinuxUserspaceArg<Option<*const stack_t>>,
        uoss: LinuxUserspaceArg<Option<*mut stack_t>>,
    ) -> Result<isize, Errno> {
        self.do_sigaltstack(uss, uoss)
    }

    async fn kill(&mut self, pid: c_int, sig: c_int) -> Result<isize, Errno> {
        self.do_kill(pid, sig)
    }

    async fn tgkill(&mut self, _tgid: c_int, tid: c_int, sig: c_int) -> Result<isize, Errno> {
        let target_tid = Tid::try_from_i32(tid).ok_or(Errno::ESRCH)?;
        if let Some(sig) = crate::processes::signal::validate_signal(sig)? {
            process_table::THE.lock().send_signal(target_tid, sig);
        }
        Ok(0)
    }

    async fn tkill(&mut self, tid: c_int, sig: c_int) -> Result<isize, Errno> {
        self.tgkill(0, tid, sig).await
    }

    async fn socket(&mut self, domain: c_int, typ: c_int, protocol: c_int) -> Result<isize, Errno> {
        self.do_socket(domain, typ, protocol)
    }

    async fn bind(
        &mut self,
        fd: c_int,
        addr: LinuxUserspaceArg<*const u8>,
        addrlen: c_uint,
    ) -> Result<isize, Errno> {
        self.do_bind(fd, addr, addrlen)
    }

    async fn sendto(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*const u8>,
        len: usize,
        flags: c_int,
        dest_addr: LinuxUserspaceArg<*const u8>,
        addrlen: c_uint,
    ) -> Result<isize, Errno> {
        self.do_sendto(fd, buf, len, flags, dest_addr, addrlen)
    }

    async fn recvfrom(
        &mut self,
        fd: c_int,
        buf: LinuxUserspaceArg<*mut u8>,
        len: usize,
        flags: c_int,
        src_addr: LinuxUserspaceArg<Option<*mut u8>>,
        addrlen: LinuxUserspaceArg<Option<*mut c_uint>>,
    ) -> Result<isize, Errno> {
        self.do_recvfrom(fd, buf, len, flags, src_addr, addrlen)
            .await
    }

    async fn connect(
        &mut self,
        fd: c_int,
        addr: LinuxUserspaceArg<*const u8>,
        addrlen: c_uint,
    ) -> Result<isize, Errno> {
        self.do_connect(fd, addr, addrlen).await
    }

    async fn listen(&mut self, fd: c_int, _backlog: c_int) -> Result<isize, Errno> {
        self.do_listen(fd)
    }

    async fn accept4(
        &mut self,
        fd: c_int,
        addr: LinuxUserspaceArg<Option<*mut u8>>,
        addrlen: LinuxUserspaceArg<Option<*mut c_uint>>,
        _flags: c_int,
    ) -> Result<isize, Errno> {
        self.do_accept(fd, addr, addrlen).await
    }

    async fn setsockopt(
        &mut self,
        _fd: c_int,
        _level: c_int,
        _optname: c_int,
        _optval: LinuxUserspaceArg<*const u8>,
        _optlen: c_uint,
    ) -> Result<isize, Errno> {
        Ok(0)
    }

    async fn getsockname(
        &mut self,
        fd: c_int,
        addr: LinuxUserspaceArg<*mut u8>,
        addrlen: LinuxUserspaceArg<*mut c_uint>,
    ) -> Result<isize, Errno> {
        self.do_getsockname(fd, addr, addrlen)
    }

    async fn getpeername(
        &mut self,
        fd: c_int,
        addr: LinuxUserspaceArg<*mut u8>,
        addrlen: LinuxUserspaceArg<*mut c_uint>,
    ) -> Result<isize, Errno> {
        self.do_getpeername(fd, addr, addrlen)
    }

    async fn shutdown(&mut self, fd: c_int, _how: c_int) -> Result<isize, Errno> {
        self.do_shutdown(fd)
    }

    async fn nanosleep(
        &mut self,
        duration: LinuxUserspaceArg<*const timespec>,
        _rem: LinuxUserspaceArg<Option<*const timespec>>,
    ) -> Result<isize, Errno> {
        self.do_nanosleep(duration).await
    }

    async fn clock_nanosleep(
        &mut self,
        clockid: c_int,
        flags: c_int,
        request: LinuxUserspaceArg<*const timespec>,
        _remain: LinuxUserspaceArg<Option<*mut timespec>>,
    ) -> Result<isize, Errno> {
        self.do_clock_nanosleep(clockid, flags, request).await
    }

    async fn clock_gettime(
        &mut self,
        _clockid: c_int,
        tp: LinuxUserspaceArg<*mut timespec>,
    ) -> Result<isize, Errno> {
        tp.write_slice(&[crate::processes::timer::current_time()])?;
        Ok(0)
    }

    async fn ppoll(
        &mut self,
        fds: LinuxUserspaceArg<*mut pollfd>,
        n: c_uint,
        to: LinuxUserspaceArg<Option<*const timespec>>,
        mask: LinuxUserspaceArg<Option<*const sigset_t>>,
    ) -> Result<isize, Errno> {
        self.do_ppoll(fds, n, to, mask)
    }

    async fn getpid(&mut self) -> Result<isize, Errno> {
        Ok(self.current_process.with_lock(|p| p.main_tid()).as_isize())
    }

    async fn getppid(&mut self) -> Result<isize, Errno> {
        Ok(self.current_thread.lock().parent_tid().as_isize())
    }

    async fn gettid(&mut self) -> Result<isize, Errno> {
        Ok(self.current_tid.as_isize())
    }

    async fn getpgid(&mut self, pid: c_int) -> Result<isize, Errno> {
        self.do_getpgid(pid)
    }

    async fn getsid(&mut self, pid: c_int) -> Result<isize, Errno> {
        self.do_getsid(pid)
    }

    async fn setpgid(&mut self, pid: c_int, pgid: c_int) -> Result<isize, Errno> {
        self.do_setpgid(pid, pgid)
    }

    async fn setrlimit(
        &mut self,
        _resource: c_uint,
        _rlim: LinuxUserspaceArg<*const u8>,
    ) -> Result<isize, Errno> {
        Ok(0)
    }

    async fn setsid(&mut self) -> Result<isize, Errno> {
        self.current_process.with_lock(|mut p| {
            let main_tid = p.main_tid();
            if p.pgid() == main_tid {
                return Err(Errno::EPERM);
            }
            p.set_pgid(main_tid);
            p.set_sid(main_tid);
            Ok(main_tid.as_isize())
        })
    }

    async fn set_tid_address(
        &mut self,
        tidptr: LinuxUserspaceArg<*mut c_int>,
    ) -> Result<isize, Errno> {
        self.current_thread.with_lock(|mut t| {
            t.set_clear_child_tid((&tidptr).into());
        });
        Ok(self.current_tid.as_isize())
    }

    async fn futex(
        &mut self,
        uaddr: usize,
        op: c_int,
        val: c_uint,
        _timeout: usize,
        _uaddr2: usize,
        _val3: c_uint,
    ) -> Result<isize, Errno> {
        self.do_futex(uaddr, op, val).await
    }

    async fn uname(&mut self, buf: LinuxUserspaceArg<*mut u8>) -> Result<isize, Errno> {
        self.do_uname(buf)
    }

    async fn sysinfo(&mut self, info: LinuxUserspaceArg<*mut u8>) -> Result<isize, Errno> {
        self.do_sysinfo(info)
    }

    async fn getrlimit(
        &mut self,
        resource: c_uint,
        rlim: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        self.do_getrlimit(resource, rlim)
    }

    async fn getrusage(
        &mut self,
        who: c_int,
        usage: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        self.do_getrusage(who, usage)
    }

    async fn getrandom(
        &mut self,
        buf: LinuxUserspaceArg<*mut u8>,
        buflen: usize,
        _flags: c_uint,
    ) -> Result<isize, Errno> {
        self.do_getrandom(buf, buflen)
    }

    fn get_process(&self) -> ProcessRef {
        self.current_process.clone()
    }

    async fn set_robust_list(&mut self, _head: usize, _len: usize) -> Result<isize, Errno> {
        Ok(0)
    }

    async fn madvise(
        &mut self,
        _addr: usize,
        _length: usize,
        _advice: c_int,
    ) -> Result<isize, Errno> {
        Ok(0)
    }

    async fn fadvise64(
        &mut self,
        _fd: c_int,
        _offset: isize,
        _len: isize,
        _advice: c_int,
    ) -> Result<isize, Errno> {
        Ok(0)
    }

    async fn listxattr(
        &mut self,
        _pathname: LinuxUserspaceArg<*const u8>,
        _list: LinuxUserspaceArg<*mut u8>,
        _size: usize,
    ) -> Result<isize, Errno> {
        Ok(0)
    }

    async fn llistxattr(
        &mut self,
        _pathname: LinuxUserspaceArg<*const u8>,
        _list: LinuxUserspaceArg<*mut u8>,
        _size: usize,
    ) -> Result<isize, Errno> {
        Ok(0)
    }

    async fn utimensat(
        &mut self,
        _dirfd: c_int,
        _pathname: LinuxUserspaceArg<*const u8>,
        _times: usize,
        _flags: c_int,
    ) -> Result<isize, Errno> {
        Ok(0)
    }

    async fn prctl(
        &mut self,
        option: c_int,
        arg2: c_ulong,
        arg3: c_ulong,
        arg4: c_ulong,
        arg5: c_ulong,
    ) -> Result<isize, Errno> {
        self.do_prctl(option, arg2, arg3, arg4, arg5)
    }

    async fn prlimit64(
        &mut self,
        pid: c_int,
        resource: c_uint,
        _new_limit: LinuxUserspaceArg<Option<*const u8>>,
        old_limit: LinuxUserspaceArg<Option<*mut u8>>,
    ) -> Result<isize, Errno> {
        self.do_prlimit64(pid, resource, old_limit)
    }

    async fn readlinkat(
        &mut self,
        _dirfd: c_int,
        _pathname: LinuxUserspaceArg<*const u8>,
        _buf: LinuxUserspaceArg<*mut u8>,
        _bufsiz: usize,
    ) -> Result<isize, Errno> {
        Err(Errno::EINVAL)
    }

    async fn getuid(&mut self) -> Result<isize, Errno> {
        Ok(self.current_process.with_lock(|p| p.credentials().uid) as isize)
    }

    async fn geteuid(&mut self) -> Result<isize, Errno> {
        Ok(self.current_process.with_lock(|p| p.credentials().euid) as isize)
    }

    async fn getgid(&mut self) -> Result<isize, Errno> {
        Ok(self.current_process.with_lock(|p| p.credentials().gid) as isize)
    }

    async fn getegid(&mut self) -> Result<isize, Errno> {
        Ok(self.current_process.with_lock(|p| p.credentials().egid) as isize)
    }

    async fn setuid(&mut self, uid: c_uint) -> Result<isize, Errno> {
        self.do_setuid(uid)
    }

    async fn setgid(&mut self, gid: c_uint) -> Result<isize, Errno> {
        self.do_setgid(gid)
    }

    async fn setreuid(&mut self, ruid: c_uint, euid: c_uint) -> Result<isize, Errno> {
        self.do_setreuid(ruid, euid)
    }

    async fn setregid(&mut self, rgid: c_uint, egid: c_uint) -> Result<isize, Errno> {
        self.do_setregid(rgid, egid)
    }

    async fn setresuid(
        &mut self,
        ruid: c_uint,
        euid: c_uint,
        suid: c_uint,
    ) -> Result<isize, Errno> {
        self.do_setresuid(ruid, euid, suid)
    }

    async fn setresgid(
        &mut self,
        rgid: c_uint,
        egid: c_uint,
        sgid: c_uint,
    ) -> Result<isize, Errno> {
        self.do_setresgid(rgid, egid, sgid)
    }

    async fn getresuid(
        &mut self,
        ruid: LinuxUserspaceArg<*mut u8>,
        euid: LinuxUserspaceArg<*mut u8>,
        suid: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        self.do_getresuid(ruid, euid, suid)
    }

    async fn getresgid(
        &mut self,
        rgid: LinuxUserspaceArg<*mut u8>,
        egid: LinuxUserspaceArg<*mut u8>,
        sgid: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        self.do_getresgid(rgid, egid, sgid)
    }

    async fn getgroups(
        &mut self,
        size: c_int,
        list: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        self.do_getgroups(size, list)
    }

    async fn setgroups(
        &mut self,
        size: c_int,
        list: LinuxUserspaceArg<*const u8>,
    ) -> Result<isize, Errno> {
        self.do_setgroups(size, list)
    }

    async fn splice(
        &mut self,
        _fd_in: c_int,
        _off_in: usize,
        _fd_out: c_int,
        _off_out: usize,
        _len: usize,
        _flags: c_uint,
    ) -> Result<isize, Errno> {
        Err(Errno::ENOSYS)
    }
}

impl LinuxSyscallHandler {
    pub fn new() -> Self {
        let current_thread = Cpu::with_scheduler(|s| s.get_current_thread().clone());
        let (current_tid, current_process) =
            current_thread.with_lock(|t| (t.get_tid(), t.process()));
        Self {
            current_process,
            current_thread,
            current_tid,
        }
    }
}
