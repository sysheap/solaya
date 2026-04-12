pub mod jh7110;

use alloc::vec::Vec;

use console::{debug, info};
use driver_api::{DmaBuffer, MacAddress};
use hal::{mmio::MMIO, spinlock::Spinlock};

// --- Register offsets within the 64KB MMIO region ---

// MAC registers (base + 0x000)
const MAC_CONFIGURATION: usize = 0x000;
const MAC_PACKET_FILTER: usize = 0x008;
const MAC_Q0_TX_FLOW_CTRL: usize = 0x070;
const MAC_RX_FLOW_CTRL: usize = 0x090;
const MAC_TXQ_PRTY_MAP0: usize = 0x098;
const MAC_RXQ_CTRL0: usize = 0x0A0;
const MAC_RXQ_CTRL1: usize = 0x0A4;
const MAC_RXQ_CTRL2: usize = 0x0A8;
const MAC_HW_FEATURE1: usize = 0x120;
const MAC_MDIO_ADDRESS: usize = 0x200;
const MAC_MDIO_DATA: usize = 0x204;
const MAC_ADDRESS0_HIGH: usize = 0x300;
const MAC_ADDRESS0_LOW: usize = 0x304;

// MAC configuration bits
const MAC_CONFIG_RE: u32 = 1 << 0;
const MAC_CONFIG_TE: u32 = 1 << 1;
const MAC_CONFIG_DM: u32 = 1 << 13;
const MAC_CONFIG_FES: u32 = 1 << 14;
const MAC_CONFIG_PS: u32 = 1 << 15;
const MAC_CONFIG_JE: u32 = 1 << 16;
const MAC_CONFIG_JD: u32 = 1 << 17;
const MAC_CONFIG_WD: u32 = 1 << 19;
const MAC_CONFIG_ACS: u32 = 1 << 20;
const MAC_CONFIG_CST: u32 = 1 << 21;
const MAC_CONFIG_GPSLCE: u32 = 1 << 23;

// MAC RXQ_CTRL0 bits
const RXQ0EN_ENABLED_DCB: u32 = 2;

// MAC flow control
const Q0_TX_FLOW_CTRL_TFE: u32 = 1 << 1;
const RX_FLOW_CTRL_RFE: u32 = 1 << 0;

// MAC HW feature1 fields
const HW_FEATURE1_TXFIFOSIZE_SHIFT: u32 = 6;
const HW_FEATURE1_TXFIFOSIZE_MASK: u32 = 0x1F;
const HW_FEATURE1_RXFIFOSIZE_SHIFT: u32 = 0;
const HW_FEATURE1_RXFIFOSIZE_MASK: u32 = 0x1F;

// MTL registers (base + 0xD00)
const MTL_TXQ0_OPERATION_MODE: usize = 0xD00;
const MTL_TXQ0_QUANTUM_WEIGHT: usize = 0xD18;
const MTL_RXQ0_OPERATION_MODE: usize = 0xD30;

// MTL TXQ0 operation mode bits
const MTL_TXQ0_TSF: u32 = 1 << 1;
const MTL_TXQ0_TXQEN_SHIFT: u32 = 2;
const MTL_TXQ0_TQS_SHIFT: u32 = 16;
const MTL_TXQ0_TQS_MASK: u32 = 0x1FF;

// MTL RXQ0 operation mode bits
const MTL_RXQ0_RSF: u32 = 1 << 5;
const MTL_RXQ0_EHFC: u32 = 1 << 7;
const MTL_RXQ0_RFA_SHIFT: u32 = 8;
const MTL_RXQ0_RFA_MASK: u32 = 0x3F;
const MTL_RXQ0_RFD_SHIFT: u32 = 14;
const MTL_RXQ0_RFD_MASK: u32 = 0x3F;
const MTL_RXQ0_RQS_SHIFT: u32 = 20;
const MTL_RXQ0_RQS_MASK: u32 = 0x3FF;

// DMA registers (base + 0x1000)
const DMA_SYSBUS_MODE: usize = 0x1004;
const DMA_CH0_CONTROL: usize = 0x1100;
const DMA_CH0_TX_CONTROL: usize = 0x1104;
const DMA_CH0_RX_CONTROL: usize = 0x1108;
const DMA_CH0_TXDESC_LIST_HADDR: usize = 0x1110;
const DMA_CH0_TXDESC_LIST_ADDR: usize = 0x1114;
const DMA_CH0_RXDESC_LIST_HADDR: usize = 0x1118;
const DMA_CH0_RXDESC_LIST_ADDR: usize = 0x111C;
const DMA_CH0_TXDESC_TAIL_PTR: usize = 0x1120;
const DMA_CH0_RXDESC_TAIL_PTR: usize = 0x1128;
const DMA_CH0_TXDESC_RING_LENGTH: usize = 0x112C;
const DMA_CH0_RXDESC_RING_LENGTH: usize = 0x1130;
const DMA_CH0_INTERRUPT_ENABLE: usize = 0x1134;
const DMA_CH0_STATUS: usize = 0x1160;

