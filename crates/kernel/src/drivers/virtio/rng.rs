use alloc::vec;

use crate::{
    drivers::virtio::{
        capability::{
            DEVICE_STATUS_ACKNOWLEDGE, DEVICE_STATUS_DRIVER, DEVICE_STATUS_DRIVER_OK,
            DEVICE_STATUS_FAILED, DEVICE_STATUS_FEATURES_OK, VIRTIO_DEVICE_ID, VIRTIO_F_VERSION_1,
            VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_NOTIFY_CFG, VIRTIO_VENDOR_ID,
            VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID, virtio_pci_cap, virtio_pci_capFields,
            virtio_pci_common_cfg, virtio_pci_common_cfgFields, virtio_pci_notify_cap,
            virtio_pci_notify_capFields,
        },
        virtqueue::{BufferDirection, VirtQueue},
    },
    info,
    klibc::{MMIO, Spinlock, util::is_power_of_2_or_zero},
    pci::{
        GeneralDevicePciHeaderExt, GeneralDevicePciHeaderFields, PCIDevice, PciCapabilityFields,
    },
};

const QUEUE_SIZE: usize = 0x10;
const VIRTIO_RNG_SUBSYSTEM_ID: u16 = 4;

pub struct RngDevice {
    common_cfg: MMIO<virtio_pci_common_cfg>,
    request_queue: VirtQueue<QUEUE_SIZE>,
}

static RNG_DEVICE: Spinlock<Option<RngDevice>> = Spinlock::new(None);

pub fn set_device(device: RngDevice) {
    *RNG_DEVICE.lock() = Some(device);
}

pub fn read_random(buf: &mut [u8]) {
    let mut guard = RNG_DEVICE.lock();
    let dev = guard.as_mut().expect("RNG device not initialized");
    dev.read(buf);
}

pub fn is_available() -> bool {
    RNG_DEVICE.lock().is_some()
}

pub fn register_devfs_node() {
    use crate::fs::{
        devfs,
        vfs::{NodeType, VfsNode},
    };
    use alloc::sync::Arc;
    use headers::errno::Errno;

    struct DevRandom {
        ino: u64,
    }

    impl VfsNode for DevRandom {
        fn node_type(&self) -> NodeType {
            NodeType::File
        }
        fn ino(&self) -> u64 {
            self.ino
        }
        fn size(&self) -> usize {
            0
        }
        fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
            read_random(buf);
            Ok(buf.len())
        }
        fn write(&self, _offset: usize, data: &[u8]) -> Result<usize, Errno> {
            Ok(data.len())
        }
        fn truncate(&self, _length: usize) -> Result<(), Errno> {
            Ok(())
        }
    }

    devfs::register_device(
        "random",
        Arc::new(DevRandom {
            ino: devfs::alloc_dev_ino(),
        }),
    );
}

impl RngDevice {
    pub fn is_virtio_rng(device: &PCIDevice) -> bool {
        let cs = device.configuration_space();
        cs.vendor_id().read() == VIRTIO_VENDOR_ID
            && VIRTIO_DEVICE_ID.contains(&cs.device_id().read())
            && cs.subsystem_id().read() == VIRTIO_RNG_SUBSYSTEM_ID
    }

    pub fn initialize(mut pci_device: PCIDevice) -> Result<RngDevice, &'static str> {
        let capabilities = pci_device.capabilities();
        let virtio_capabilities: alloc::vec::Vec<MMIO<virtio_pci_cap>> = capabilities
            .filter(|cap| cap.id().read() == VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID)
            .map(|cap| cap.new_type::<virtio_pci_cap>())
            .collect();

