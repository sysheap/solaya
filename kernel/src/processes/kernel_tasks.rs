use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    task::Wake,
};
use common::{pid::Tid, syscalls::trap_frame::TrapFrame};
use core::{
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    task::{Context, Waker},
};

use crate::{
    klibc::Spinlock,
    memory::{VirtAddr, page_tables::RootPageTableHolder},
    processes::brk::Brk,
};

use super::{
    process::POWERSAVE_TID,
    process_table,
    task::Task,
    thread::{ThreadRef, get_next_tid},
};

const WORKER_STACK_SIZE: usize = 16 * 1024;

static TASKS: Spinlock<BTreeMap<usize, Task<()>>> = Spinlock::new(BTreeMap::new());
static READY_IDS: Spinlock<VecDeque<usize>> = Spinlock::new(VecDeque::new());
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
static WORKER_THREAD: Spinlock<Option<ThreadRef>> = Spinlock::new(None);
static WORKER_SLEEP_REQUESTED: AtomicBool = AtomicBool::new(false);
static WORKER_TID: AtomicU64 = AtomicU64::new(u64::MAX);

pub fn spawn(future: impl Future<Output = ()> + Send + 'static) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    TASKS.lock().insert(id, Task::new(future));
    READY_IDS.lock().push_back(id);
    wake_worker_thread();
}

fn poll_ready_tasks() {
    loop {
        let id = match READY_IDS.lock().pop_front() {
            Some(id) => id,
            None => return,
        };
        let mut task = match TASKS.lock().remove(&id) {
            Some(task) => task,
            None => {
                // Waker fired while this task was being polled. Re-queue
                // so the wakeup isn't lost; return to avoid spinning.
                READY_IDS.lock().push_back(id);
                return;
            }
        };
        let waker = Waker::from(Arc::new(KernelTaskWaker { task_id: id }));
        let mut cx = Context::from_waker(&waker);
        if task.poll(&mut cx).is_pending() {
            TASKS.lock().insert(id, task);
        }
    }
}

pub fn create_worker_thread() {
    let stack = alloc::boxed::Box::leak(alloc::vec![0u8; WORKER_STACK_SIZE].into_boxed_slice());
    let stack_top = stack.as_ptr() as usize + WORKER_STACK_SIZE;

    let page_table = RootPageTableHolder::new_with_kernel_mapping(true);

    let mut register_state = TrapFrame::zero();
    register_state[common::syscalls::trap_frame::Register::sp] = stack_top;

    let tid = get_next_tid();
    let thread = super::thread::Thread::new_process(
        "kernel_worker",
        tid,
        register_state,
        page_table,
        VirtAddr::new(kernel_worker_entry as *const () as usize),
        alloc::collections::BTreeMap::new(),
        true,
        Brk::empty(),
        POWERSAVE_TID,
        tid,
        tid,
    );

    WORKER_TID.store(tid.as_u64(), Ordering::Relaxed);
    *WORKER_THREAD.lock() = Some(thread.clone());
    process_table::RUN_QUEUE.lock().push_back(thread);
}

extern "C" fn kernel_worker_entry() -> ! {
    loop {
        poll_ready_tasks();
        if READY_IDS.lock().is_empty() {
            WORKER_SLEEP_REQUESTED.store(true, Ordering::Release);
            sys::cpu::trigger_supervisor_software_interrupt();
        }
    }
}

pub fn take_sleep_request() -> bool {
    WORKER_SLEEP_REQUESTED.swap(false, Ordering::Acquire)
}

pub fn is_current_worker_tid(tid: Tid) -> bool {
    tid.as_u64() == WORKER_TID.load(Ordering::Relaxed)
}

fn wake_worker_thread() {
    if let Some(thread) = WORKER_THREAD.lock().as_ref() {
        let woke = thread.lock().wake_up();
        if woke {
            process_table::RUN_QUEUE.lock().push_back(thread.clone());
        }
    }
}

struct KernelTaskWaker {
    task_id: usize,
}

impl Wake for KernelTaskWaker {
    fn wake(self: Arc<Self>) {
        READY_IDS.lock().push_back(self.task_id);
        wake_worker_thread();
    }
}
