# Syscall Handling

## Overview

Solaya uses Linux-compatible syscalls exclusively. All syscall handlers are async, enabling blocking operations (e.g. nanosleep, read) without blocking the kernel.

## Syscall Dispatch

**File:** `kernel/src/interrupts/trap.rs`

```rust
fn handle_syscall() {
    let trap_frame = Cpu::read_trap_frame();
    let task = Task::new(async { handler.handle(&trap_frame).await });
    if let Poll::Ready(result) = task.poll(&mut cx) {
        trap_frame[Register::a0] = result;
        sepc += 4;  // Skip ecall
    } else {
        thread.set_syscall_task_and_suspend(task);
        scheduler.schedule();
    }
}
```

## Supported Syscalls

**File:** `kernel/src/syscalls/linux.rs`

| Syscall | Args | Description |
|---------|------|-------------|
| bind | fd, addr, addrlen | Bind socket to address/port |
| brk | brk | Adjust heap break |
| chdir | pathname | Change working directory (validates path is a directory) |
| clock_gettime | clockid, tp | Get clock time |
| clock_nanosleep | clockid, flags, request, remain | Sleep with clock selection |
| clone | flags, stack, ptid, tls, ctid | Create child process (vfork) or thread (CLONE_THREAD) |
| close | fd | Close file descriptor |
| dup3 | oldfd, newfd, flags | Duplicate file descriptor |
| execve | filename, argv, envp | Replace process image (inherits CWD) |
| exit | status | Exit calling thread |
| exit_group | status | Exit process (stores exit status, then kills process) |
| faccessat | dirfd, pathname, mode | Check file accessibility (supports dirfd + CWD-relative) |
| fadvise64 | fd, offset, len, advice | File access advice (stub, returns 0) |
| fcntl | fd, cmd, arg | File descriptor control (F_DUPFD, F_DUPFD_CLOEXEC, F_GETFD, F_SETFD, F_GETFL, F_SETFL) |
| fstat | fd, statbuf | Get file status by fd |
| futex | uaddr, op, val, ... | Fast userspace mutex |
| getcwd | buf, size | Get current working directory |
| getdents64 | fd, dirp, count | Read directory entries |
| getegid | | Get effective group ID (stub, returns 0) |
| geteuid | | Get effective user ID (stub, returns 0) |
| getgid | | Get group ID (stub, returns 0) |
| getpgid | pid | Get process group ID |
| getpid | | Get process ID (main thread TID) |
| getppid | | Get parent process ID |
| getsid | pid | Get session ID |
| gettid | | Get thread ID |
| getuid | | Get user ID (stub, returns 0) |
| ioctl | fd, op, arg | Device control (+ Solaya extensions, FIONBIO for sockets) |
| kill | pid, sig | Send signal to process |
| listxattr | pathname, list, size | List extended attributes (stub, returns 0) |
| llistxattr | pathname, list, size | List extended attributes, no symlink follow (stub, returns 0) |
| lseek | fd, offset, whence | Reposition file offset |
| madvise | addr, length, advice | Memory advice (stub, returns 0) |
| mkdirat | dirfd, pathname, mode | Create directory (supports CWD-relative paths) |
| mmap | addr, len, prot, flags, fd, off | Map memory |
| mprotect | addr, len, prot | Memory protection (stub, returns 0) |
| munmap | addr, len | Unmap memory |
| nanosleep | duration, rem | Sleep |
| newfstatat | dirfd, pathname, statbuf, flags | Get file status (supports dirfd + CWD-relative) |
| openat | dirfd, pathname, flags, mode | Open file (supports dirfd-relative paths, O_CREAT, O_DIRECTORY) |
| pipe2 | fds, flags | Create pipe |
| ppoll | fds, n, timeout, mask | Poll file descriptors |
| prctl | | Process control |
| read | fd, buf, count | Read from fd |
| readlinkat | dirfd, pathname, buf, bufsiz | Read symlink (stub, returns EINVAL) |
| recvfrom | fd, buf, len, flags, src_addr, addrlen | Receive UDP datagram with sender address |
| rt_sigaction | sig, act, oact, size | Set/get signal action |
| rt_sigprocmask | how, set, oldset, size | Set/get signal mask |
| rt_sigreturn | | Restore state after signal handler returns |
| sendto | fd, buf, len, flags, dest_addr, addrlen | Send UDP datagram to destination |
| set_robust_list | head, len | Set robust futex list (stub, returns 0) |
| set_tid_address | tidptr | Set clear_child_tid |
| setpgid | pid, pgid | Set process group ID |
| setsid | | Create new session |
| sigaltstack | uss, uoss | Signal stack |
| socket | domain, type, protocol | Create socket (AF_INET + SOCK_DGRAM only) |
| statx | dirfd, pathname, flags, mask, statxbuf | Extended file status (supports dirfd + CWD-relative) |
| tgkill | tgid, tid, sig | Send signal to thread in thread group |
| tkill | tid, sig | Send signal to thread |
| umask | mask | Set file creation mask (stores per-process, returns previous) |
| unlinkat | dirfd, pathname, flags | Remove file/directory (AT_REMOVEDIR for dirs, EISDIR/ENOTEMPTY checks) |
| utimensat | dirfd, pathname, times, flags | Set file timestamps (stub, returns 0) |
| wait4 | pid, status, options, rusage | Wait for child process (supports WNOHANG) |
| write | fd, buf, count | Write to fd |
| writev | fd, iov, iovcnt | Vectored write |

