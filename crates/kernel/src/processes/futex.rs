use alloc::{collections::BTreeMap, vec::Vec};
use common::pid::Tid;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use headers::errno::Errno;

use crate::{klibc::Spinlock, processes::process::ProcessRef};

use super::userspace_ptr::UserspacePtr;

type FutexKey = (Tid, usize);

static WAITERS: Spinlock<BTreeMap<FutexKey, Vec<Waker>>> = Spinlock::new(BTreeMap::new());

pub struct FutexWait {
    process: ProcessRef,
    addr: usize,
    expected: u32,
    main_tid: Tid,
}

impl FutexWait {
    pub fn new(process: ProcessRef, addr: usize, expected: u32, main_tid: Tid) -> Self {
        Self {
            process,
            addr,
            expected,
            main_tid,
        }
    }
}

impl Future for FutexWait {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ptr = UserspacePtr::new(self.addr as *const u32);
        let key = (self.main_tid, self.addr);

        // Hold the WAITERS lock across both the value read and waker
        // registration to prevent lost wakeups. Without this, a concurrent
        // futex_wake can fire between the read and registration, finding
        // no waiters — the thread would then sleep forever.
        let mut waiters = WAITERS.lock();

        let current_val = self
            .process
            .with_lock(|p| p.read_userspace_ptr(&ptr))
            .unwrap_or(u32::MAX);

        if current_val != self.expected {
            return Poll::Ready(-(Errno::EAGAIN as i32));
        }

        waiters.entry(key).or_default().push(cx.waker().clone());

        Poll::Pending
    }
}

pub fn futex_wake(main_tid: Tid, addr: usize, count: u32) -> i32 {
    let key = (main_tid, addr);
    let mut waiters = WAITERS.lock();
    let Some(wakers) = waiters.get_mut(&key) else {
        return 0;
    };
    let wake_count = (count as usize).min(wakers.len());
    let to_wake: Vec<Waker> = wakers.drain(..wake_count).collect();
    if wakers.is_empty() {
        waiters.remove(&key);
    }
    drop(waiters);
    for waker in to_wake {
        waker.wake();
    }
    i32::try_from(wake_count).expect("wake count fits in i32")
}
