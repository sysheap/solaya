use crate::klibc::MMIO;

const SYS_CRG_BASE: usize = 0x1302_0000;
const AON_CRG_BASE: usize = 0x1700_0000;
const SYS_SYSCON_BASE: usize = 0x1303_0000;
const AON_SYSCON_BASE: usize = 0x1701_0000;

const RESET_ASSERT_BASE: usize = 0x2F8;
const RESET_STATUS_BASE: usize = 0x308;
const CLK_ENABLE_BIT: u32 = 1 << 31;

pub struct Crg {
    base: usize,
}

impl Crg {
    fn new(base: usize) -> Self {
        Self { base }
    }

    pub fn enable_clock(&self, clock_id: u32) {
        let reg_offset = clock_id as usize * 4;
        let mut reg = MMIO::<u32>::new(self.base + reg_offset);
        reg |= CLK_ENABLE_BIT;
    }

    pub fn deassert_reset(&self, reset_id: u32) {
        let reg_index = reset_id / 32;
        let bit = reset_id % 32;
        let assert_offset = RESET_ASSERT_BASE + reg_index as usize * 4;
        let status_offset = RESET_STATUS_BASE + reg_index as usize * 4;

        let mut assert_reg = MMIO::<u32>::new(self.base + assert_offset);
        assert_reg &= !(1u32 << bit);

        // Poll status register until reset is deasserted (bit set = deasserted)
        let status_reg = MMIO::<u32>::new(self.base + status_offset);
        while status_reg.read() & (1u32 << bit) == 0 {
            core::hint::spin_loop();
        }
    }
}

/// GMAC configuration for the JH7110 SoC.
/// Sets up clocks, resets, and syscon registers for a given GMAC port.
pub fn init_gmac(gmac_index: u8, clock_ids: &[u32], reset_ids: &[u32]) {
    let (crg_base, syscon_base, syscon_offset, sel_i_shift, sel_i_mask) = match gmac_index {
        0 => (
            AON_CRG_BASE,
            AON_SYSCON_BASE,
            0x0Cu32,      // AON_SYSCFG_12
            0x12u32,      // GMAC5_0_SEL_I_SHIFT
            0x1C_0000u32, // GMAC5_0_SEL_I_MASK
        ),
        1 => (
            SYS_CRG_BASE,
            SYS_SYSCON_BASE,
            0x90u32, // SYS_SYSCON_144
            0x02u32, // GMAC5_1_SEL_I_SHIFT
            0x1Cu32, // GMAC5_1_SEL_I_MASK
        ),
        _ => panic!("JH7110 only has GMAC0 and GMAC1"),
    };

    // Enable all GMAC clocks
    let crg = Crg::new(crg_base);
    for &clock_id in clock_ids {
        crg.enable_clock(clock_id);
    }

    // Deassert all GMAC resets
    let reset_crg = Crg::new(SYS_CRG_BASE);
    for &reset_id in reset_ids {
        reset_crg.deassert_reset(reset_id);
    }

    // Configure syscon for RGMII 1000M mode: SEL_I = 1
    let mut syscon_reg = MMIO::<u32>::new(syscon_base + syscon_offset as usize);
    let val = syscon_reg.read();
    let val = (val & !sel_i_mask) | ((1 << sel_i_shift) & sel_i_mask);
    syscon_reg.write(val);

    // Select TX clock to RGMII
    let (tx_clk_offset, tx_clk_bit, tx_clk_mask) = match gmac_index {
        0 => (0x14usize, 0x18u32, 0x100_0000u32),  // GMAC5_0_CLK_TX
        1 => (0x1A4usize, 0x18u32, 0x100_0000u32), // GMAC5_1_CLK_TX
        _ => unreachable!(),
    };
    let mut tx_clk_reg = MMIO::<u32>::new(crg_base + tx_clk_offset);
    let val = tx_clk_reg.read();
    let val = (val & !tx_clk_mask) | ((1 << tx_clk_bit) & tx_clk_mask);
    tx_clk_reg.write(val);
}
