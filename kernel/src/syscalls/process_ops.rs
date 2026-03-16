use alloc::{string::String, sync::Arc};
use core::ffi::{c_int, c_ulong};
use headers::{
    errno::Errno,
    syscall_types::{CLONE_CHILD_CLEARTID, CLONE_PARENT_SETTID, CLONE_SETTLS},
};

use crate::{
    cpu::Cpu,
    klibc::Spinlock,
    memory::VirtAddr,
    processes::{
        process::Process,
        process_table,
        thread::{Thread, get_next_tid},
        wait_child::{WaitChild, WaitPid},
    },
    syscalls::linux_validator::LinuxUserspaceArg,
};
use common::{pid::Tid, syscalls::trap_frame::Register};

use super::linux::LinuxSyscallHandler;

impl LinuxSyscallHandler {
    pub(super) async fn clone_fork(&mut self, stack: usize) -> Result<isize, Errno> {
        let parent_regs = Cpu::read_trap_frame();
        let parent_pc = sys::cpu::read_sepc();

        let parent_process = self.current_process.clone();
        let (parent_main_tid, child_name, parent_pgid, parent_sid) =
            parent_process.with_lock(|p| {
                (
                    p.main_tid(),
                    Arc::new(String::from(p.get_name())),
                    p.pgid(),
                    p.sid(),
                )
            });

        let child_tid = get_next_tid();

        let forked = parent_process.with_lock(|p| p.fork_address_space());

        let child_process = Arc::new(Spinlock::new(Process::new(
            child_name.clone(),
            forked.page_table,
            forked.allocated_pages,
            forked.brk,
            child_tid,
            parent_pgid,
            parent_sid,
        )));

        let (parent_fd_table, parent_cwd, parent_umask) =
            parent_process.with_lock(|p| (p.fd_table().clone(), String::from(p.cwd()), p.umask()));
        {
            let mut child = child_process.lock();
            child.set_fd_table(parent_fd_table);
            child.set_cwd(parent_cwd);
            child.set_umask(parent_umask);
            child.set_mmap_state(forked.mmap_allocations, forked.free_mmap_address);
        }

        let mut child_regs = parent_regs;
        child_regs[Register::a0] = 0;
        if stack != 0 {
            child_regs[Register::sp] = stack;
        }

        let child_thread = Thread::new(
            child_tid,
            child_name,
            child_regs,
            VirtAddr::new(parent_pc + 4),
            false,
            child_process.clone(),
            parent_main_tid,
        );

        child_process
            .lock()
            .add_thread(child_tid, Arc::downgrade(&child_thread));
        process_table::THE.lock().add_thread(child_thread);

        Ok(child_tid.as_isize())
    }

    pub(super) fn clone_thread(
        &mut self,
        flags: c_ulong,
        stack: usize,
        ptid: LinuxUserspaceArg<Option<*mut c_int>>,
        tls: c_ulong,
        ctid: LinuxUserspaceArg<Option<*mut c_int>>,
    ) -> Result<isize, Errno> {
        let parent_regs = Cpu::read_trap_frame();
        let parent_pc = sys::cpu::read_sepc();

        let parent_process = self.current_process.clone();
        let (parent_main_tid, child_name) =
            parent_process.with_lock(|p| (p.main_tid(), Arc::new(String::from(p.get_name()))));

        let child_tid = get_next_tid();

        let mut child_regs = parent_regs;
        child_regs[Register::a0] = 0;
        if stack != 0 {
            child_regs[Register::sp] = stack;
        }
        if (flags & c_ulong::from(CLONE_SETTLS)) != 0 {
            child_regs[Register::tp] = usize::try_from(tls).expect("tls fits in usize");
        }

        let child_thread = Thread::new(
            child_tid,
            child_name,
            child_regs,
            VirtAddr::new(parent_pc + 4),
            false,
            parent_process.clone(),
            parent_main_tid,
        );

        if (flags & c_ulong::from(CLONE_CHILD_CLEARTID)) != 0 {
            child_thread.lock().set_clear_child_tid((&ctid).into());
        }

        parent_process.with_lock(|mut p| {
            p.add_thread(child_tid, Arc::downgrade(&child_thread));
        });

        if (flags & c_ulong::from(CLONE_PARENT_SETTID)) != 0 {
            ptid.write_if_not_none(
                c_int::try_from(child_tid.as_isize()).expect("tid fits in c_int"),
            )?;
        }

        process_table::THE.lock().add_thread(child_thread);

        Ok(child_tid.as_isize())
    }

    pub(super) async fn do_wait4(
        &self,
        pid: c_int,
        status: LinuxUserspaceArg<Option<*mut c_int>>,
        options: c_int,
    ) -> Result<isize, Errno> {
        let wnohang = (options & headers::syscall_types::WNOHANG as c_int) != 0;
        let wuntraced = (options & headers::syscall_types::WUNTRACED as c_int) != 0;
        let known_flags =
            (headers::syscall_types::WNOHANG | headers::syscall_types::WUNTRACED) as c_int;
        assert!(
            options & !known_flags == 0,
            "wait4: unsupported options {options:#x}"
        );

        let parent_main_tid = self.current_thread.lock().get_tid();
        let target = if pid > 0 {
            WaitPid::Specific(Tid::try_from_i32(pid).expect("pid is positive"))
        } else if pid == -1 {
            WaitPid::Any
        } else if pid == 0 {
            let own_pgid = self.current_process.with_lock(|p| p.pgid());
            WaitPid::Pgid(own_pgid)
        } else {
            // pid < -1: wait for any child whose pgid == abs(pid)
            let abs_pid = pid.checked_neg().ok_or(Errno::EINVAL)?;
            WaitPid::Pgid(Tid::try_from_i32(abs_pid).expect("abs(pid) is positive"))
        };
        let (child_tid, wait_status) =
            WaitChild::new(parent_main_tid, target, wnohang, wuntraced).await?;

        status.write_if_not_none(wait_status)?;

        Ok(child_tid.as_isize())
    }
}
