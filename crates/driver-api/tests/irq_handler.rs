//! Trait-level sanity test for `IrqHandler`.
//!
//! Runs on host x86_64 via `cargo test -p driver-api`. The PLIC-side
//! registration / `Drop` plumbing lives in the kernel and cannot be
//! exercised off-target; this test just proves the trait is object-safe
//! and that a mock handler can be invoked through `Arc<dyn IrqHandler>`.

extern crate alloc;

use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};

use driver_api::IrqHandler;

struct CountingHandler {
    hits: AtomicUsize,
}

impl IrqHandler for CountingHandler {
    fn handle(&self) {
        self.hits.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn trait_object_dispatch() {
    let handler = Arc::new(CountingHandler {
        hits: AtomicUsize::new(0),
    });
    let dyn_handler: Arc<dyn IrqHandler> = handler.clone();

    dyn_handler.handle();
    dyn_handler.handle();
    dyn_handler.handle();

    assert_eq!(handler.hits.load(Ordering::SeqCst), 3);
}

#[test]
fn multiple_handlers_independent() {
    let a = Arc::new(CountingHandler {
        hits: AtomicUsize::new(0),
    });
    let b = Arc::new(CountingHandler {
        hits: AtomicUsize::new(0),
    });
    let da: Arc<dyn IrqHandler> = a.clone();
    let db: Arc<dyn IrqHandler> = b.clone();

    da.handle();
    db.handle();
    db.handle();

    assert_eq!(a.hits.load(Ordering::SeqCst), 1);
    assert_eq!(b.hits.load(Ordering::SeqCst), 2);
}