// DMA sysbus mode bits
const DMA_SYSBUS_MODE_EAME: u32 = 1 << 11;
const DMA_SYSBUS_MODE_BLEN16: u32 = 1 << 3;
const DMA_SYSBUS_MODE_BLEN8: u32 = 1 << 2;
const DMA_SYSBUS_MODE_BLEN4: u32 = 1 << 1;

// DMA CH0 control bits
const DMA_CH0_CONTROL_PBLX8: u32 = 1 << 16;
const DMA_CH0_CONTROL_DSL_SHIFT: u32 = 18;

// DMA CH0 TX control bits
const DMA_CH0_TX_CONTROL_OSP: u32 = 1 << 4;
const DMA_CH0_TX_CONTROL_ST: u32 = 1 << 0;
const DMA_CH0_TX_CONTROL_TXPBL_SHIFT: u32 = 16;

// DMA CH0 RX control bits
const DMA_CH0_RX_CONTROL_SR: u32 = 1 << 0;
const DMA_CH0_RX_CONTROL_RBSZ_SHIFT: u32 = 1;
const DMA_CH0_RX_CONTROL_RXPBL_SHIFT: u32 = 16;

// DMA interrupt enable bits
const DMA_CH0_IE_NIE: u32 = 1 << 15; // Normal interrupt summary enable
const DMA_CH0_IE_RIE: u32 = 1 << 6; // Receive interrupt enable
const DMA_CH0_IE_TIE: u32 = 1 << 0; // Transmit interrupt enable

// MDIO address register bits
const MDIO_PA_SHIFT: u32 = 21; // PHY address
const MDIO_RDA_SHIFT: u32 = 16; // Register/device address
const MDIO_CR_SHIFT: u32 = 8; // Clock rate
const MDIO_CR_250_300: u32 = 5; // CSR clock 250-300 MHz (JH7110)
const MDIO_GOC_SHIFT: u32 = 2; // Operation code
const MDIO_GOC_READ: u32 = 3;
const MDIO_GOC_WRITE: u32 = 1;
const MDIO_GB: u32 = 1 << 0; // Go Busy

// Standard PHY registers (IEEE 802.3)
const PHY_BMCR: u32 = 0; // Basic Mode Control
const PHY_BMSR: u32 = 1; // Basic Mode Status
const PHY_BMCR_RESET: u16 = 1 << 15;
const PHY_BMCR_AN_ENABLE: u16 = 1 << 12;
const PHY_BMCR_AN_RESTART: u16 = 1 << 9;
const PHY_BMSR_AN_COMPLETE: u16 = 1 << 5;
const PHY_BMSR_LINK_STATUS: u16 = 1 << 2;

// Motorcomm YT8531 extended register access (via MDIO indirect)
const PHY_EXT_REG_ADDR: u32 = 0x1E;
const PHY_EXT_REG_DATA: u32 = 0x1F;
const YT8531_CHIP_CONFIG: u16 = 0xA001;
const YT8531_RGMII_CONFIG1: u16 = 0xA003;
const YT8531_PAD_DRIVE_STRENGTH: u16 = 0xA010;

// DMA descriptor flags
const DESC3_OWN: u32 = 1 << 31;
const DESC3_IOC: u32 = 1 << 30; // Interrupt on Completion (triggers RI in DMA status)
const DESC3_FD: u32 = 1 << 29;
const DESC3_LD: u32 = 1 << 28;
const DESC3_BUF1V: u32 = 1 << 24;

// Ring sizes and buffer sizes
const TX_RING_SIZE: usize = 16;
const RX_RING_SIZE: usize = 16;
const PACKET_BUF_SIZE: usize = 1600;

#[repr(C, align(64))]
struct DmaDescriptor {
    des0: u32,
    des1: u32,
    des2: u32,
    des3: u32,
}

#[repr(C, align(64))]
struct DescriptorRing<const N: usize> {
    descriptors: [DmaDescriptor; N],
}

#[repr(C, align(64))]
#[derive(Clone)]
struct PacketBuffer([u8; PACKET_BUF_SIZE]);

pub struct DwmacDevice {
    base: usize,
    tx_ring: DmaBuffer,
    rx_ring: DmaBuffer,
    // Each DmaBuffer contains N contiguous PacketBuffer slots.
    tx_buffers: DmaBuffer,
    rx_buffers: DmaBuffer,
    tx_idx: usize,
    rx_idx: usize,
    mac_address: MacAddress,
}

fn read_reg(base: usize, offset: usize) -> u32 {
    MMIO::<u32>::new(base + offset).read()
}

fn write_reg(base: usize, offset: usize, val: u32) {
    MMIO::<u32>::new(base + offset).write(val);
}

