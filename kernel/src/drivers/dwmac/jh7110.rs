use crate::{info, klibc::MMIO};

const SYS_CRG_BASE: usize = 0x1302_0000;
const STG_CRG_BASE: usize = 0x1023_0000;
const AON_CRG_BASE: usize = 0x1700_0000;
const SYS_SYSCON_BASE: usize = 0x1303_0000;
const AON_SYSCON_BASE: usize = 0x1701_0000;

const CLK_ENABLE_BIT: u32 = 1 << 31;

// Clock ID boundaries (from JH7110 clkgen specification)
const CLK_SYS_REG_END: u32 = 190;
const CLK_STG_REG_END: u32 = 219;

// Reset register offsets differ per CRG block
const SYSCRG_RESET_ASSERT0: usize = 0x2F8;
const SYSCRG_RESET_STATUS0: usize = 0x308;
const AONCRG_RESET_ASSERT: usize = 0x38;
const AONCRG_RESET_STATUS: usize = 0x3C;
const STGCRG_RESET_ASSERT: usize = 0x74;
const STGCRG_RESET_STATUS: usize = 0x78;

fn enable_clock(clock_id: u32) {
    let (base, offset) = if clock_id < CLK_SYS_REG_END {
        (SYS_CRG_BASE, clock_id as usize * 4)
    } else if clock_id < CLK_STG_REG_END {
        (STG_CRG_BASE, (clock_id - CLK_SYS_REG_END) as usize * 4)
    } else {
        (AON_CRG_BASE, (clock_id - CLK_STG_REG_END) as usize * 4)
    };
    let mut reg = MMIO::<u32>::new(base + offset);
    reg |= CLK_ENABLE_BIT;
}

fn deassert_reset(reset_id: u32) {
    let group = reset_id / 32;
    let bit = reset_id % 32;

    let (assert_addr, status_addr) = match group {
        // SYSCRG groups 0-3: consecutive registers at 0x2F8+
        0..=3 => {
            let idx = group as usize;
            (
                SYS_CRG_BASE + SYSCRG_RESET_ASSERT0 + idx * 4,
                SYS_CRG_BASE + SYSCRG_RESET_STATUS0 + idx * 4,
            )
        }
        // STGCRG group 4
        4 => (
            STG_CRG_BASE + STGCRG_RESET_ASSERT,
            STG_CRG_BASE + STGCRG_RESET_STATUS,
        ),
        // AONCRG group 5
        5 => (
            AON_CRG_BASE + AONCRG_RESET_ASSERT,
            AON_CRG_BASE + AONCRG_RESET_STATUS,
        ),
        _ => panic!("unsupported reset group {}", group),
    };

    let mut assert_reg = MMIO::<u32>::new(assert_addr);
    assert_reg &= !(1u32 << bit);

    let status_reg = MMIO::<u32>::new(status_addr);
    for _ in 0..100_000 {
        if status_reg.read() & (1u32 << bit) != 0 {
            return;
        }
        core::hint::spin_loop();
    }
    info!("DWMAC JH7110: reset {} deassert timed out", reset_id);
}

/// GMAC configuration for the JH7110 SoC.
/// Sets up clocks, resets, and syscon registers for a given GMAC port.
pub fn init_gmac(gmac_index: u8, clock_ids: &[u32], reset_ids: &[u32]) {
    let (syscon_base, syscon_offset, sel_i_shift, sel_i_mask) = match gmac_index {
        0 => (
            AON_SYSCON_BASE,
            0x0Cu32,      // AON_SYSCFG_12
            0x12u32,      // GMAC5_0_SEL_I_SHIFT
            0x1C_0000u32, // GMAC5_0_SEL_I_MASK
        ),
        1 => (
            SYS_SYSCON_BASE,
            0x90u32, // SYS_SYSCON_144
            0x02u32, // GMAC5_1_SEL_I_SHIFT
            0x1Cu32, // GMAC5_1_SEL_I_MASK
        ),
        _ => panic!("JH7110 only has GMAC0 and GMAC1"),
    };

    for &clock_id in clock_ids {
        enable_clock(clock_id);
    }
    info!("DWMAC JH7110: GMAC{} clocks enabled", gmac_index);

    for &reset_id in reset_ids {
        deassert_reset(reset_id);
    }
    info!("DWMAC JH7110: GMAC{} resets deasserted", gmac_index);

    // Configure syscon for RGMII 1000M mode: SEL_I = 1
    let mut syscon_reg = MMIO::<u32>::new(syscon_base + syscon_offset as usize);
    let val = syscon_reg.read();
    let val = (val & !sel_i_mask) | ((1 << sel_i_shift) & sel_i_mask);
    syscon_reg.write(val);

    // Select TX clock source to RGMII
    let (tx_clk_base, tx_clk_offset, tx_clk_bit, tx_clk_mask) = match gmac_index {
        0 => (AON_CRG_BASE, 0x14usize, 0x18u32, 0x100_0000u32),
        1 => (SYS_CRG_BASE, 0x1A4usize, 0x18u32, 0x100_0000u32),
        _ => unreachable!(),
    };
    let mut tx_clk_reg = MMIO::<u32>::new(tx_clk_base + tx_clk_offset);
    let val = tx_clk_reg.read();
    let val = (val & !tx_clk_mask) | ((1 << tx_clk_bit) & tx_clk_mask);
    tx_clk_reg.write(val);

    info!("DWMAC JH7110: GMAC{} init complete", gmac_index);
}
