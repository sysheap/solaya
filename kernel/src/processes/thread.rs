use super::process::ProcessRef;
use crate::{
    debug,
    klibc::elf::ElfFile,
    memory::{VirtAddr, page::PinnedHeapPages, page_tables::RootPageTableHolder},
    processes::{
        brk::Brk,
        loader::{self, LoadedElf},
        process::{POWERSAVE_TID, Process},
        task::Task,
        userspace_ptr::{ContainsUserspacePtr, UserspacePtr},
    },
};
use alloc::{
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
};
use common::{
    errors::LoaderError,
    pid::Tid,
    syscalls::trap_frame::{Register, TrapFrame},
};
use core::{
    ffi::{c_int, c_uint},
    fmt::Debug,
    ptr::null_mut,
    sync::atomic::{AtomicU64, Ordering},
};
use headers::{
    errno::Errno,
    syscall_types::{_NSIG, sigaction, sigaltstack, sigset_t, stack_t},
};
use sys::klibc::send_sync::UnsafeSendSync;

use crate::klibc::Spinlock;

pub type ThreadRef = Arc<Spinlock<Thread>>;
pub type ThreadWeakRef = Weak<Spinlock<Thread>>;

pub type SyscallTask = Task<Result<isize, Errno>>;

pub fn get_next_tid() -> Tid {
    // PIDs will start from 1
    // 0 is reserved for the powersave process
    static TID_COUNTER: AtomicU64 = AtomicU64::new(1);
    let next_tid = TID_COUNTER.fetch_add(1, Ordering::Relaxed);
    assert_ne!(next_tid, u64::MAX, "We ran out of process pids");
    Tid::new(next_tid)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Running { cpu_id: crate::cpu::CpuId },
    Runnable,
    Waiting,
    Stopped,
    Zombie(super::signal::ExitStatus),
}

#[derive(Debug)]
struct SignalState {
    sigaltstack: ContainsUserspacePtr<stack_t>,
    sigmask: sigset_t,
    sigaction: [sigaction; _NSIG as usize],
    #[allow(dead_code)]
    pending: super::signal::PendingSignals,
}

impl SignalState {
    fn new() -> Self {
        Self {
            sigaltstack: ContainsUserspacePtr(UnsafeSendSync(sigaltstack {
                ss_sp: null_mut(),
                ss_flags: 0,
                ss_size: 0,
            })),
            sigmask: sigset_t { sig: [0] },
            sigaction: [sigaction {
                sa_handler: None,
                sa_flags: 0,
                sa_mask: sigset_t { sig: [0] },
            }; _NSIG as usize],
            pending: super::signal::PendingSignals::new(),
        }
    }
}

#[derive(Debug)]
pub struct Thread {
    tid: Tid,
    parent_tid: Tid,
    process_name: Arc<String>,
    register_state: TrapFrame,
    program_counter: VirtAddr,
    state: ThreadState,
    wakeup_pending: bool,
    in_kernel_mode: bool,
    process: ProcessRef,
    clear_child_tid: Option<UserspacePtr<*mut c_int>>,
    signal_state: SignalState,
    syscall_task: Option<SyscallTask>,
    // Set by execve when it replaces the thread's register state with a new
    // program's entry state. Signals the syscall return path to skip writing a
    // return value to a0 and skip advancing PC past ecall.
    registers_replaced: bool,
    pub stopped_notified: bool,
    pub stop_signal: u32,
}

impl core::fmt::Display for Thread {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "tid={} process_name={} pc={} state={:?} wakeup_pending={} in_kernel_mode={}",
            self.tid,
            self.process_name,
            self.program_counter,
            self.state,
            self.wakeup_pending,
            self.in_kernel_mode,
        )
    }
}

impl Thread {
    pub fn create_powersave_thread() -> Arc<Spinlock<Self>> {
        let allocated_pages = BTreeMap::new();

        let page_table = RootPageTableHolder::new_with_kernel_mapping(&[]);

        let register_state = TrapFrame::zero();

        Self::new_process(
            "powersave",
            POWERSAVE_TID,
            register_state,
            page_table,
            VirtAddr::new(crate::asm::powersave_fn_addr()),
            allocated_pages,
            true,
            Brk::empty(),
            POWERSAVE_TID,
            POWERSAVE_TID,
            POWERSAVE_TID,
        )
    }