fn set_bits(base: usize, offset: usize, bits: u32) {
    let mut reg = MMIO::<u32>::new(base + offset);
    reg |= bits;
}

fn clear_set_bits(base: usize, offset: usize, clear: u32, set: u32) {
    let mut reg = MMIO::<u32>::new(base + offset);
    let val = reg.read();
    reg.write((val & !clear) | set);
}

impl DwmacDevice {
    /// Initialize a DWMAC device at the given MMIO base address.
    /// Clocks and resets must already be enabled before calling this.
    /// Returns None if DMA reset fails (hardware not functional).
    pub fn new(base: usize, mac_address: MacAddress, phy_addr: u32) -> Option<Self> {
        info!("DWMAC: initializing at {:#x}, MAC {}", base, mac_address);

        // Read RXQ_CTRL0 before any changes (debug)
        let rxq_before = read_reg(base, MAC_RXQ_CTRL0);
        info!("DWMAC: RXQ_CTRL0 before init: {:#x}", rxq_before);

        // Stop MAC and DMA before reconfiguring
        clear_set_bits(base, MAC_CONFIGURATION, MAC_CONFIG_TE | MAC_CONFIG_RE, 0);
        clear_set_bits(base, DMA_CH0_TX_CONTROL, DMA_CH0_TX_CONTROL_ST, 0);
        clear_set_bits(base, DMA_CH0_RX_CONTROL, DMA_CH0_RX_CONTROL_SR, 0);

        let mut dev = Self {
            base,
            tx_ring: DmaBuffer::new_coherent(core::mem::size_of::<DescriptorRing<TX_RING_SIZE>>())
                .ok()?,
            rx_ring: DmaBuffer::new_coherent(core::mem::size_of::<DescriptorRing<RX_RING_SIZE>>())
                .ok()?,
            tx_buffers: DmaBuffer::new_coherent(
                TX_RING_SIZE * core::mem::size_of::<PacketBuffer>(),
            )
            .ok()?,
            rx_buffers: DmaBuffer::new_coherent(
                RX_RING_SIZE * core::mem::size_of::<PacketBuffer>(),
            )
            .ok()?,
            tx_idx: 0,
            rx_idx: 0,
            mac_address,
        };

        if !dev.init_hardware(phy_addr) {
            return None;
        }
        Some(dev)
    }

    fn tx_ring_mut(&mut self) -> &mut DescriptorRing<TX_RING_SIZE> {
        // SAFETY: DescriptorRing is `[Descriptor; N]`, and Descriptor is a
        // POD struct of u32 fields (DWMAC normal descriptor layout). All-zero
        // is a valid bit pattern (represents an empty descriptor).
        unsafe { self.tx_ring.as_typed_mut::<DescriptorRing<TX_RING_SIZE>>() }
    }

    fn rx_ring_mut(&mut self) -> &mut DescriptorRing<RX_RING_SIZE> {
        // SAFETY: see `tx_ring_mut`.
        unsafe { self.rx_ring.as_typed_mut::<DescriptorRing<RX_RING_SIZE>>() }
    }

    fn rx_ring_ref(&self) -> &DescriptorRing<RX_RING_SIZE> {
        // SAFETY: see `tx_ring_mut`.
        unsafe { self.rx_ring.as_typed::<DescriptorRing<RX_RING_SIZE>>() }
    }

    fn tx_ring_ref(&self) -> &DescriptorRing<TX_RING_SIZE> {
        // SAFETY: see `tx_ring_mut`.
        unsafe { self.tx_ring.as_typed::<DescriptorRing<TX_RING_SIZE>>() }
    }

    fn tx_buffer_slice_mut(&mut self, i: usize) -> &mut [u8] {
        let start = i * core::mem::size_of::<PacketBuffer>();
        &mut self.tx_buffers.as_mut_slice()[start..start + PACKET_BUF_SIZE]
    }

    fn rx_buffer_slice(&self, i: usize) -> &[u8] {
        let start = i * core::mem::size_of::<PacketBuffer>();
        &self.rx_buffers.as_slice()[start..start + PACKET_BUF_SIZE]
    }

    /// Physical address of TX slot `i`. 32-bit for DWMAC.
    fn tx_buffer_phys_u32(&self, i: usize) -> u32 {
        let base = self.tx_buffers.phys_addr_u32();
        let offset = u32::try_from(i * core::mem::size_of::<PacketBuffer>())
            .expect("TX buffer offset fits in u32");
        base + offset
    }

    fn rx_buffer_phys_u32(&self, i: usize) -> u32 {
        let base = self.rx_buffers.phys_addr_u32();
        let offset = u32::try_from(i * core::mem::size_of::<PacketBuffer>())
            .expect("RX buffer offset fits in u32");
        base + offset
    }

