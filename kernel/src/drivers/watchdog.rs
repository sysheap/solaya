use crate::klibc::MMIO;

// AON SYSCON software reset register (JH7110 TRM, Table 2-9 / SYSCFG 40)
// Writing 0 to bit 5 (u0_reset_ctrl_rstn_sw) triggers a full chip reset
// from the always-on domain, resetting everything including PAD outputs.
const AON_SYSCON_BASE: usize = 0x1701_0000;
const AON_SYSCFG_40: usize = 0x28;
const AON_SW_RESET_BIT: u32 = 1 << 5;

/// Trigger a full chip reset via the AON SYSCON software reset register.
///
/// Per the JH7110 TRM (Table 2-9), the "Software reset in the always-on domain"
/// resets the whole chip. This is done by clearing bit 5 of AON SYSCONSAIF SYSCFG 40.
pub fn trigger_reset() -> ! {
    crate::println!("[RESET] Triggering full chip reset via AON SYSCON software reset");

    let mut reg = MMIO::<u32>::new(AON_SYSCON_BASE + AON_SYSCFG_40);
    let before = reg.read();
    crate::println!(
        "[RESET] AON SYSCFG 40 ({:#x}) before: {:#010x} (bit5={})",
        AON_SYSCON_BASE + AON_SYSCFG_40,
        before,
        if before & AON_SW_RESET_BIT != 0 { 1 } else { 0 }
    );

    // Clear bit 5 to trigger the reset
    reg &= !AON_SW_RESET_BIT;

    // If we're still here, the reset didn't fire immediately.
    // Read back and report.
    let after = reg.read();
    crate::println!(
        "[RESET] AON SYSCFG 40 after: {:#010x} (bit5={}). Waiting for reset...",
        after,
        if after & AON_SW_RESET_BIT != 0 { 1 } else { 0 }
    );

    let mut iterations: u64 = 0;
    loop {
        iterations += 1;
        if iterations.is_multiple_of(100_000_000) {
            crate::println!(
                "[RESET] Still waiting after {}00M iterations, reg={:#010x}",
                iterations / 100_000_000,
                reg.read()
            );
        }
        core::hint::spin_loop();
    }
}