### LinuxSyscallHandler

The main syscall handler that implements all Linux-compatible system calls.

```rust
pub struct LinuxSyscallHandler {
    current_process: ProcessRef,
    current_thread: ThreadRef,
    current_tid: Tid,
}

impl LinuxSyscalls for LinuxSyscallHandler {
    async fn read(&mut self, fd, buf, count) -> Result<isize, Errno>;
    async fn write(&mut self, fd, buf, count) -> Result<isize, Errno>;
    async fn exit_group(&mut self, status) -> Result<isize, Errno>;
    // ... other syscalls
}
```

When a syscall is invoked, `LinuxSyscallHandler::new()` captures the current thread, process, and TID from the scheduler at syscall entry. These fields are then directly accessible to all syscall implementations without additional indirection.

Syscall implementations are split across concern-grouped files. Each trait method in `linux.rs` is a thin wrapper (≤5 lines) that delegates to a `do_*` helper in the appropriate file. Trivial stubs (`Ok(0)`, `Err(EINVAL)`) stay inline.

| File | Syscalls |
|------|----------|
| `io_ops.rs` | read, write, writev, pipe2, fcntl |
| `ioctl_ops.rs` | ioctl |
| `fs_ops.rs` | openat, fstat, newfstatat, statx, getdents64, faccessat, mkdirat, unlinkat, getcwd |
| `mm_ops.rs` | mmap, mprotect |
| `process_ops.rs` | clone_vfork, clone_thread, wait4 |
| `exec_ops.rs` | execve (do_execve) |
| `signal_ops.rs` | rt_sigaction, rt_sigprocmask, sigaltstack, kill |
| `net_ops.rs` | socket, bind, sendto, recvfrom |
| `time_ops.rs` | nanosleep, clock_nanosleep, ppoll |
| `id_ops.rs` | getpgid, getsid, setpgid, futex |
| `helpers.rs` | path resolution, read_cstring, resolve_dirfd_node |

### Solaya ioctl Extensions

Custom kernel functionality exposed via `ioctl` on stdout. Constants and userspace wrappers defined in `common/src/ioctl.rs`.

| Command | Value | Description |
|---------|-------|-------------|
| SOLAYA_PANIC | 0x5301 | Trigger kernel panic from userspace |

## Userspace Pointer Validation

**File:** `kernel/src/syscalls/linux_validator.rs`

```rust
pub struct LinuxUserspaceArg<P: LinuxPointer>(UserspacePtr<P>);

impl LinuxUserspaceArg<*const T> {
    pub fn validate_ptr(&self) -> Result<T, Errno>;
    pub fn validate_str(&self, len: usize) -> Result<&str, Errno>;
    pub fn validate_slice(&self, len: usize) -> Result<&[T], Errno>;
}

impl LinuxUserspaceArg<*mut T> {
    pub fn write(&self, value: T) -> Result<(), Errno>;
    pub fn write_slice(&self, data: &[T]) -> Result<(), Errno>;
    pub fn write_if_not_none(&self, value: T) -> Result<(), Errno>;
}
```

### Validation Process

1. Check pointer is in userspace address range
2. Translate virtual address through page tables
3. Verify page permissions (read/write)
4. Return kernel-accessible physical address