    /// Physical address of TX descriptor `i`. 32-bit.
    fn tx_desc_phys_u32(&self, i: usize) -> u32 {
        let base = self.tx_ring.phys_addr_u32();
        let offset = u32::try_from(i * core::mem::size_of::<DmaDescriptor>())
            .expect("TX descriptor offset fits in u32");
        base + offset
    }

    fn rx_desc_phys_u32(&self, i: usize) -> u32 {
        let base = self.rx_ring.phys_addr_u32();
        let offset = u32::try_from(i * core::mem::size_of::<DmaDescriptor>())
            .expect("RX descriptor offset fits in u32");
        base + offset
    }

    fn init_hardware(&mut self, phy_addr: u32) -> bool {
        if !self.init_phy(phy_addr) {
            return false;
        }
        self.configure_mtl();
        self.configure_mac();
        self.write_mac_address();
        self.configure_dma();
        self.setup_descriptor_rings();
        self.enable_hardware();

        // RXQ_CTRL0 is unwritable on JH7110 DWMAC — the single RX queue
        // appears to be always enabled in hardware.

        info!("DWMAC: initialization complete");
        true
    }

    fn mdio_wait_idle(&self) {
        for _ in 0..100_000 {
            if read_reg(self.base, MAC_MDIO_ADDRESS) & MDIO_GB == 0 {
                return;
            }
            core::hint::spin_loop();
        }
        panic!("mdio_wait_idle not done");
    }

    fn mdio_read(&self, phy_addr: u32, reg: u32) -> u16 {
        self.mdio_wait_idle();
        let val = (phy_addr << MDIO_PA_SHIFT)
            | (reg << MDIO_RDA_SHIFT)
            | (MDIO_CR_250_300 << MDIO_CR_SHIFT)
            | (MDIO_GOC_READ << MDIO_GOC_SHIFT)
            | MDIO_GB;
        write_reg(self.base, MAC_MDIO_ADDRESS, val);
        self.mdio_wait_idle();
        read_reg(self.base, MAC_MDIO_DATA) as u16
    }

    fn mdio_write(&self, phy_addr: u32, reg: u32, data: u16) {
        self.mdio_wait_idle();
        write_reg(self.base, MAC_MDIO_DATA, data as u32);
        let val = (phy_addr << MDIO_PA_SHIFT)
            | (reg << MDIO_RDA_SHIFT)
            | (MDIO_CR_250_300 << MDIO_CR_SHIFT)
            | (MDIO_GOC_WRITE << MDIO_GOC_SHIFT)
            | MDIO_GB;
        write_reg(self.base, MAC_MDIO_ADDRESS, val);
        self.mdio_wait_idle();
    }

    fn phy_read_ext(&self, phy_addr: u32, ext_reg: u16) -> u16 {
        self.mdio_write(phy_addr, PHY_EXT_REG_ADDR, ext_reg);
        self.mdio_read(phy_addr, PHY_EXT_REG_DATA)
    }

    fn phy_write_ext(&self, phy_addr: u32, ext_reg: u16, val: u16) {
        self.mdio_write(phy_addr, PHY_EXT_REG_ADDR, ext_reg);
        self.mdio_write(phy_addr, PHY_EXT_REG_DATA, val);
    }

    /// Configure Motorcomm YT8531 PHY RGMII timing delays and drive strengths.
    /// Values from VisionFive 2 device tree for GMAC1 (ethernet-phy@1).
    fn configure_yt8531_phy(&self, phy_addr: u32) {
        // 0xA001 CHIP_CONFIG: clear rxc_dly_en (bit 8)
        let val = self.phy_read_ext(phy_addr, YT8531_CHIP_CONFIG);
        self.phy_write_ext(phy_addr, YT8531_CHIP_CONFIG, val & !(1 << 8));

        // 0xA010 PAD_DRIVE_STRENGTH:
        //   bits 5-4:   rgmii_sw_dr   = 0x3
        //   bit 12:     rgmii_sw_dr_2 = 0x0
        //   bits 15-13: rgmii_sw_dr_rxc = 0x6
        let mut val = self.phy_read_ext(phy_addr, YT8531_PAD_DRIVE_STRENGTH);
        val = (val & !(0x3 << 4)) | (0x3 << 4); // rgmii_sw_dr
        val &= !(1 << 12); // rgmii_sw_dr_2
        val = (val & !(0x7 << 13)) | (0x6 << 13); // rgmii_sw_dr_rxc
        self.phy_write_ext(phy_addr, YT8531_PAD_DRIVE_STRENGTH, val);

        // 0xA003 RGMII_CONFIG1:
        //   bits 13-10: rx_delay_sel    = 0x2
        //   bits 7-4:   tx_delay_sel_fe = 0x5
        //   bits 3-0:   tx_delay_sel    = 0x0
        //   bit 14:     tx_inverted     = 0x0 (no inversion at 1000M)
        let mut val = self.phy_read_ext(phy_addr, YT8531_RGMII_CONFIG1);
        val = (val & !(0xF << 10)) | (0x2 << 10); // rx_delay_sel
        val = (val & !(0xF << 4)) | (0x5 << 4); // tx_delay_sel_fe
        val &= !(0xF); // tx_delay_sel = 0
        val &= !(1 << 14); // tx_inverted = 0
        self.phy_write_ext(phy_addr, YT8531_RGMII_CONFIG1, val);

        info!("DWMAC: YT8531 PHY RGMII delays configured");
    }

