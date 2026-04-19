use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use headers::errno::Errno;

use crate::{
    fs,
    processes::{elf::ElfFile, loader, process::Process, userspace_ptr::UserspacePtr},
};
use abi::syscalls::trap_frame::{Register, TrapFrame};
use klib::parser::ConsumableBuffer;

use super::linux::{LinuxSyscallHandler, LinuxSyscalls};

impl LinuxSyscallHandler {
    fn read_string_array(&self, array_ptr: usize) -> Result<Vec<Vec<u8>>, Errno> {
        let process = self.get_process();
        let mut buffers = Vec::new();
        let mut current = array_ptr;
        loop {
            let ptr_val = process.with_lock(|p| {
                let ptr = UserspacePtr::new(current as *const usize);
                p.read_userspace_ptr(&ptr)
            })?;
            if ptr_val == 0 {
                break;
            }
            let max_read = usize::MAX.wrapping_sub(ptr_val).wrapping_add(1).min(256);
            let bytes = process.with_lock(|p| {
                let uptr = UserspacePtr::new(ptr_val as *const u8);
                p.read_userspace_slice(&uptr, max_read)
            })?;
            buffers.push(bytes);
            current = current.wrapping_add(core::mem::size_of::<usize>());
        }
        Ok(buffers)
    }

    pub(super) fn do_execve(
        &mut self,
        filename: usize,
        argv: usize,
        envp: usize,
    ) -> Result<isize, Errno> {
        let process = self.get_process();
        let filename_bytes = process.with_lock(|p| {
            let ptr = UserspacePtr::new(filename as *const u8);
            p.read_userspace_slice(&ptr, 256)
        })?;
        let mut buf = ConsumableBuffer::new(&filename_bytes);
        let filename_str = buf.consume_str().ok_or(Errno::EFAULT)?;

        let argv_buffers = self.read_string_array(argv)?;
        let mut args: Vec<&str> = Vec::new();
        // Skip argv[0] (program name) since load_elf adds it automatically
        for buf_ref in argv_buffers.iter().skip(1) {
            let mut cb = ConsumableBuffer::new(buf_ref);
            if let Some(s) = cb.consume_str() {
                args.push(s);
            }
        }

        let envp_buffers = if envp != 0 {
            self.read_string_array(envp)?
        } else {
            Vec::new()
        };
        let mut env_strs: Vec<&str> = Vec::new();
        for buf_ref in &envp_buffers {
            let mut cb = ConsumableBuffer::new(buf_ref);
            if let Some(s) = cb.consume_str() {
                env_strs.push(s);
            }
        }

        let old_cwd_str = self.get_process().with_lock(|p| String::from(p.cwd()));

        // Resolve the filename (plus any shebang layers) against the VFS.
        // Errors (ENOENT, EACCES, ELOOP, EIO, E2BIG, ENOEXEC) propagate to
        // userspace as-is.
        let (vfs_bytes, final_argv) = resolve_shebang(filename_str, &args, &old_cwd_str)?;
        let elf_arc: Arc<[u8]> = Arc::<[u8]>::from(vfs_bytes.as_slice());

        // After shebang resolution, argv[0] is the interpreter path (or the
        // original filename if no shebang); the binary's basename becomes
        // the process name.
        let resolved_path = &final_argv[0];
        let name = resolved_path
            .rsplit('/')
            .next()
            .unwrap_or(resolved_path.as_str());
        let args_refs: Vec<&str> = final_argv.iter().skip(1).map(String::as_str).collect();

        let elf = ElfFile::parse(&elf_arc).map_err(|_| Errno::ENOEXEC)?;
        let loaded =
            loader::load_elf(&elf, name, &args_refs, &env_strs).expect("ELF loading must succeed");

        let process_name = Arc::new(String::from(name));
        let old_process = self.get_process();
        let (old_pgid, old_sid, old_creds) =
            old_process.with_lock(|p| (p.pgid(), p.sid(), p.credentials().clone()));
        let new_process = Arc::new(hal::spinlock::Spinlock::new(Process::new(
            process_name.clone(),
            loaded.page_tables,
            loaded.allocated_pages,
            loaded.brk,
            self.current_thread.lock().get_tid(),
            old_pgid,
            old_sid,
            loaded.auxv_bytes,
        )));

        let mut inherited_fd_table = self.get_process().with_lock(|p| p.fd_table().clone());
        inherited_fd_table.close_cloexec_fds();
        {
            let mut np = new_process.lock();
            np.set_fd_table(inherited_fd_table);
            np.set_cwd(old_cwd_str);
            np.set_credentials(old_creds);
            np.set_elf_bytes(elf_arc);
        }

        let current_thread = self.current_thread.clone();
        let old_process = current_thread.lock().process();
        let tid = current_thread.lock().get_tid();

        old_process.lock().remove_thread(tid);

        current_thread.with_lock(|mut t| {
            t.set_process(new_process.clone(), process_name);
            let mut regs = TrapFrame::zero();
            regs[Register::a0] = loaded.args_start.as_usize();
            regs[Register::sp] = loaded.args_start.as_usize();
            t.set_register_state(regs);
            t.set_program_counter(loaded.entry_address);
            t.set_registers_replaced(true);
        });

        new_process
            .lock()
            .add_thread(tid, Arc::downgrade(&current_thread));

        Ok(0)
    }
}

