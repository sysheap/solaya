use alloc::{string::String, sync::Arc, vec};
use driver_api::{
    BarIndex, BusContext, DriverFactory, DriverInstance, IoError, PciCapabilityHeaderExt,
    ProbeError, RngDevice as RngTrait, bus::pci_command,
};

use console::info;
use hal::{mmio::MMIO, spinlock::Spinlock};
use klib::util::is_power_of_2_or_zero;

use crate::virtio::{
    capability::{
        DEVICE_STATUS_ACKNOWLEDGE, DEVICE_STATUS_DRIVER, DEVICE_STATUS_DRIVER_OK,
        DEVICE_STATUS_FAILED, DEVICE_STATUS_FEATURES_OK, VIRTIO_F_VERSION_1,
        VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_NOTIFY_CFG, VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID,
        virtio_pci_cap, virtio_pci_capFields, virtio_pci_common_cfg, virtio_pci_common_cfgFields,
        virtio_pci_notify_cap, virtio_pci_notify_capFields,
    },
    virtqueue::{BufferDirection, VirtQueue},
};

const QUEUE_SIZE: usize = 0x10;
const VIRTIO_RNG_SUBSYSTEM_ID: u16 = 4;

pub struct RngDevice {
    common_cfg: MMIO<virtio_pci_common_cfg>,
    request_queue: VirtQueue<QUEUE_SIZE>,
}

/// `driver_api::RngDevice` adapter for the virtio-rng driver. Holds the
/// underlying device behind a `Spinlock` so `fill` can take `&self` while
/// mutating the virtqueue.
pub struct VirtioRngHandle {
    inner: Spinlock<RngDevice>,
    name: String,
}

impl VirtioRngHandle {
    pub fn new(device: RngDevice) -> Self {
        Self {
            inner: Spinlock::new(device),
            name: String::from("random"),
        }
    }
}

impl RngTrait for VirtioRngHandle {
    fn name(&self) -> &str {
        &self.name
    }

    fn fill(&self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.inner.lock().read(buf);
        Ok(buf.len())
    }
}

impl RngDevice {
    pub fn is_virtio_rng(bus: &dyn BusContext) -> bool {
        crate::virtio::capability::is_virtio_with_subsystem(bus, VIRTIO_RNG_SUBSYSTEM_ID)
    }

    pub fn initialize(bus: &dyn BusContext) -> Result<RngDevice, &'static str> {
        let pci = bus.as_pci().ok_or("virtio-rng requires a PCI bus")?;
        let virtio_capabilities: alloc::vec::Vec<MMIO<virtio_pci_cap>> = pci
            .capabilities()
            .filter(|cap| cap.id() == VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID)
            .map(|cap| cap.as_type::<virtio_pci_cap>())
            .collect();

        let common_cfg_cap = virtio_capabilities
            .iter()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_COMMON_CFG)
            .ok_or("Common configuration capability not found")?;

        let config_bar = pci
            .map_bar(BarIndex(common_cfg_cap.bar().read()))
            .map_err(|_| "Failed to map common-cfg BAR")?;
        let common_cfg: MMIO<virtio_pci_common_cfg> =
            MMIO::new(config_bar.virt_base + common_cfg_cap.offset().read() as usize);

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

        let notify_bar = pci
            .map_bar(BarIndex(notify_cfg.cap().bar().read()))
            .map_err(|_| "Failed to map notify BAR")?;

        // Setup single request queue at index 0.
        // Write our desired queue size to the device (VirtIO spec allows
        // reducing from the device maximum).
        common_cfg.queue_select().write(0);
        let queue_size = QUEUE_SIZE as u16;
        common_cfg.queue_size().write(queue_size);
        let mut request_queue: VirtQueue<QUEUE_SIZE> = VirtQueue::new(queue_size, 0);

        let notify_mmio: MMIO<u16> = MMIO::new(
            notify_bar.virt_base
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
        pci.set_command_bits(pci_command::BUS_MASTER);
        pci.clear_command_bits(pci_command::INTERRUPT_DISABLE);

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

/// Catalog entry for the virtio-rng driver.
pub struct VirtioRngFactory;

impl DriverFactory for VirtioRngFactory {
    fn name(&self) -> &'static str {
        "virtio-rng"
    }

    fn probe(&self, bus: &dyn BusContext) -> bool {
        RngDevice::is_virtio_rng(bus)
    }

    fn attach(&self, bus: &dyn BusContext) -> Result<DriverInstance, ProbeError> {
        let rng = RngDevice::initialize(bus).map_err(ProbeError::InitializationFailed)?;
        let handle: Arc<dyn driver_api::RngDevice> = Arc::new(VirtioRngHandle::new(rng));
        Ok(DriverInstance::Rng(handle))
    }
}
