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
    let mut reg = MMIO::<u32>::new(AON_SYSCON_BASE + AON_SYSCFG_40);
    reg &= !AON_SW_RESET_BIT;

    loop {
        core::hint::spin_loop();
    }
}
