use core::{
    any::type_name,
    pin::Pin,
    task::{Context, Poll},
};

use alloc::boxed::Box;

pub struct Task<Output = ()> {
    future: Pin<Box<dyn Future<Output = Output> + Send + 'static>>,
}

impl<Output> core::fmt::Debug for Task<Output> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Task")
            .field_with("future", |f| {
                write!(f, "Future<Output = {}>", type_name::<Output>())
            })
            .finish()
    }
}

impl<Output> Task<Output> {
    pub fn new(future: impl Future<Output = Output> + Send + 'static) -> Self {
        Self {
            future: Box::pin(future),
        }
    }
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Output> {
        self.future.as_mut().poll(cx)
    }
}
