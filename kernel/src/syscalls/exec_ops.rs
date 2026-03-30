use alloc::{string::String, sync::Arc, vec::Vec};
use headers::errno::Errno;

use crate::{
    fs::vfs::{self, NodeType},
    klibc::{consumable_buffer::ConsumableBuffer, elf::ElfFile},
    processes::{loader, process::Process, userspace_ptr::UserspacePtr},
};
use common::syscalls::trap_frame::{Register, TrapFrame};

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
        let name = filename_str.rsplit('/').next().unwrap_or(filename_str);

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

        let node = vfs::resolve_path(filename_str)?;
        if node.node_type() != NodeType::File {
            return Err(Errno::EACCES);
        }
        let size = node.size();
        let mut buf = sys::klibc::util::AlignedBuffer::new(size);
        node.read(0, buf.as_bytes_mut()).map_err(|_| Errno::EIO)?;

        let elf = ElfFile::parse(buf.as_bytes()).map_err(|_| Errno::ENOEXEC)?;
        let loaded = loader::load_elf(&elf, name, &args, &env_strs).map_err(|_| Errno::ENOMEM)?;

        let process_name = Arc::new(String::from(name));
        let old_process = self.get_process();
        let (old_pgid, old_sid, old_cwd, old_creds) = old_process.with_lock(|p| {
            (
                p.pgid(),
                p.sid(),
                String::from(p.cwd()),
                p.credentials().clone(),
            )
        });
        let new_process = Arc::new(crate::klibc::Spinlock::new(Process::new(
            process_name.clone(),
            loaded.page_tables,
            loaded.allocated_pages,
            loaded.brk,
            self.current_thread.lock().get_tid(),
            old_pgid,
            old_sid,
        )));

        let mut inherited_fd_table = self.get_process().with_lock(|p| p.fd_table().clone());
        inherited_fd_table.close_cloexec_fds();
        {
            let mut np = new_process.lock();
            np.set_fd_table(inherited_fd_table);
            np.set_cwd(old_cwd);
            np.set_credentials(old_creds);
            np.set_binary_path(Arc::new(String::from(filename_str)));
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
