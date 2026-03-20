use alloc::{collections::VecDeque, vec};

use crate::{
    drivers::virtio::{
        capability::{
            DEVICE_STATUS_ACKNOWLEDGE, DEVICE_STATUS_DRIVER, DEVICE_STATUS_DRIVER_OK,
            DEVICE_STATUS_FAILED, DEVICE_STATUS_FEATURES_OK, VIRTIO_DEVICE_ID, VIRTIO_F_VERSION_1,
            VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_ISR_CFG, VIRTIO_PCI_CAP_NOTIFY_CFG,
            VIRTIO_VENDOR_ID, VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID, virtio_pci_cap,
            virtio_pci_capFields, virtio_pci_common_cfg, virtio_pci_common_cfgFields,
            virtio_pci_notify_cap, virtio_pci_notify_capFields,
        },
        virtqueue::{BufferDirection, VirtQueue},
    },
    info,
    klibc::{
        MMIO, Spinlock,
        runtime_initialized::RuntimeInitializedData,
        util::{ByteInterpretable, is_power_of_2_or_zero},
    },
    pci::{GeneralDevicePciHeaderFields, PCIDevice, PciCapabilityFields},
};

const QUEUE_SIZE: usize = 32;
const VIRTIO_INPUT_SUBSYSTEM_ID: u16 = 18;
const EVENT_SIZE: usize = core::mem::size_of::<VirtioInputEvent>();
const MAX_BUFFERED_EVENTS: usize = 128;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioInputEvent {
    pub event_type: u16,
    pub code: u16,
    pub value: u32,
}

impl ByteInterpretable for VirtioInputEvent {}

pub struct InputDevice {
    common_cfg: MMIO<virtio_pci_common_cfg>,
    event_queue: VirtQueue<QUEUE_SIZE>,
}

static INPUT_DEVICE: Spinlock<Option<InputDevice>> = Spinlock::new(None);
static EVENT_BUFFER: Spinlock<VecDeque<VirtioInputEvent>> = Spinlock::new(VecDeque::new());
static ISR_STATUS: RuntimeInitializedData<MMIO<u32>> = RuntimeInitializedData::new();

pub fn read_events(buf: &mut [u8]) -> usize {
    let mut events = EVENT_BUFFER.lock();
    let max_events = buf.len() / EVENT_SIZE;
    let mut written = 0;
    for _ in 0..max_events {
        if let Some(event) = events.pop_front() {
            buf[written..written + EVENT_SIZE].copy_from_slice(event.as_slice());
            written += EVENT_SIZE;
        } else {
            break;
        }
    }
    written
}

pub fn on_input_interrupt() {
    let _isr = ISR_STATUS.read();
    let mut dev = INPUT_DEVICE.lock();
    if let Some(dev) = dev.as_mut() {
        dev.process_events();
    }
}

pub fn init_isr_status(isr: MMIO<u32>) {
    ISR_STATUS.initialize(isr);
}

pub fn set_device(device: InputDevice) {
    *INPUT_DEVICE.lock() = Some(device);
}

pub fn register_devfs_node() {
    use crate::fs::{
        devfs,
        vfs::{NodeType, VfsNode},
    };
    use alloc::sync::Arc;
    use headers::errno::Errno;

    struct DevKeyboard {
        ino: u64,
    }

    impl VfsNode for DevKeyboard {
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
            let n = read_events(buf);
            if n == 0 {
                return Err(Errno::EAGAIN);
            }
            Ok(n)
        }
        fn write(&self, _offset: usize, data: &[u8]) -> Result<usize, Errno> {
            Ok(data.len())
        }
        fn truncate(&self) -> Result<(), Errno> {
            Ok(())
        }
    }

    devfs::register_device(
        "keyboard0",
        Arc::new(DevKeyboard {
            ino: devfs::alloc_dev_ino(),
        }),
    );
}

impl InputDevice {
    pub fn is_virtio_input(device: &PCIDevice) -> bool {
        let cs = device.configuration_space();
        if cs.vendor_id().read() != VIRTIO_VENDOR_ID {
            return false;
        }
        let device_id = cs.device_id().read();
        if !VIRTIO_DEVICE_ID.contains(&device_id) {
            return false;
        }
        // Non-transitional VirtIO 1.0+ devices encode the device type in the
        // PCI device ID (0x1040 + type). Transitional devices use subsystem_id.
        if device_id >= 0x1040 {
            return device_id - 0x1040 == VIRTIO_INPUT_SUBSYSTEM_ID;
        }
        cs.subsystem_id().read() == VIRTIO_INPUT_SUBSYSTEM_ID
    }

