/// Disable interrupts during panic. Safe because during a panic,
/// no further interrupt handling is expected.
pub fn panic_disable_interrupts() {
    // SAFETY: Called during panic — no further interrupt handling needed.
    unsafe {
        arch::cpu::disable_global_interrupts();
    }
}