## Adding a New Syscall

1. Add to `linux_syscalls!` macro in `kernel/src/syscalls/linux.rs`:
```rust
linux_syscalls! {
    SYSCALL_NR_MYSYSCALL => mysyscall(arg1: type1, arg2: type2);
}
```

2. Add a thin trait method in `impl LinuxSyscalls for LinuxSyscallHandler` in `linux.rs` that delegates to a helper:
```rust
async fn mysyscall(&mut self, arg1: LinuxUserspaceArg<type1>, arg2: LinuxUserspaceArg<type2>)
    -> Result<isize, Errno>
{
    self.do_mysyscall(arg1, arg2)
}
```

3. Implement the `do_mysyscall` helper in the appropriate `*_ops.rs` file grouped by concern (e.g., `fs_ops.rs` for filesystem, `net_ops.rs` for networking). Trait methods in `linux.rs` should stay ≤5 lines; trivial stubs (`Ok(0)`, `Err(EINVAL)`) can stay inline.

## Error Handling

Linux syscalls return:
- Success: positive value or 0
- Error: `-Errno` (negative errno value)

```rust
let ret = match result {
    Ok(ret) => ret,
    Err(errno) => -(errno as isize),
};
trap_frame[Register::a0] = ret as usize;
```

## Syscall Tracer

**Config:** `kernel/src/syscalls/trace_config.rs`
**Logic:** `kernel/src/syscalls/tracer.rs`

A compile-time configurable tracer that logs ENTER/EXIT for all syscalls made by processes listed in `TRACED_PROCESSES`. Metadata (syscall name, arg names, arg display formats) is auto-generated by the `linux_syscalls!` macro via the `SyscallArgFormat` trait — adding a new syscall to the macro invocation automatically makes it traceable.

The `trace_syscall()` function in `tracer.rs` wraps `handler.handle()` and is called from `trap.rs`. Argument types are mapped to display formats: `c_int`/`isize` as signed decimal, `usize`/`c_uint`/`c_ulong` as hex, pointers as hex with NULL detection.

Example output:
```
[SYSCALL ENTER] tid=3 write(fd: 1, buf: 0x53fd0, count: 0x11)
[SYSCALL EXIT]  tid=3 write = 17
[SYSCALL ENTER] tid=3 close(fd: -1)
[SYSCALL EXIT]  tid=3 close = -9 (EBADF)
```

## Key Files

| File | Purpose |
|------|---------|
| kernel/src/syscalls/mod.rs | Module exports |
| kernel/src/syscalls/linux.rs | Syscall trait dispatch, struct, macro invocation (~640 lines) |
| kernel/src/syscalls/io_ops.rs | File descriptor I/O: read, write, writev, pipe2, fcntl |
| kernel/src/syscalls/ioctl_ops.rs | Device control: ioctl dispatch |
| kernel/src/syscalls/fs_ops.rs | Filesystem: openat, stat, getdents64, unlinkat, etc. |
| kernel/src/syscalls/mm_ops.rs | Memory management: mmap, mprotect |
| kernel/src/syscalls/signal_ops.rs | Signal handling: rt_sigaction, rt_sigprocmask, kill, etc. |
| kernel/src/syscalls/net_ops.rs | Networking: socket, bind, sendto, recvfrom |
| kernel/src/syscalls/time_ops.rs | Time & polling: nanosleep, clock_nanosleep, ppoll |
| kernel/src/syscalls/id_ops.rs | Process/thread identity: getpgid, setpgid, futex |
| kernel/src/syscalls/process_ops.rs | Process lifecycle: clone_vfork, clone_thread, wait4 |
| kernel/src/syscalls/exec_ops.rs | Exec: do_execve |
| kernel/src/syscalls/helpers.rs | Path/FD helpers for LinuxSyscallHandler |
| kernel/src/syscalls/macros.rs | linux_syscalls! macro + SYSCALL_METADATA generation |
| kernel/src/syscalls/linux_validator.rs | LinuxUserspaceArg validation |
| kernel/src/syscalls/trace_config.rs | Syscall tracer process name configuration |
| kernel/src/syscalls/tracer.rs | Syscall tracer logic and types |
| common/src/ioctl.rs | Solaya ioctl constants + userspace wrappers |
| headers/src/syscall_types.rs | Syscall type definitions |
| headers/src/errno.rs | Error codes |