    fn init_phy(&self, phy_addr: u32) -> bool {
        // Reset PHY and wait for the PHY to self-clear BMCR.RESET.
        self.mdio_write(phy_addr, PHY_BMCR, PHY_BMCR_RESET);
        let mut reset_done = false;
        for _ in 0..100_000 {
            if self.mdio_read(phy_addr, PHY_BMCR) & PHY_BMCR_RESET == 0 {
                reset_done = true;
                break;
            }
            core::hint::spin_loop();
        }
        assert!(reset_done, "DWMAC: PHY {phy_addr} did not clear BMCR.RESET");

        self.configure_yt8531_phy(phy_addr);

        // Enable and restart auto-negotiation
        self.mdio_write(phy_addr, PHY_BMCR, PHY_BMCR_AN_ENABLE | PHY_BMCR_AN_RESTART);

        // Wait for auto-negotiation to complete
        for i in 0..1_000_000 {
            let bmsr = self.mdio_read(phy_addr, PHY_BMSR);
            if bmsr & PHY_BMSR_AN_COMPLETE != 0 {
                if bmsr & PHY_BMSR_LINK_STATUS != 0 {
                    info!("DWMAC: PHY auto-negotiation complete, link up");
                    return true;
                }
                info!("DWMAC: PHY auto-negotiation complete but no link, skipping");
                return false;
            }
            if i % 200_000 == 0 && i > 0 {
                info!("DWMAC: waiting for PHY auto-negotiation...");
            }
            core::hint::spin_loop();
        }
        info!("DWMAC: PHY auto-negotiation timed out, skipping");
        false
    }

    fn configure_mtl(&self) {
        // Enable Store-and-Forward for TX, enable TX queue
        set_bits(
            self.base,
            MTL_TXQ0_OPERATION_MODE,
            MTL_TXQ0_TSF | (2 << MTL_TXQ0_TXQEN_SHIFT),
        );

        // TX queue weight
        write_reg(self.base, MTL_TXQ0_QUANTUM_WEIGHT, 0x10);

        // Enable Store-and-Forward for RX
        set_bits(self.base, MTL_RXQ0_OPERATION_MODE, MTL_RXQ0_RSF);

        // Read FIFO sizes from hardware feature register
        let hw_feat1 = read_reg(self.base, MAC_HW_FEATURE1);
        let tx_fifo_sz = (hw_feat1 >> HW_FEATURE1_TXFIFOSIZE_SHIFT) & HW_FEATURE1_TXFIFOSIZE_MASK;
        let rx_fifo_sz = (hw_feat1 >> HW_FEATURE1_RXFIFOSIZE_SHIFT) & HW_FEATURE1_RXFIFOSIZE_MASK;

        // fifo_sz is encoded as log2(n / 128). Queue size is (n / 256) - 1.
        let tqs = (128 << tx_fifo_sz) / 256 - 1;
        let rqs = (128 << rx_fifo_sz) / 256 - 1;

        debug!(
            "DWMAC: TX FIFO {}KB, RX FIFO {}KB",
            (128 << tx_fifo_sz) / 1024,
            (128 << rx_fifo_sz) / 1024
        );

        clear_set_bits(
            self.base,
            MTL_TXQ0_OPERATION_MODE,
            MTL_TXQ0_TQS_MASK << MTL_TXQ0_TQS_SHIFT,
            tqs << MTL_TXQ0_TQS_SHIFT,
        );
        clear_set_bits(
            self.base,
            MTL_RXQ0_OPERATION_MODE,
            MTL_RXQ0_RQS_MASK << MTL_RXQ0_RQS_SHIFT,
            rqs << MTL_RXQ0_RQS_SHIFT,
        );

        // Flow control if FIFO >= 4KB
        if rqs >= (4096 / 256 - 1) {
            set_bits(self.base, MTL_RXQ0_OPERATION_MODE, MTL_RXQ0_EHFC);
            let (rfd, rfa) = if rqs == (4096 / 256 - 1) {
                (0x3u32, 0x1u32) // Full-3K, Full-1.5K
            } else if rqs == (8192 / 256 - 1) {
                (0x6, 0xa) // Full-4K, Full-6K
            } else {
                (0x6, 0x1E) // Full-4K, Full-16K
            };
            clear_set_bits(
                self.base,
                MTL_RXQ0_OPERATION_MODE,
                (MTL_RXQ0_RFD_MASK << MTL_RXQ0_RFD_SHIFT)
                    | (MTL_RXQ0_RFA_MASK << MTL_RXQ0_RFA_SHIFT),
                (rfd << MTL_RXQ0_RFD_SHIFT) | (rfa << MTL_RXQ0_RFA_SHIFT),
            );
        }
    }