/// Linux caps shebang recursion at 4 layers (`BINPRM_MAX_RECURSION`).
const MAX_SHEBANG_DEPTH: usize = 4;

/// Maximum bytes of the first line we inspect for a `#!` header.  Matches
/// Linux's `BINPRM_BUF_SIZE` envelope: `#!` + 255 interpreter bytes + `\n`.
const SHEBANG_MAX_LINE: usize = 257;

/// Parse a `#!` line.  Returns `(interpreter, optional_arg)` where the
/// optional arg is the remainder of the line after the interpreter token,
/// treated as a single argument (matching Linux behavior — no splitting).
///
/// Returns `Err(Errno::ENOEXEC)` if the shebang header is malformed (no
/// newline within the bound, empty interpreter, ...).
fn parse_shebang(bytes: &[u8]) -> Result<(String, Option<String>), Errno> {
    let scan_len = bytes.len().min(SHEBANG_MAX_LINE);
    let line_end = bytes[..scan_len]
        .iter()
        .position(|&b| b == b'\n')
        .ok_or(Errno::ENOEXEC)?;
    // Skip the `#!` prefix then leading spaces/tabs.
    let after_bang = &bytes[2..line_end];
    let start = after_bang
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .ok_or(Errno::ENOEXEC)?;
    let rest = &after_bang[start..];
    let interp_end = rest
        .iter()
        .position(|&b| b == b' ' || b == b'\t')
        .unwrap_or(rest.len());
    if interp_end == 0 {
        return Err(Errno::ENOEXEC);
    }
    let interpreter = core::str::from_utf8(&rest[..interp_end])
        .map_err(|_| Errno::ENOEXEC)?
        .to_string();
    let arg_region = &rest[interp_end..];
    let arg_start = arg_region
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(arg_region.len());
    let trimmed = &arg_region[arg_start..];
    let trimmed_end = trimmed
        .iter()
        .rposition(|&b| b != b' ' && b != b'\t')
        .map(|p| p + 1)
        .unwrap_or(0);
    let optional_arg = if trimmed_end == 0 {
        None
    } else {
        Some(
            core::str::from_utf8(&trimmed[..trimmed_end])
                .map_err(|_| Errno::ENOEXEC)?
                .to_string(),
        )
    };
    Ok((interpreter, optional_arg))
}

/// Read the file at `filename`, following up to `MAX_SHEBANG_DEPTH` layers
/// of `#!` indirection.  Returns the final binary bytes plus the full argv
/// (argv[0] is the resolved interpreter or original filename, followed by
/// any shebang-contributed args, then the caller's `trailing_args`).
fn resolve_shebang(
    filename: &str,
    trailing_args: &[&str],
    cwd: &str,
) -> Result<(Vec<u8>, Vec<String>), Errno> {
    let mut current_path = String::from(filename);
    // Per-layer (optional_arg, script_path) in discovery order (outermost
    // first).  On exit, innermost interpreter path is `current_path`.
    let mut layers: Vec<(Option<String>, String)> = Vec::new();
    let bytes = loop {
        let bytes = try_read_from_vfs(&current_path, cwd)?;
        if bytes.len() < 2 || &bytes[..2] != b"#!" {
            break bytes;
        }
        if layers.len() >= MAX_SHEBANG_DEPTH {
            return Err(Errno::ELOOP);
        }
        let (interpreter, optional_arg) = parse_shebang(&bytes)?;
        layers.push((optional_arg, current_path));
        current_path = interpreter;
    };
    // Assemble argv.  Linux semantics:
    //   argv[0] = innermost interpreter (the actual binary)
    //   then, unwinding innermost-layer first:
    //       if that layer had an optional arg: push it
    //       push that layer's script path
    //   then trailing_args (argv[1..] from the original execve call)
    let mut argv: Vec<String> = Vec::with_capacity(1 + layers.len() * 2 + trailing_args.len());
    argv.push(current_path);
    for (opt_arg, script) in layers.into_iter().rev() {
        if let Some(a) = opt_arg {
            argv.push(a);
        }
        argv.push(script);
    }
    for a in trailing_args {
        argv.push(String::from(*a));
    }
    Ok((bytes, argv))
}

/// Read the whole file at `filename` into memory, preserving the VFS
/// errno so userspace can distinguish ENOENT / EACCES / ELOOP / EIO.
///
/// Path resolution follows execve(2): absolute paths resolve against the
/// VFS root, relative paths against `cwd`.  No PATH search — shells are
/// expected to do that themselves (dash/busybox ash both do).
fn try_read_from_vfs(filename: &str, cwd: &str) -> Result<Vec<u8>, Errno> {
    let absolute: String = if filename.starts_with('/') {
        filename.to_string()
    } else if cwd.ends_with('/') {
        alloc::format!("{cwd}{filename}")
    } else {
        alloc::format!("{cwd}/{filename}")
    };
    let node = fs::resolve_path(&absolute)?;
    let size = node.size();
    // Refuse outlandish sizes to avoid a rogue or corrupt VFS entry
    // allocating the whole heap; 64 MiB is ~10× the largest userspace
    // binary we produce.
    if size > 64 * 1024 * 1024 {
        return Err(Errno::E2BIG);
    }
    let mut buf: Vec<u8> = alloc::vec![0u8; size];
    let n = node.read(0, &mut buf)?;
    buf.truncate(n);
    Ok(buf)
}
