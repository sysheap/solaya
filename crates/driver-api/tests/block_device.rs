//! Trait-level sanity test for `BlockDevice` using a mock implementation.
//!
//! Runs on host x86_64 via `cargo test -p driver-api`. Proves the trait is
//! object-safe and exercises the `Pin<Box<dyn Future>>` shape.

extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec};
use core::{future::Future, pin::Pin};
use std::sync::Mutex;

use driver_api::{BlockDevice, IoError};

struct MockBlock {
    name: alloc::string::String,
    block_size: usize,
    storage: Mutex<alloc::vec::Vec<u8>>,
}

impl MockBlock {
    fn new(name: &str, block_size: usize, num_blocks: u64) -> Self {
        Self {
            name: name.into(),
            block_size,
            storage: Mutex::new(vec![0u8; (num_blocks as usize) * block_size]),
        }
    }
}

impl BlockDevice for MockBlock {
    fn name(&self) -> &str {
        &self.name
    }

    fn num_blocks(&self) -> u64 {
        (self.storage.lock().expect("mock not poisoned").len() / self.block_size) as u64
    }

    fn block_size(&self) -> usize {
        self.block_size
    }

    fn read<'a>(
        &'a self,
        offset_bytes: u64,
        buf: &'a mut [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, IoError>> + Send + 'a>> {
        Box::pin(async move {
            let storage = self.storage.lock().expect("mock not poisoned");
            let off = offset_bytes as usize;
            if off >= storage.len() {
                return Ok(0);
            }
            let n = core::cmp::min(buf.len(), storage.len() - off);
            buf[..n].copy_from_slice(&storage[off..off + n]);
            Ok(n)
        })
    }

    fn write<'a>(
        &'a self,
        offset_bytes: u64,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, IoError>> + Send + 'a>> {
        Box::pin(async move {
            let mut storage = self.storage.lock().expect("mock not poisoned");
            let off = offset_bytes as usize;
            if off >= storage.len() {
                return Ok(0);
            }
            let n = core::cmp::min(data.len(), storage.len() - off);
            storage[off..off + n].copy_from_slice(&data[..n]);
            Ok(n)
        })
    }
}

/// Minimal futures executor so the test doesn't pull in a runtime crate.
fn block_on<F: Future>(fut: F) -> F::Output {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn noop_clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &NOOP_VTABLE)
    }
    fn noop(_: *const ()) {}
    static NOOP_VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);

    let raw = RawWaker::new(core::ptr::null(), &NOOP_VTABLE);
    // SAFETY: vtable functions are no-ops over a null pointer — the waker is inert.
    let waker = unsafe { Waker::from_raw(raw) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

#[test]
fn trait_object_round_trip() {
    let dev: Arc<dyn BlockDevice> = Arc::new(MockBlock::new("mock0", 512, 4));

    assert_eq!(dev.name(), "mock0");
    assert_eq!(dev.block_size(), 512);
    assert_eq!(dev.num_blocks(), 4);

    let payload = b"hello-block-device";
    let written = block_on(dev.write(16, payload)).expect("write");
    assert_eq!(written, payload.len());

    let mut buf = vec![0u8; payload.len()];
    let read = block_on(dev.read(16, &mut buf)).expect("read");
    assert_eq!(read, payload.len());
    assert_eq!(&buf, payload);
}

#[test]
fn short_read_at_end_of_device() {
    let dev: Arc<dyn BlockDevice> = Arc::new(MockBlock::new("mock1", 512, 1));
    let mut buf = vec![0u8; 1024];
    // Device is 512 bytes; reading 1024 at offset 0 gives 512.
    let read = block_on(dev.read(0, &mut buf)).expect("read");
    assert_eq!(read, 512);

    // Offset past end -> 0.
    let read = block_on(dev.read(10_000, &mut buf)).expect("read");
    assert_eq!(read, 0);
}
