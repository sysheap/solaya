//! RX-arrival notifier hook: a callback the kernel's network stack installs
//! at boot, which every `NetDevice` driver calls from its `IrqHandler` after
//! acknowledging the device's own interrupt-status register.
//!
//! The driver crate can't import kernel internals, and the network stack
//! can't spawn per-driver wakers from inside an IrqHandler, so we route
//! through a single function pointer stored here. The kernel installs it
//! once at init; drivers call `notify()` from IRQ context.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Holds the `fn()` pointer cast to `usize`. `0` means "no notifier
/// installed yet" — `fn` pointers are never null. `AtomicUsize` is used
/// for lock-free, allocation-free access from IRQ context.
static NOTIFIER: AtomicUsize = AtomicUsize::new(0);

/// Install the RX-arrival callback. Called once by the kernel during net
/// stack init. Subsequent calls overwrite the previous notifier.
pub fn set_notifier(notifier: fn()) {
    NOTIFIER.store(notifier as usize, Ordering::Release);
}

/// Called by driver IRQ handlers after they ack the device's ISR. No-op
/// if no notifier has been installed yet.
pub fn notify() {
    let addr = NOTIFIER.load(Ordering::Acquire);
    if addr == 0 {
        return;
    }
    // SAFETY: `addr` was produced by casting a `fn()` via `set_notifier`.
    // fn pointers round-trip losslessly through `usize` on all supported
    // targets (RISC-V + host for tests). `addr != 0` is checked above.
    let f: fn() = unsafe { core::mem::transmute(addr) };
    f();
}