    fn configure_mac(&self) {
        // RX queue enable (DCB mode) — write may be ignored on some JH7110 variants
        clear_set_bits(self.base, MAC_RXQ_CTRL0, 0x3, RXQ0EN_ENABLED_DCB);

        // Multicast and Broadcast Queue Enable
        set_bits(self.base, MAC_RXQ_CTRL1, 0x0010_0000);

        // Enable promiscuous mode
        set_bits(self.base, MAC_PACKET_FILTER, 0x1);

        // TX flow control: set pause time and enable
        set_bits(self.base, MAC_Q0_TX_FLOW_CTRL, 0xFFFF_0000);
        // Clear TX queue priority
        clear_set_bits(self.base, MAC_TXQ_PRTY_MAP0, 0xFF, 0);
        // Clear RX queue priority
        clear_set_bits(self.base, MAC_RXQ_CTRL2, 0xFF, 0);
        // Enable TX and RX flow control
        set_bits(self.base, MAC_Q0_TX_FLOW_CTRL, Q0_TX_FLOW_CTRL_TFE);
        set_bits(self.base, MAC_RX_FLOW_CTRL, RX_FLOW_CTRL_RFE);

        // MAC configuration: strip CRC, auto pad strip, clear watchdog/jabber/jumbo
        clear_set_bits(
            self.base,
            MAC_CONFIGURATION,
            MAC_CONFIG_GPSLCE | MAC_CONFIG_WD | MAC_CONFIG_JD | MAC_CONFIG_JE,
            MAC_CONFIG_CST | MAC_CONFIG_ACS,
        );

        // Set speed to 1000M full-duplex (GMII mode: clear PS and FES, set DM)
        clear_set_bits(
            self.base,
            MAC_CONFIGURATION,
            MAC_CONFIG_PS | MAC_CONFIG_FES,
            MAC_CONFIG_DM,
        );
    }

    fn write_mac_address(&self) {
        let mac = self.mac_address.as_bytes();
        let low =
            (mac[3] as u32) << 24 | (mac[2] as u32) << 16 | (mac[1] as u32) << 8 | (mac[0] as u32);
        let high = (mac[5] as u32) << 8 | (mac[4] as u32);
        write_reg(self.base, MAC_ADDRESS0_LOW, low);
        write_reg(self.base, MAC_ADDRESS0_HIGH, high);
    }

    fn configure_dma(&self) {
        // Enable OSP (Operate on Second Packet) for TX
        set_bits(self.base, DMA_CH0_TX_CONTROL, DMA_CH0_TX_CONTROL_OSP);

        // RX buffer size (must be multiple of bus width)
        clear_set_bits(
            self.base,
            DMA_CH0_RX_CONTROL,
            0x3FFF << DMA_CH0_RX_CONTROL_RBSZ_SHIFT,
            (PACKET_BUF_SIZE as u32) << DMA_CH0_RX_CONTROL_RBSZ_SHIFT,
        );

        // Descriptor skip length: our descriptors are 64-byte aligned (padded from 16 bytes)
        // DSL = (desc_size - 16) / bus_width = (64 - 16) / 8 = 6
        let desc_pad = (64 - 16) / 8;
        set_bits(
            self.base,
            DMA_CH0_CONTROL,
            DMA_CH0_CONTROL_PBLX8 | (desc_pad << DMA_CH0_CONTROL_DSL_SHIFT),
        );

        // TX programmable burst length
        clear_set_bits(
            self.base,
            DMA_CH0_TX_CONTROL,
            0x3F << DMA_CH0_TX_CONTROL_TXPBL_SHIFT,
            16 << DMA_CH0_TX_CONTROL_TXPBL_SHIFT,
        );

        // RX programmable burst length
        clear_set_bits(
            self.base,
            DMA_CH0_RX_CONTROL,
            0x3F << DMA_CH0_RX_CONTROL_RXPBL_SHIFT,
            8 << DMA_CH0_RX_CONTROL_RXPBL_SHIFT,
        );

        // DMA system bus mode: AXI burst lengths + enhanced address mode
        write_reg(
            self.base,
            DMA_SYSBUS_MODE,
            (2 << 16) // rd_osr_lmt
                | DMA_SYSBUS_MODE_EAME
                | DMA_SYSBUS_MODE_BLEN16
                | DMA_SYSBUS_MODE_BLEN8
                | DMA_SYSBUS_MODE_BLEN4,
        );
    }

