use crate::klibc::MMIO;

const WDT_BASE: usize = 0x1307_0000;
const SYS_CRG_BASE: usize = 0x1302_0000;

// DW WDT clock IDs (from device tree)
const WDT_CLK_CORE: u32 = 0x7b;
const WDT_CLK_APB: u32 = 0x7a;

// DW WDT reset IDs (from device tree)
const WDT_RST_APB: u32 = 0x6d;
const WDT_RST_CORE: u32 = 0x6e;

// CRG register layout
const RESET_ASSERT_BASE: usize = 0x2F8;
const RESET_STATUS_BASE: usize = 0x308;
const CLK_ENABLE_BIT: u32 = 1 << 31;

// DW WDT registers
const WDOG_CONTROL_REG: usize = 0x00;
const WDOG_TIMEOUT_RANGE_REG: usize = 0x04;
const WDOG_COUNTER_RESTART_REG: usize = 0x0c;

const WDOG_CONTROL_WDT_EN: u32 = 0x01;
const WDOG_COUNTER_RESTART_KICK: u32 = 0x76;

fn enable_clock(clock_id: u32) {
    let mut reg = MMIO::<u32>::new(SYS_CRG_BASE + clock_id as usize * 4);
    reg |= CLK_ENABLE_BIT;
}

fn deassert_reset(reset_id: u32) {
    let reg_index = reset_id / 32;
    let bit = reset_id % 32;

    let mut assert_reg =
        MMIO::<u32>::new(SYS_CRG_BASE + RESET_ASSERT_BASE + reg_index as usize * 4);
    assert_reg &= !(1u32 << bit);

    let status_reg = MMIO::<u32>::new(SYS_CRG_BASE + RESET_STATUS_BASE + reg_index as usize * 4);
    while status_reg.read() & (1u32 << bit) == 0 {
        core::hint::spin_loop();
    }
}

/// Trigger a system reset via the DesignWare watchdog timer.
/// Enables clocks and resets, programs minimal timeout, and lets it fire.
pub fn trigger_reset() -> ! {
    // Enable watchdog clocks
    enable_clock(WDT_CLK_APB);
    enable_clock(WDT_CLK_CORE);

    // Deassert watchdog resets
    deassert_reset(WDT_RST_APB);
    deassert_reset(WDT_RST_CORE);

    // Set minimum timeout (TOP=0 → smallest interval)
    MMIO::<u32>::new(WDT_BASE + WDOG_TIMEOUT_RANGE_REG).write(0);

    // Kick the counter to load the new timeout
    MMIO::<u32>::new(WDT_BASE + WDOG_COUNTER_RESTART_REG).write(WDOG_COUNTER_RESTART_KICK);

    // Enable the watchdog in reset mode (bit 1 = 0 → direct reset, no interrupt first)
    MMIO::<u32>::new(WDT_BASE + WDOG_CONTROL_REG).write(WDOG_CONTROL_WDT_EN);

    loop {
        core::hint::spin_loop();
    }
}
