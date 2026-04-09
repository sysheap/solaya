use core::sync::atomic::{AtomicBool, Ordering};

static IN_PANIC: AtomicBool = AtomicBool::new(false);

/// Disable interrupts on the panic path.
///
/// This wrapper exists because the `kernel` crate has
/// `#![forbid(unsafe_code)]` on non-test builds and therefore cannot emit an
/// `unsafe` block itself, even for something as straightforward as disabling
/// interrupts. Keeping this helper in `sys` lets the kernel call a safe fn on
/// the panic path without widening its unsafe footprint.
///
/// Do not inline or remove. The wrapper is load-bearing for the kernel's
/// `forbid(unsafe_code)` invariant.
pub fn panic_disable_interrupts() {
    // SAFETY: Called during panic — no further interrupt handling needed.
    unsafe {
        hal::cpu::disable_global_interrupts();
    }
}

pub fn enter_panic_mode() {
    IN_PANIC.store(true, Ordering::Relaxed);
}

pub fn is_panic_mode() -> bool {
    IN_PANIC.load(Ordering::Relaxed)
}
