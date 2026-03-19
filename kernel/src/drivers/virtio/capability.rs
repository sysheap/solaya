use crate::mmio_struct;

pub const VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID: u8 = 0x9;

pub const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
pub const VIRTIO_DEVICE_ID: core::ops::RangeInclusive<u16> = 0x1000..=0x107F;

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
