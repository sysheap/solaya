use crate::klibc::mmio_struct;
use driver_api::BusContext;

pub const VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID: u8 = 0x9;

pub const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
pub const VIRTIO_DEVICE_ID: core::ops::RangeInclusive<u16> = 0x1000..=0x107F;

// Standard PCI config-space offsets.
const PCI_VENDOR_ID_OFFSET: u16 = 0x00;
const PCI_SUBSYSTEM_ID_OFFSET: u16 = 0x2c;

/// Matcher helper: does `bus` describe a virtio PCI device with the given
/// subsystem ID? Reads only the standard PCI config-space fields, not any
/// virtio capability structure — so it's safe to call before device
/// initialization.
pub fn is_virtio_with_subsystem(bus: &dyn BusContext, subsystem_id: u16) -> bool {
    let Some(pci) = bus.as_pci() else {
        return false;
    };
    let vendor_device = pci.read_config_u32(PCI_VENDOR_ID_OFFSET);
    let vendor = (vendor_device & 0xFFFF) as u16;
    let device = ((vendor_device >> 16) & 0xFFFF) as u16;
    let subsys_vendor_subsys = pci.read_config_u32(PCI_SUBSYSTEM_ID_OFFSET);
    let subsys = ((subsys_vendor_subsys >> 16) & 0xFFFF) as u16;
    vendor == VIRTIO_VENDOR_ID && VIRTIO_DEVICE_ID.contains(&device) && subsys == subsystem_id
}

/// Matcher helper for virtio 1.0+ non-transitional devices, which encode the
/// device type in the device ID itself (`0x1040 + type`). Falls back to the
/// legacy subsystem-based match for device IDs below 0x1040.
pub fn is_virtio_modern_or_legacy(bus: &dyn BusContext, subsystem_id: u16) -> bool {
    let Some(pci) = bus.as_pci() else {
        return false;
    };
    let vendor_device = pci.read_config_u32(PCI_VENDOR_ID_OFFSET);
    let vendor = (vendor_device & 0xFFFF) as u16;
    let device = ((vendor_device >> 16) & 0xFFFF) as u16;
    if vendor != VIRTIO_VENDOR_ID || !VIRTIO_DEVICE_ID.contains(&device) {
        return false;
    }
    if device >= 0x1040 {
        return device - 0x1040 == subsystem_id;
    }
    let subsys = ((pci.read_config_u32(PCI_SUBSYSTEM_ID_OFFSET) >> 16) & 0xFFFF) as u16;
    subsys == subsystem_id
}

pub const DEVICE_STATUS_ACKNOWLEDGE: u8 = 1;
pub const DEVICE_STATUS_DRIVER: u8 = 2;
pub const DEVICE_STATUS_DRIVER_OK: u8 = 4;
pub const DEVICE_STATUS_FEATURES_OK: u8 = 8;
pub const DEVICE_STATUS_FAILED: u8 = 128;

pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;

// cfg_type values
/* Common configuration */
pub const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
/* Notifications */
pub const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
/* ISR Status */
pub const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
/* Device specific configuration */
pub const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
/* PCI configuration access */
#[allow(dead_code)]
pub const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;
/* Shared memory region */
#[allow(dead_code)]
pub const VIRTIO_PCI_CAP_SHARED_MEMORY_CFG: u8 = 8;
/* Vendor-specific data */
#[allow(dead_code)]
pub const VIRTIO_PCI_CAP_VENDOR_CFG: u8 = 9;

mmio_struct! {
    #[repr(C, packed)]
    struct virtio_pci_cap {
        cap_vndr: u8,     /* Generic PCI field: PCI_CAP_ID_VNDR */
        cap_next: u8,     /* Generic PCI field: next ptr. */
        cap_len: u8,      /* Generic PCI field: capability length */
        cfg_type: u8,     /* Identifies the structure. */
        bar: u8,          /* Where to find it. */
        id: u8,           /* Multiple capabilities of the same type */
        padding: [u8; 2], /* Pad to full dword. */
        offset: u32,      /* Offset within bar. */
        length: u32,      /* Length of the structure, in bytes. */
    }
}

mmio_struct! {
    #[repr(C)]
    struct virtio_pci_common_cfg {
        device_feature_select: u32,
        device_feature: u32,
        driver_feature_select: u32,
        driver_feature: u32,
        config_msix_vector: u16,
        num_queues: u16,
        device_status: u8,
        config_generation: u8,
        /* About a specific virtqueue. */
        queue_select: u16,
        queue_size: u16,
        queue_msix_vector: u16,
        queue_enable: u16,
        queue_notify_off: u16,
        queue_desc: u64,
        queue_driver: u64,
        queue_device: u64,
    }
}

mmio_struct! {
    #[repr(C)]
    struct virtio_pci_notify_cap {
        cap: crate::drivers::virtio::capability::virtio_pci_cap,
        notify_off_multiplier: u32,
    }
}