    pub fn initialize(mut pci_device: PCIDevice) -> Result<InitializedInput, &'static str> {
        let capabilities = pci_device.capabilities();
        let virtio_capabilities: alloc::vec::Vec<MMIO<virtio_pci_cap>> = capabilities
            .filter(|cap| cap.id().read() == VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID)
            // SAFETY: VirtIO vendor-specific capabilities have the virtio_pci_cap
            // layout per the VirtIO spec.
            .map(|cap| unsafe { cap.new_type::<virtio_pci_cap>() })
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
        // SAFETY: The notify capability extends virtio_pci_cap with an
        // additional notify_off_multiplier field per the VirtIO spec.
        let notify_cfg = unsafe { notify_cfg_cap.new_type::<virtio_pci_notify_cap>() };

        assert!(
            is_power_of_2_or_zero(notify_cfg.notify_off_multiplier().read()),
            "Notify offset multiplier must be a power of 2 or zero"
        );

        let notify_bar = pci_device.get_or_initialize_bar(notify_cfg.cap().bar().read());

        // Setup eventq at index 0
        common_cfg.queue_select().write(0);
        let queue_size = QUEUE_SIZE as u16;
        common_cfg.queue_size().write(queue_size);
        let mut event_queue: VirtQueue<QUEUE_SIZE> = VirtQueue::new(queue_size, 0);

        let notify_mmio: MMIO<u16> = MMIO::new(
            notify_bar.cpu_address.as_usize()
                + notify_cfg.cap().offset().read() as usize
                + common_cfg.queue_notify_off().read() as usize
                    * notify_cfg.notify_off_multiplier().read() as usize,
        );
        event_queue.set_notify(notify_mmio);

        common_cfg.queue_select().write(0);
        common_cfg
            .queue_desc()
            .write(event_queue.descriptor_area_physical_address());
        common_cfg
            .queue_driver()
            .write(event_queue.driver_area_physical_address());
        common_cfg
            .queue_device()
            .write(event_queue.device_area_physical_address());
        common_cfg.queue_enable().write(1);

        // ISR status register for interrupt acknowledgement
        let isr_cap = virtio_capabilities
            .iter()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_ISR_CFG)
            .ok_or("ISR capability not found")?;
        let isr_bar = pci_device.get_or_initialize_bar(isr_cap.bar().read());
        let interrupt_status: MMIO<u32> =
            MMIO::new((isr_bar.cpu_address + isr_cap.offset().read() as usize).as_usize());

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

        // Pre-fill eventq with device-writable buffers for receiving events
        event_queue.enable_interrupts();
        let mut device = InputDevice {
            common_cfg,
            event_queue,
        };
        device.fill_event_queue();

        info!("Successfully initialized VirtIO input (keyboard) device");

        Ok(InitializedInput {
            device,
            interrupt_status,
        })
    }

    fn fill_event_queue(&mut self) {
        while self
            .event_queue
            .put_buffer(vec![0u8; EVENT_SIZE], BufferDirection::DeviceWritable)
            .is_ok()
        {}
        self.event_queue.notify();
    }

    fn process_events(&mut self) {
        let received = self.event_queue.receive_buffer();
        if received.is_empty() {
            return;
        }

        let mut events = EVENT_BUFFER.lock();
        for used in &received {
            let data = &used.buffers[0];
            if data.len() >= EVENT_SIZE {
                // SAFETY: data is at least EVENT_SIZE bytes and we use
                // read_unaligned to handle potential alignment issues.
                let event =
                    unsafe { core::ptr::read_unaligned(data.as_ptr().cast::<VirtioInputEvent>()) };
                if events.len() < MAX_BUFFERED_EVENTS {
                    events.push_back(event);
                }
            }
        }
        drop(events);

        self.fill_event_queue();
    }
}

pub struct InitializedInput {
    pub device: InputDevice,
    pub interrupt_status: MMIO<u32>,
}

impl Drop for InputDevice {
    fn drop(&mut self) {
        info!("Reset input device because of drop");
        self.common_cfg.device_status().write(0x0);
    }
}