        let common_cfg_cap = virtio_capabilities
            .iter()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_COMMON_CFG)
            .ok_or("Common configuration capability not found")?;

        let config_bar = pci_device.get_or_initialize_bar(common_cfg_cap.bar().read());
        let common_cfg: MMIO<virtio_pci_common_cfg> = MMIO::new(
            (config_bar.cpu_address + common_cfg_cap.offset().read() as usize).as_usize(),
        );

        // Reset and acknowledge
        common_cfg.device_status().write(0x0);
        #[allow(clippy::while_immutable_condition)]
        while common_cfg.device_status().read() != 0x0 {}

        let mut device_status = common_cfg.device_status();
        device_status |= DEVICE_STATUS_ACKNOWLEDGE;
        assert!(
            device_status.read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );
        device_status |= DEVICE_STATUS_DRIVER;
        assert!(
            device_status.read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );

        // Negotiate features (only VIRTIO_F_VERSION_1)
        common_cfg.device_feature_select().write(0);
        let mut device_features = common_cfg.device_feature().read() as u64;
        common_cfg.device_feature_select().write(1);
        device_features |= (common_cfg.device_feature().read() as u64) << 32;

        assert!(
            device_features & VIRTIO_F_VERSION_1 != 0,
            "Virtio version 1 not supported"
        );

        let wanted_features: u64 = VIRTIO_F_VERSION_1;

        common_cfg.driver_feature_select().write(0);
        common_cfg
            .driver_feature()
            .write(u32::try_from(wanted_features & 0xFFFF_FFFF).expect("masked to 32 bits"));
        common_cfg.driver_feature_select().write(1);
        common_cfg
            .driver_feature()
            .write(u32::try_from(wanted_features >> 32).expect("high 32 bits fit in u32"));

        device_status |= DEVICE_STATUS_FEATURES_OK;
        assert!(
            device_status.read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );
        assert!(
            device_status.read() & DEVICE_STATUS_FEATURES_OK != 0,
            "Device features not ok"
        );

        // Setup notification
        let notify_cfg_cap = virtio_capabilities
            .iter()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_NOTIFY_CFG)
            .ok_or("Notification capability not found")?;
        let notify_cfg = notify_cfg_cap.new_type::<virtio_pci_notify_cap>();

        assert!(
            is_power_of_2_or_zero(notify_cfg.notify_off_multiplier().read()),
            "Notify offset multiplier must be a power of 2 or zero"
        );

        let notify_bar = pci_device.get_or_initialize_bar(notify_cfg.cap().bar().read());

        // Setup single request queue at index 0.
        // Write our desired queue size to the device (VirtIO spec allows
        // reducing from the device maximum).
        common_cfg.queue_select().write(0);
        let queue_size = QUEUE_SIZE as u16;
        common_cfg.queue_size().write(queue_size);
        let mut request_queue: VirtQueue<QUEUE_SIZE> = VirtQueue::new(queue_size, 0);

        let notify_mmio: MMIO<u16> = MMIO::new(
            notify_bar.cpu_address.as_usize()
                + notify_cfg.cap().offset().read() as usize
                + common_cfg.queue_notify_off().read() as usize
                    * notify_cfg.notify_off_multiplier().read() as usize,
        );
        request_queue.set_notify(notify_mmio);

        // Configure queue on device
        common_cfg.queue_select().write(0);
        common_cfg
            .queue_desc()
            .write(request_queue.descriptor_area_physical_address());
        common_cfg
            .queue_driver()
            .write(request_queue.driver_area_physical_address());
        common_cfg
            .queue_device()
            .write(request_queue.device_area_physical_address());
        common_cfg.queue_enable().write(1);

        // Mark driver ready
        device_status |= DEVICE_STATUS_DRIVER_OK;
        assert!(
            device_status.read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );

        // Enable Bus Master for DMA and clear Interrupt Disable for legacy INTx
        pci_device
            .configuration_space_mut()
            .set_command_register_bits(crate::pci::command_register::BUS_MASTER);
        pci_device
            .configuration_space_mut()
            .clear_command_register_bits(crate::pci::command_register::INTERRUPT_DISABLE);

        info!("Successfully initialized VirtIO RNG device");

        Ok(RngDevice {
            common_cfg,
            request_queue,
        })
    }

    fn read(&mut self, buf: &mut [u8]) {
        let mut filled = 0;
        while filled < buf.len() {
            let request_len = core::cmp::min(buf.len() - filled, 256);
            let request_buf = vec![0u8; request_len];
            self.request_queue
                .put_buffer(request_buf, BufferDirection::DeviceWritable)
                .expect("Must be able to submit RNG request");
            self.request_queue.notify();

            let received = loop {
                let buffers = self.request_queue.receive_buffer();
                if !buffers.is_empty() {
                    break buffers;
                }
                core::hint::spin_loop();
            };

            for used in received {
                let data = used.buffers.into_first();
                let copy_len = core::cmp::min(data.len(), buf.len() - filled);
                buf[filled..filled + copy_len].copy_from_slice(&data[..copy_len]);
                filled += copy_len;
            }
        }
    }
}

impl Drop for RngDevice {
    fn drop(&mut self) {
        info!("Reset RNG device because of drop");
        self.common_cfg.device_status().write(0x0);
    }
}
