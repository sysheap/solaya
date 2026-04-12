//! RX-arrival notifier hook: a callback the kernel's network stack installs
//! at boot, which every `NetDevice` driver calls from its `IrqHandler` after
//! acknowledging the device's own interrupt-status register.
//!
//! The driver crate can't import kernel internals, and the network stack
//! can't spawn per-driver wakers from inside an IrqHandler, so we route
//! through a single function pointer stored here. The kernel installs it
//! once during init; drivers call `notify()` from IRQ context afterwards.
//!
//! Backed by `klib::runtime_initialized::RuntimeInitializedData<fn()>`,
//! which is lock-free (AtomicBool + UnsafeCell) and therefore IRQ-safe:
//! the read path is a single acquire load plus a function-pointer call.

use klib::runtime_initialized::RuntimeInitializedData;

static NOTIFIER: RuntimeInitializedData<fn()> = RuntimeInitializedData::new();

/// Install the RX-arrival callback. Called once by the kernel during net
/// stack init, before any `NetDevice` driver is registered with the IRQ
/// controller — so `notify()` is guaranteed to see an initialized value.
pub fn set_notifier(notifier: fn()) {
    NOTIFIER.initialize(notifier);
}

/// Called by driver IRQ handlers after they ack the device's ISR.
/// Panics if no notifier has been installed — that would mean a driver
/// fired an RX IRQ before the net stack finished init, which is an
/// ordering bug, not a runtime condition.
pub fn notify() {
    (*NOTIFIER)();
}
