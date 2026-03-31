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
const WDOG_CURRENT_COUNT_REG: usize = 0x08;
const WDOG_COUNTER_RESTART_REG: usize = 0x0c;
const WDOG_INT_STATUS_REG: usize = 0x10;

const WDOG_CONTROL_WDT_EN: u32 = 0x01;
const WDOG_COUNTER_RESTART_KICK: u32 = 0x76;

fn enable_clock(clock_id: u32) {
    let addr = SYS_CRG_BASE + clock_id as usize * 4;
    let mut reg = MMIO::<u32>::new(addr);
    reg |= CLK_ENABLE_BIT;
    let readback = reg.read();
    crate::println!(
        "[WDT] enable_clock: id={:#x} addr={:#x} readback={:#010x} (bit31={})",
        clock_id,
        addr,
        readback,
        if readback & CLK_ENABLE_BIT != 0 {
            "set"
        } else {
            "CLEAR"
        }
    );
}

fn deassert_reset(reset_id: u32) {
    let reg_index = reset_id / 32;
    let bit = reset_id % 32;

    let assert_addr = SYS_CRG_BASE + RESET_ASSERT_BASE + reg_index as usize * 4;
    let status_addr = SYS_CRG_BASE + RESET_STATUS_BASE + reg_index as usize * 4;

    let mut assert_reg = MMIO::<u32>::new(assert_addr);
    assert_reg &= !(1u32 << bit);
    crate::println!(
        "[WDT] deassert_reset: id={:#x} assert_addr={:#x} assert_readback={:#010x}",
        reset_id,
        assert_addr,
        assert_reg.read()
    );

    let status_reg = MMIO::<u32>::new(status_addr);
    let mut polls = 0u32;
    while status_reg.read() & (1u32 << bit) == 0 {
        polls += 1;
        if polls.is_multiple_of(1_000_000) {
            crate::println!(
                "[WDT] deassert_reset: id={:#x} still waiting after {} polls, status={:#010x}",
                reset_id,
                polls,
                status_reg.read()
            );
        }
        core::hint::spin_loop();
    }
    crate::println!(
        "[WDT] deassert_reset: id={:#x} done after {} polls, status={:#010x}",
        reset_id,
        polls,
        status_reg.read()
    );
}

/// Trigger a system reset via the DesignWare watchdog timer.
/// Enables clocks and resets, programs minimal timeout, and lets it fire.
pub fn trigger_reset() -> ! {
    crate::println!("[WDT] === Starting watchdog reset sequence ===");

    // Enable watchdog clocks
    enable_clock(WDT_CLK_APB);
    enable_clock(WDT_CLK_CORE);

    // Deassert watchdog resets
    deassert_reset(WDT_RST_APB);
    deassert_reset(WDT_RST_CORE);

    // Read control register before configuring
    let ctrl_before = MMIO::<u32>::new(WDT_BASE + WDOG_CONTROL_REG).read();
    crate::println!("[WDT] CONTROL before config: {:#010x}", ctrl_before);

    // Set minimum timeout (TOP=0 → smallest interval)
    MMIO::<u32>::new(WDT_BASE + WDOG_TIMEOUT_RANGE_REG).write(0);
    let top_readback = MMIO::<u32>::new(WDT_BASE + WDOG_TIMEOUT_RANGE_REG).read();
    crate::println!("[WDT] TIMEOUT_RANGE readback: {:#010x}", top_readback);

    // Kick the counter to load the new timeout
    MMIO::<u32>::new(WDT_BASE + WDOG_COUNTER_RESTART_REG).write(WDOG_COUNTER_RESTART_KICK);
    crate::println!(
        "[WDT] Kicked counter (wrote {:#x})",
        WDOG_COUNTER_RESTART_KICK
    );

    // Read counter value before enabling
    let count_before = MMIO::<u32>::new(WDT_BASE + WDOG_CURRENT_COUNT_REG).read();
    crate::println!("[WDT] CURRENT_COUNT before enable: {:#010x}", count_before);

    // Enable the watchdog in reset mode
    // bit 0 (WDT_EN) = 1: enable watchdog
    // bit 1 (RMOD) = 0: direct system reset (no interrupt-first stage)
    MMIO::<u32>::new(WDT_BASE + WDOG_CONTROL_REG).write(WDOG_CONTROL_WDT_EN);
    let ctrl_after = MMIO::<u32>::new(WDT_BASE + WDOG_CONTROL_REG).read();
    crate::println!(
        "[WDT] CONTROL after enable: {:#010x} (EN={}, RMOD={})",
        ctrl_after,
        if ctrl_after & 0x1 != 0 { "yes" } else { "NO" },
        if ctrl_after & 0x2 != 0 {
            "interrupt-then-reset"
        } else {
            "direct-reset"
        }
    );

    crate::println!("[WDT] Waiting for watchdog to fire...");
    let mut iterations: u64 = 0;
    loop {
        iterations += 1;
        if iterations.is_multiple_of(100_000_000) {
            let count = MMIO::<u32>::new(WDT_BASE + WDOG_CURRENT_COUNT_REG).read();
            let int_status = MMIO::<u32>::new(WDT_BASE + WDOG_INT_STATUS_REG).read();
            let ctrl = MMIO::<u32>::new(WDT_BASE + WDOG_CONTROL_REG).read();
            crate::println!(
                "[WDT] iter={}: COUNT={:#010x} INT_STATUS={:#010x} CTRL={:#010x}",
                iterations / 100_000_000,
                count,
                int_status,
                ctrl
            );
        }
        core::hint::spin_loop();
    }
}