    fn setup_descriptor_rings(&mut self) {
        // Initialize RX descriptors with buffer addresses
        for i in 0..RX_RING_SIZE {
            let buf_phys = self.rx_buffer_phys_u32(i);
            let desc = &mut self.rx_ring_mut().descriptors[i];
            desc.des0 = buf_phys;
            desc.des1 = 0;
            desc.des2 = 0;
            desc.des3 = DESC3_OWN | DESC3_IOC | DESC3_BUF1V;
        }

        // TX descriptors start empty
        for desc in &mut self.tx_ring_mut().descriptors {
            desc.des0 = 0;
            desc.des1 = 0;
            desc.des2 = 0;
            desc.des3 = 0;
        }

        // Flush descriptor rings from CPU cache to RAM so DMA sees them
        let rx_base_virt = self.rx_ring_ref().descriptors.as_ptr() as usize;
        hal::cache::flush_range(
            rx_base_virt,
            RX_RING_SIZE * core::mem::size_of::<DmaDescriptor>(),
        );
        let tx_base_virt = self.tx_ring_ref().descriptors.as_ptr() as usize;
        hal::cache::flush_range(
            tx_base_virt,
            TX_RING_SIZE * core::mem::size_of::<DmaDescriptor>(),
        );

        let tx_base_phys = self.tx_ring.phys_addr_u32();
        let rx_base_phys = self.rx_ring.phys_addr_u32();

        // TX descriptor list
        write_reg(self.base, DMA_CH0_TXDESC_LIST_HADDR, 0);
        write_reg(self.base, DMA_CH0_TXDESC_LIST_ADDR, tx_base_phys);
        write_reg(
            self.base,
            DMA_CH0_TXDESC_RING_LENGTH,
            (TX_RING_SIZE - 1) as u32,
        );

        // RX descriptor list
        write_reg(self.base, DMA_CH0_RXDESC_LIST_HADDR, 0);
        write_reg(self.base, DMA_CH0_RXDESC_LIST_ADDR, rx_base_phys);
        write_reg(
            self.base,
            DMA_CH0_RXDESC_RING_LENGTH,
            (RX_RING_SIZE - 1) as u32,
        );

        // Tail pointer must point past the last descriptor to make all available
        let end_of_ring = rx_base_phys
            + u32::try_from(RX_RING_SIZE * core::mem::size_of::<DmaDescriptor>())
                .expect("RX ring size fits in u32");
        write_reg(self.base, DMA_CH0_RXDESC_TAIL_PTR, end_of_ring);

        debug!(
            "DWMAC: RX ring at {:#x}, tail at {:#x}, buf[0] at {:#x}",
            rx_base_phys,
            end_of_ring,
            self.rx_buffers.phys_addr()
        );
    }

    fn enable_hardware(&self) {
        // Enable DMA interrupts (normal summary + RX + TX)
        write_reg(
            self.base,
            DMA_CH0_INTERRUPT_ENABLE,
            DMA_CH0_IE_NIE | DMA_CH0_IE_RIE | DMA_CH0_IE_TIE,
        );

        // Start DMA TX and RX
        set_bits(self.base, DMA_CH0_TX_CONTROL, DMA_CH0_TX_CONTROL_ST);
        set_bits(self.base, DMA_CH0_RX_CONTROL, DMA_CH0_RX_CONTROL_SR);

        // Enable MAC TX and RX
        set_bits(self.base, MAC_CONFIGURATION, MAC_CONFIG_TE | MAC_CONFIG_RE);
    }

    /// Returns the MMIO address of the DMA CH0 status register.
    /// Reading this register acknowledges (write-to-clear) interrupt flags.
    pub fn isr_status_mmio(&self) -> MMIO<u32> {
        MMIO::new(self.base + DMA_CH0_STATUS)
    }
}

impl DwmacDevice {
    fn receive_packets(&mut self) -> Vec<Vec<u8>> {
        let mut received = Vec::new();

        loop {
            let rx_idx = self.rx_idx;

            // Invalidate descriptor so CPU reads DMA's writes from RAM
            let desc_virt =
                &self.rx_ring_ref().descriptors[rx_idx] as *const DmaDescriptor as usize;
            hal::cache::flush_range(desc_virt, core::mem::size_of::<DmaDescriptor>());

            let des3 = MMIO::<u32>::new(
                &self.rx_ring_ref().descriptors[rx_idx].des3 as *const u32 as usize,
            )
            .read();

            if des3 & DESC3_OWN != 0 {
                break;
            }

            let length = (des3 & 0x7FFF) as usize;
            if length > 0 && length <= PACKET_BUF_SIZE {
                // Invalidate RX buffer so CPU reads DMA-written packet data
                let buf_virt = self.rx_buffer_slice(rx_idx).as_ptr() as usize;
                hal::cache::flush_range(buf_virt, length);

                let data = self.rx_buffer_slice(rx_idx)[..length].to_vec();
                received.push(data);
            }

            // Re-arm descriptor — compute phys addr before taking &mut borrow.
            let buf_phys = self.rx_buffer_phys_u32(rx_idx);
            let desc_phys = self.rx_desc_phys_u32(rx_idx);
            let desc = &mut self.rx_ring_mut().descriptors[rx_idx];
            desc.des0 = buf_phys;
            desc.des1 = 0;
            desc.des2 = 0;
            desc.des3 = DESC3_OWN | DESC3_IOC | DESC3_BUF1V;

            // Flush descriptor to RAM so DMA sees OWN bit
            let desc_virt = desc as *const _ as usize;
            hal::cache::flush_range(desc_virt, core::mem::size_of::<DmaDescriptor>());

            // Update tail pointer
            write_reg(self.base, DMA_CH0_RXDESC_TAIL_PTR, desc_phys);

            self.rx_idx = (rx_idx + 1) % RX_RING_SIZE;
        }

        received
    }

