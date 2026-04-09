use common::pid::Tid;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use headers::errno::Errno;

use super::process_table;

pub enum WaitPid {
    Specific(Tid),
    Any,
    Pgid(Tid),
}

pub struct WaitChild {
    parent_main_tid: Tid,
    target: WaitPid,
    wnohang: bool,
    wuntraced: bool,
}

impl WaitChild {
    pub fn new(parent_main_tid: Tid, target: WaitPid, wnohang: bool, wuntraced: bool) -> Self {
        Self {
            parent_main_tid,
            target,
            wnohang,
            wuntraced,
        }
    }
}

impl Future for WaitChild {
    type Output = Result<(Tid, i32), Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        process_table::THE.with_lock(|mut pt| {
            if let Some((tid, status)) = pt.take_zombie(self.parent_main_tid, &self.target) {
                return Poll::Ready(Ok((tid, status)));
            }

            if self.wuntraced
                && let Some((tid, status)) = pt.take_stopped(self.parent_main_tid, &self.target)
            {
                return Poll::Ready(Ok((tid, status)));
            }

            if !pt.has_any_child_of(self.parent_main_tid) {
                return Poll::Ready(Err(Errno::ECHILD));
            }

            if self.wnohang {
                return Poll::Ready(Ok((Tid::new(0), 0)));
            }

            pt.register_wait_waker(cx.waker().clone());
            Poll::Pending
        })
    }
}
