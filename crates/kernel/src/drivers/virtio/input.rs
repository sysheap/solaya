use alloc::{collections::VecDeque, string::String, vec};
use driver_api::{
    BarIndex, BusContext, InputDevice as InputTrait, InputEvent, IrqHandler,
    PciCapabilityHeaderExt, bus::pci_command,
};

use crate::{
    drivers::virtio::{
        capability::{
            DEVICE_STATUS_ACKNOWLEDGE, DEVICE_STATUS_DRIVER, DEVICE_STATUS_DRIVER_OK,
            DEVICE_STATUS_FAILED, DEVICE_STATUS_FEATURES_OK, VIRTIO_F_VERSION_1,
            VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_ISR_CFG, VIRTIO_PCI_CAP_NOTIFY_CFG,
            VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID, virtio_pci_cap, virtio_pci_capFields,
            virtio_pci_common_cfg, virtio_pci_common_cfgFields, virtio_pci_notify_cap,
            virtio_pci_notify_capFields,
        },
        virtqueue::{BufferDirection, VirtQueue},
    },
    info,
    klibc::{
        MMIO, Spinlock,
        util::{ByteInterpretable, is_power_of_2_or_zero},
    },
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

static EVENT_BUFFER: Spinlock<VecDeque<VirtioInputEvent>> = Spinlock::new(VecDeque::new());

/// `driver_api::InputDevice` adapter for the virtio-input driver. Holds the
/// underlying device behind a `Spinlock` so the interrupt path can refill
/// the event queue through `&self`. Also implements `IrqHandler`: the PLIC
/// keeps an `Arc<dyn IrqHandler>` pointing at this struct, so no extra
/// `HANDLE` shim is needed.
pub struct VirtioInputHandle {
    inner: Spinlock<InputDevice>,
    name: String,
    isr_status: MMIO<u32>,
    irq: Spinlock<Option<driver_api::IrqRegistration>>,
}

impl VirtioInputHandle {
    pub fn new(device: InputDevice, isr_status: MMIO<u32>) -> Self {
        Self {
            inner: Spinlock::new(device),
            name: String::from("keyboard0"),
            isr_status,
            irq: Spinlock::new(None),
        }
    }

    pub fn set_irq_registration(&self, registration: driver_api::IrqRegistration) {
        *self.irq.lock() = Some(registration);
    }
}

impl InputTrait for VirtioInputHandle {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll_event(&self) -> Option<InputEvent> {
        EVENT_BUFFER.lock().pop_front().map(|e| InputEvent {
            event_type: e.event_type,
            code: e.code,
            value: e.value,
        })
    }
}

impl IrqHandler for VirtioInputHandle {
    fn handle(&self) {
        let _isr = self.isr_status.read();
        self.inner.lock().process_events();
    }
}

impl InputDevice {
    pub fn is_virtio_input(bus: &dyn BusContext) -> bool {
        crate::drivers::virtio::capability::is_virtio_modern_or_legacy(
            bus,
            VIRTIO_INPUT_SUBSYSTEM_ID,
        )
    }

    pub fn initialize(bus: &dyn BusContext) -> Result<InitializedInput, &'static str> {
        let pci = bus.as_pci().ok_or("virtio-input requires a PCI bus")?;
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

        // Setup eventq at index 0
        common_cfg.queue_select().write(0);
        let queue_size = QUEUE_SIZE as u16;
        common_cfg.queue_size().write(queue_size);
        let mut event_queue: VirtQueue<QUEUE_SIZE> = VirtQueue::new(queue_size, 0);

        let notify_mmio: MMIO<u16> = MMIO::new(
            notify_bar.virt_base
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
        let isr_bar = pci
            .map_bar(BarIndex(isr_cap.bar().read()))
            .map_err(|_| "Failed to map ISR BAR")?;
        let interrupt_status: MMIO<u32> =
            MMIO::new(isr_bar.virt_base + isr_cap.offset().read() as usize);

        // Mark driver ready
        device_status |= DEVICE_STATUS_DRIVER_OK;
        assert!(
            device_status.read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );

        // Enable Bus Master for DMA and clear Interrupt Disable for legacy INTx
        pci.set_command_bits(pci_command::BUS_MASTER);
        pci.clear_command_bits(pci_command::INTERRUPT_DISABLE);

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
                let event: VirtioInputEvent = klib::util::read_from_bytes(data);
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