    pub fn from_elf(
        elf_file: &ElfFile,
        name: &str,
        args: &[&str],
        env: &[&str],
        parent_tid: Tid,
    ) -> Result<Arc<Spinlock<Self>>, LoaderError> {
        debug!("Create process from elf file");

        let LoadedElf {
            entry_address,
            page_tables: page_table,
            allocated_pages,
            args_start,
            brk,
        } = loader::load_elf(elf_file, name, args, env)?;

        let mut register_state = TrapFrame::zero();
        register_state[Register::a0] = args_start.as_usize();
        register_state[Register::sp] = args_start.as_usize();

        let tid = get_next_tid();
        Ok(Self::new_process(
            name,
            tid,
            register_state,
            page_table,
            entry_address,
            allocated_pages,
            false,
            brk,
            parent_tid,
            tid,
            tid,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_process(
        name: impl Into<String>,
        tid: Tid,
        register_state: TrapFrame,
        page_table: RootPageTableHolder,
        program_counter: VirtAddr,
        allocated_pages: BTreeMap<VirtAddr, PinnedHeapPages>,
        in_kernel_mode: bool,
        brk: Brk,
        parent_tid: Tid,
        pgid: Tid,
        sid: Tid,
    ) -> ThreadRef {
        let name = Arc::new(name.into());
        let process = Arc::new(Spinlock::new(Process::new(
            name.clone(),
            page_table,
            allocated_pages,
            brk,
            tid,
            pgid,
            sid,
        )));

        let main_thread = Thread::new(
            tid,
            name,
            register_state,
            program_counter,
            in_kernel_mode,
            process.clone(),
            parent_tid,
        );

        process
            .lock()
            .add_thread(tid, ThreadRef::downgrade(&main_thread));

        main_thread
    }

    pub fn new(
        tid: Tid,
        process_name: Arc<String>,
        register_state: TrapFrame,
        program_counter: VirtAddr,
        in_kernel_mode: bool,
        process: ProcessRef,
        parent_tid: Tid,
    ) -> ThreadRef {
        Arc::new(Spinlock::new(Self {
            tid,
            parent_tid,
            process_name,
            register_state,
            program_counter,
            state: ThreadState::Runnable,
            wakeup_pending: false,
            in_kernel_mode,
            process,
            clear_child_tid: None,
            signal_state: SignalState::new(),
            syscall_task: None,
            registers_replaced: false,
            stopped_notified: false,
            stop_signal: 0,
        }))
    }

    pub fn get_name(&self) -> &str {
        &self.process_name
    }

    pub fn set_syscall_task_and_suspend(&mut self, task: SyscallTask) {
        assert!(self.syscall_task.is_none(), "syscall task is already set");
        if matches!(self.state, ThreadState::Zombie(_)) {
            // Thread was killed by another CPU between poll() returning Pending
            // and now. Don't store the task or suspend — the thread is dead.
            return;
        }
        self.syscall_task = Some(task);
        if self.wakeup_pending {
            // A waker fired between poll() returning Pending and now.
            // The thread is still Running so wake_up() couldn't transition
            // it to Runnable. Don't suspend — stay schedulable.
            self.wakeup_pending = false;
        } else if self.has_pending_unblocked_signal() {
            // A signal arrived while the thread was Running (before the syscall
            // yielded). Don't suspend — stay schedulable so the scheduler can
            // deliver the signal and return EINTR.
        } else {
            self.suspend();
        }
    }

    pub fn wake_up(&mut self) -> bool {
        if self.state == ThreadState::Waiting {
            self.state = ThreadState::Runnable;
            return true;
        }
        if matches!(self.state, ThreadState::Running { .. }) {
            self.wakeup_pending = true;
        }
        false
    }

    pub fn suspend_unless_wakeup_pending(&mut self) {
        if self.wakeup_pending {
            self.wakeup_pending = false;
        } else {
            self.suspend();
        }
    }

    fn suspend(&mut self) {
        assert_ne!(
            self.state,
            ThreadState::Waiting,
            "Thread should not be already in waiting state"
        );
        self.state = ThreadState::Waiting;
    }

    pub fn get_tid(&self) -> Tid {
        self.tid
    }

    pub fn parent_tid(&self) -> Tid {
        self.parent_tid
    }

    pub fn set_parent_tid(&mut self, parent_tid: Tid) {
        self.parent_tid = parent_tid;
    }

    pub fn set_sigaction(&mut self, sig: c_uint, sigaction: sigaction) -> Result<sigaction, Errno> {
        if sig >= _NSIG {
            return Err(Errno::EINVAL);
        }
        Ok(core::mem::replace(
            &mut self.signal_state.sigaction[sig as usize],
            sigaction,
        ))
    }

    pub fn get_sigaction(&self, sig: c_uint) -> Result<sigaction, Errno> {
        if sig >= _NSIG {
            return Err(Errno::EINVAL);
        }
        Ok(self.signal_state.sigaction[sig as usize])
    }

    pub fn get_sigset(&self) -> sigset_t {
        self.signal_state.sigmask
    }

    pub fn set_sigset(&mut self, sigmask: sigset_t) -> sigset_t {
        core::mem::replace(&mut self.signal_state.sigmask, sigmask)
    }

    pub fn get_sigaltstack(&self) -> sigaltstack {
        *self.signal_state.sigaltstack.0
    }

    pub fn set_sigaltstack(&mut self, sigaltstack: &sigaltstack) {
        self.signal_state.sigaltstack.0 = UnsafeSendSync(*sigaltstack);
    }

    pub fn clear_wakeup_pending(&mut self) {
        self.wakeup_pending = false;
    }

    pub fn take_syscall_task(&mut self) -> Option<SyscallTask> {
        self.syscall_task.take()
    }

    pub fn store_syscall_task(&mut self, task: SyscallTask) {
        self.syscall_task = Some(task);
    }

    pub fn set_clear_child_tid(&mut self, clear_child_tid: UserspacePtr<*mut c_int>) {
        self.clear_child_tid = Some(clear_child_tid);
    }

    pub fn get_clear_child_tid(&self) -> &Option<UserspacePtr<*mut c_int>> {
        &self.clear_child_tid
    }

    pub fn get_register_state(&self) -> &TrapFrame {
        &self.register_state
    }

    pub fn get_register_state_mut(&mut self) -> &mut TrapFrame {
        &mut self.register_state
    }

    pub fn set_register_state(&mut self, register_state: TrapFrame) {
        self.register_state = register_state;
    }

    pub fn get_program_counter(&self) -> VirtAddr {
        self.program_counter
    }

    pub fn set_program_counter(&mut self, program_counter: VirtAddr) {
        self.program_counter = program_counter;
    }

    pub fn get_state(&self) -> ThreadState {
        self.state
    }

    pub fn set_state(&mut self, state: ThreadState) {
        self.state = state;
    }

    pub fn get_in_kernel_mode(&self) -> bool {
        self.in_kernel_mode
    }

    pub fn process(&self) -> ProcessRef {
        self.process.clone()
    }

    pub fn set_process(&mut self, new_process: ProcessRef, name: Arc<String>) {
        self.process = new_process;
        self.process_name = name;
    }

    pub fn registers_replaced(&self) -> bool {
        self.registers_replaced
    }

    pub fn set_registers_replaced(&mut self, value: bool) {
        self.registers_replaced = value;
    }

    pub fn clear_pending_stop_signals(&mut self) {
        use headers::syscall_types::{SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU};
        for sig in [SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU] {
            self.signal_state.pending.clear(sig);
        }
    }

    pub fn raise_signal(&mut self, sig: u32) {
        use super::signal::{DefaultAction, default_action};
        use headers::syscall_types::{SIGCONT, SIGKILL, SIGSTOP};

        assert!((1..=31).contains(&sig), "signal {sig} out of range 1..=31");

        // SIGCONT cancels pending stop signals; stop signals cancel pending SIGCONT
        match default_action(sig) {
            DefaultAction::Continue => self.clear_pending_stop_signals(),
            DefaultAction::Stop => self.signal_state.pending.clear(SIGCONT),
            _ => {}
        }

        // SIGKILL and SIGSTOP cannot be caught, blocked, or ignored
        if sig == SIGKILL || sig == SIGSTOP {
            self.signal_state.pending.raise(sig);
            return;
        }

        let action = &self.signal_state.sigaction[sig as usize];
        let handler = action.sa_handler;

        match handler {
            None => {
                // SIG_DFL
                match default_action(sig) {
                    DefaultAction::Ignore => {}
                    DefaultAction::Terminate | DefaultAction::Stop | DefaultAction::Continue => {
                        self.signal_state.pending.raise(sig);
                    }
                }
            }
            Some(f) if f as usize == 1 => {
                // SIG_IGN
            }
            Some(_) => {
                self.signal_state.pending.raise(sig);
            }
        }
    }

    pub fn has_pending_unblocked_signal(&self) -> bool {
        self.signal_state
            .pending
            .first_unblocked(self.signal_state.sigmask.sig[0])
            .is_some()
    }

    pub fn peek_first_unblocked_signal(&self) -> Option<u32> {
        self.signal_state
            .pending
            .first_unblocked(self.signal_state.sigmask.sig[0])
    }

    pub fn take_next_pending_signal(&mut self) -> Option<u32> {
        let sig = self
            .signal_state
            .pending
            .first_unblocked(self.signal_state.sigmask.sig[0])?;
        self.signal_state.pending.clear(sig);
        Some(sig)
    }

    pub fn get_sigaction_raw(&self, sig: u32) -> &sigaction {
        &self.signal_state.sigaction[sig as usize]
    }

    pub fn get_sigmask(&self) -> u64 {
        self.signal_state.sigmask.sig[0]
    }

    pub fn set_sigmask_raw(&mut self, mask: u64) {
        self.signal_state.sigmask.sig[0] = mask;
    }
}
