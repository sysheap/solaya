use crate::{
    info,
    klibc::{MMIO, mmio},
    pci::{GeneralDevicePciHeaderExt, GeneralDevicePciHeaderFields, PCIDevice, PciCpuAddr},
};
use core::sync::atomic::{AtomicUsize, Ordering};
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

pub fn fb_base() -> Option<PciCpuAddr> {
    let addr = FB_BASE.load(Ordering::Relaxed);
    if addr == 0 {
        None
    } else {
        Some(PciCpuAddr::new(addr))
    }
}

pub fn is_bochs_display(device: &PCIDevice) -> bool {
    let cs = device.configuration_space();
    cs.vendor_id().read() == BOCHS_VENDOR_ID && cs.device_id().read() == BOCHS_DEVICE_ID
}

fn write_vbe_reg(dispi_base: usize, index: u16, value: u16) {
    let offset = index as usize * 2;
    let mut reg: MMIO<u16> = MMIO::new(dispi_base + offset);
    reg.write(value);
}

pub fn initialize(mut pci_device: PCIDevice) {
    let bar0 = pci_device.get_or_initialize_bar(0);
    let bar2 = pci_device.get_or_initialize_bar(2);

    let fb_addr = bar0.cpu_address.as_usize();
    let dispi_base = bar2.cpu_address.as_usize() + 0x500;

    write_vbe_reg(dispi_base, VBE_DISPI_INDEX_ENABLE, 0);
    write_vbe_reg(dispi_base, VBE_DISPI_INDEX_XRES, FB_WIDTH as u16);
    write_vbe_reg(dispi_base, VBE_DISPI_INDEX_YRES, FB_HEIGHT as u16);
    write_vbe_reg(dispi_base, VBE_DISPI_INDEX_BPP, FB_BPP as u16);
    write_vbe_reg(
        dispi_base,
        VBE_DISPI_INDEX_ENABLE,
        VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED,
    );

    pci_device
        .configuration_space_mut()
        .set_command_register_bits(
            crate::pci::command_register::MEMORY_SPACE | crate::pci::command_register::BUS_MASTER,
        );

    FB_BASE.store(fb_addr, Ordering::Relaxed);

    info!(
        "bochs-display: framebuffer at {:#x}, {}x{}x{}",
        fb_addr, FB_WIDTH, FB_HEIGHT, FB_BPP
    );
}

pub fn register_devfs_node() {
    use crate::{
        fs::{
            devfs,
            vfs::{NodeType, VfsNode},
        },
        klibc::mmio,
    };
    use alloc::sync::Arc;
    use headers::errno::Errno;

    struct DevFramebuffer {
        ino: u64,
    }

    impl VfsNode for DevFramebuffer {
        fn node_type(&self) -> NodeType {
            NodeType::File
        }
        fn ino(&self) -> u64 {
            self.ino
        }
        fn size(&self) -> usize {
            FB_SIZE
        }
        fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
            let base = fb_base().ok_or(Errno::ENODEV)?;
            let end = offset.saturating_add(buf.len()).min(FB_SIZE);
            if offset >= end {
                return Ok(0);
            }
            let len = end - offset;
            mmio::read_bytes(base.as_usize() + offset, &mut buf[..len]);
            Ok(len)
        }
        fn write(&self, offset: usize, data: &[u8]) -> Result<usize, Errno> {
            let base = fb_base().ok_or(Errno::ENODEV)?;
            let end = offset.saturating_add(data.len()).min(FB_SIZE);
            if offset >= end {
                return Ok(0);
            }
            let len = end - offset;
            mmio::write_bytes(base.as_usize() + offset, &data[..len]);
            arch::cpu::memory_fence();
            Ok(len)
        }
        fn truncate(&self, _length: usize) -> Result<(), Errno> {
            Ok(())
        }
    }

    devfs::register_device(
        "fb0",
        Arc::new(DevFramebuffer {
            ino: devfs::alloc_dev_ino(),
        }),
    );
}
