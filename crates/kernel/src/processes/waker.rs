use alloc::{sync::Arc, task::Wake};
use core::task::Waker;

use crate::processes::{process_table, thread::ThreadWeakRef};

pub struct ThreadWaker {
    thread: ThreadWeakRef,
}

impl ThreadWaker {
    pub fn new_waker(thread: ThreadWeakRef) -> Waker {
        let task_waker = Arc::new(ThreadWaker { thread });
        task_waker.into()
    }
}

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        if let Some(thread) = self.thread.upgrade() {
            let woke = thread.lock().wake_up();
            if woke {
                process_table::RUN_QUEUE.lock().push_back(thread);
            }
        }
    }
}