    fn send_packet(&mut self, data: Vec<u8>) {
        let length = data.len().min(PACKET_BUF_SIZE);
        let tx_idx = self.tx_idx;

        // Wait for descriptor to be available (OWN cleared by hardware)
        let mut found = false;
        for _ in 0..1_000_000 {
            let desc_virt =
                &self.tx_ring_ref().descriptors[tx_idx] as *const DmaDescriptor as usize;
            hal::cache::flush_range(desc_virt, core::mem::size_of::<DmaDescriptor>());
            let des3 = MMIO::<u32>::new(
                &self.tx_ring_ref().descriptors[tx_idx].des3 as *const u32 as usize,
            )
            .read();
            if des3 & DESC3_OWN == 0 {
                found = true;
                break;
            }
            core::hint::spin_loop();
        }
        assert!(found, "DWMAC: TX descriptor timeout, no free descriptor");

        // Copy data to TX buffer
        self.tx_buffer_slice_mut(tx_idx)[..length].copy_from_slice(&data[..length]);

        // Flush TX buffer to RAM so DMA reads actual packet data
        let buf_virt = self.tx_buffer_slice_mut(tx_idx).as_ptr() as usize;
        hal::cache::flush_range(buf_virt, length);

        let buf_phys = self.tx_buffer_phys_u32(tx_idx);
        let desc = &mut self.tx_ring_mut().descriptors[tx_idx];
        desc.des0 = buf_phys;
        desc.des1 = 0;
        desc.des2 = length as u32;
        desc.des3 = DESC3_OWN | DESC3_FD | DESC3_LD | length as u32;

        // Flush descriptor to RAM so DMA sees OWN bit and buffer address
        let desc_virt = desc as *const _ as usize;
        hal::cache::flush_range(desc_virt, core::mem::size_of::<DmaDescriptor>());

        // Advance TX index and write tail pointer to trigger DMA
        self.tx_idx = (tx_idx + 1) % TX_RING_SIZE;
        let next_desc_phys = self.tx_desc_phys_u32(self.tx_idx);
        write_reg(self.base, DMA_CH0_TXDESC_TAIL_PTR, next_desc_phys);
    }
}

/// `driver_api::NetDevice` adapter for the DWMAC driver. Holds the
/// underlying device behind a `Spinlock` so the trait's `&self` methods
/// can mutate the ring indices.
pub struct DwmacHandle {
    inner: Spinlock<DwmacDevice>,
    mac: MacAddress,
    name: alloc::string::String,
    isr_status: MMIO<u32>,
    irq: Spinlock<Option<driver_api::IrqRegistration>>,
}

impl DwmacHandle {
    pub fn new(device: DwmacDevice) -> Self {
        let mac = device.mac_address;
        let isr_status = device.isr_status_mmio();
        Self {
            inner: Spinlock::new(device),
            mac,
            name: alloc::string::String::from("eth0"),
            isr_status,
            irq: Spinlock::new(None),
        }
    }

    pub fn set_irq_registration(&self, registration: driver_api::IrqRegistration) {
        *self.irq.lock() = Some(registration);
    }
}

impl driver_api::IrqHandler for DwmacHandle {
    fn handle(&self) {
        // DWMAC4 DMA CH0 status is write-1-to-clear.
        let mut isr = MMIO::<u32>::new(self.isr_status.addr());
        let status = isr.read();
        isr.write(status);
        driver_api::net_notifier::notify();
    }
}

impl driver_api::NetDevice for DwmacHandle {
    fn name(&self) -> &str {
        &self.name
    }

    fn mac(&self) -> MacAddress {
        self.mac
    }

    fn mtu(&self) -> u16 {
        1500
    }

    fn send(&self, frame: Vec<u8>) {
        self.inner.lock().send_packet(frame);
    }

    fn receive(&self) -> Vec<Vec<u8>> {
        self.inner.lock().receive_packets()
    }
}
