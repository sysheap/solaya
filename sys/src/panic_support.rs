use core::sync::atomic::{AtomicBool, Ordering};

static IN_PANIC: AtomicBool = AtomicBool::new(false);

/// Disable interrupts during panic. Safe because during a panic,
/// no further interrupt handling is expected.
pub fn panic_disable_interrupts() {
    // SAFETY: Called during panic — no further interrupt handling needed.
    unsafe {
        arch::cpu::disable_global_interrupts();
    }
}

pub fn enter_panic_mode() {
    IN_PANIC.store(true, Ordering::Relaxed);
}

pub fn is_panic_mode() -> bool {
    IN_PANIC.load(Ordering::Relaxed)
}
