use crate::{
    info,
    klibc::{MMIO, mmio},
};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};
use driver_api::{BarIndex, BusContext, DisplayDevice, FramebufferInfo, IoError, bus::pci_command};
use headers::errno::Errno;
#[allow(unused_imports)]
use mmio::write_bytes;

const VBE_DISPI_INDEX_XRES: u16 = 1;
const VBE_DISPI_INDEX_YRES: u16 = 2;
const VBE_DISPI_INDEX_BPP: u16 = 3;
const VBE_DISPI_INDEX_ENABLE: u16 = 4;

const VBE_DISPI_ENABLED: u16 = 0x01;
const VBE_DISPI_LFB_ENABLED: u16 = 0x40;

const BOCHS_VENDOR_ID: u16 = 0x1234;
const BOCHS_DEVICE_ID: u16 = 0x1111;

pub const FB_WIDTH: usize = 640;
pub const FB_HEIGHT: usize = 480;
pub const FB_BPP: usize = 32;
pub const FB_STRIDE: usize = FB_WIDTH * (FB_BPP / 8);
pub const FB_SIZE: usize = FB_STRIDE * FB_HEIGHT;

static FB_BASE: AtomicUsize = AtomicUsize::new(0);

pub fn fb_base() -> Option<usize> {
    let addr = FB_BASE.load(Ordering::Relaxed);
    if addr == 0 { None } else { Some(addr) }
}

pub fn is_bochs_display(bus: &dyn BusContext) -> bool {
    let Some(pci) = bus.as_pci() else {
        return false;
    };
    let vendor_device = pci.read_config_u32(0);
    let vendor = (vendor_device & 0xFFFF) as u16;
    let device = ((vendor_device >> 16) & 0xFFFF) as u16;
    vendor == BOCHS_VENDOR_ID && device == BOCHS_DEVICE_ID
}

fn write_vbe_reg(dispi_base: usize, index: u16, value: u16) {
    let offset = index as usize * 2;
    let mut reg: MMIO<u16> = MMIO::new(dispi_base + offset);
    reg.write(value);
}

pub fn initialize(bus: &dyn BusContext) -> Arc<dyn DisplayDevice> {
    let pci = bus.as_pci().expect("bochs-display requires a PCI bus");
    let bar0 = pci.map_bar(BarIndex(0)).expect("map bar0");
    let bar2 = pci.map_bar(BarIndex(2)).expect("map bar2");

    let fb_addr = bar0.virt_base;
    let dispi_base = bar2.virt_base + 0x500;

    write_vbe_reg(dispi_base, VBE_DISPI_INDEX_ENABLE, 0);
    write_vbe_reg(dispi_base, VBE_DISPI_INDEX_XRES, FB_WIDTH as u16);
    write_vbe_reg(dispi_base, VBE_DISPI_INDEX_YRES, FB_HEIGHT as u16);
    write_vbe_reg(dispi_base, VBE_DISPI_INDEX_BPP, FB_BPP as u16);
    write_vbe_reg(
        dispi_base,
        VBE_DISPI_INDEX_ENABLE,
        VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED,
    );

    pci.set_command_bits(pci_command::MEMORY_SPACE | pci_command::BUS_MASTER);

    FB_BASE.store(fb_addr, Ordering::Relaxed);

    info!(
        "bochs-display: framebuffer at {:#x}, {}x{}x{}",
        fb_addr, FB_WIDTH, FB_HEIGHT, FB_BPP
    );

    Arc::new(BochsDisplay {
        phys_addr: fb_addr as u64,
    })
}

struct BochsDisplay {
    phys_addr: u64,
}

impl DisplayDevice for BochsDisplay {
    fn name(&self) -> &str {
        "fb0"
    }

    fn framebuffer(&self) -> FramebufferInfo {
        FramebufferInfo {
            width: FB_WIDTH as u32,
            height: FB_HEIGHT as u32,
            stride: FB_STRIDE as u32,
            bpp: FB_BPP as u8,
            phys_addr: self.phys_addr,
        }
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, IoError> {
        let base = fb_base().ok_or(Errno::ENODEV)?;
        let end = offset.saturating_add(buf.len()).min(FB_SIZE);
        if offset >= end {
            return Ok(0);
        }
        let len = end - offset;
        mmio::read_bytes(base + offset, &mut buf[..len]);
        Ok(len)
    }

    fn write_at(&self, offset: usize, data: &[u8]) -> Result<usize, IoError> {
        let base = fb_base().ok_or(Errno::ENODEV)?;
        let end = offset.saturating_add(data.len()).min(FB_SIZE);
        if offset >= end {
            return Ok(0);
        }
        let len = end - offset;
        mmio::write_bytes(base + offset, &data[..len]);
        hal::cpu::memory_fence();
        Ok(len)
    }
}
