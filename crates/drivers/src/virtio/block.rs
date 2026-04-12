use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc, vec, vec::Vec};
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll, Waker},
};
use headers::errno::Errno;

use driver_api::{
    BarIndex, BusContext, DriverFactory, DriverInstance, PciCapabilityHeaderExt, ProbeError,
    bus::pci_command,
};

use console::info;
use hal::{mmio::MMIO, mmio_struct, spinlock::Spinlock};
use klib::{
    non_empty_vec::NonEmptyVec,
    util::{ByteInterpretable, is_power_of_2_or_zero},
};

use crate::virtio::{
    capability::{
        DEVICE_STATUS_ACKNOWLEDGE, DEVICE_STATUS_DRIVER, DEVICE_STATUS_DRIVER_OK,
        DEVICE_STATUS_FAILED, DEVICE_STATUS_FEATURES_OK, VIRTIO_F_VERSION_1,
        VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_DEVICE_CFG, VIRTIO_PCI_CAP_ISR_CFG,
        VIRTIO_PCI_CAP_NOTIFY_CFG, VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID, virtio_pci_cap,
        virtio_pci_capFields, virtio_pci_common_cfg, virtio_pci_common_cfgFields,
        virtio_pci_notify_cap, virtio_pci_notify_capFields,
    },
    virtqueue::{BufferDirection, UsedBuffer, VirtQueue},
};

const EXPECTED_QUEUE_SIZE: usize = 0x100;
const SECTOR_SIZE: usize = 512;
const VIRTIO_BLOCK_SUBSYSTEM_ID: u16 = 2;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_S_OK: u8 = 0;

#[repr(C)]
struct VirtioBlkReqHeader {
    request_type: u32,
    reserved: u32,
    sector: u64,
}

impl ByteInterpretable for VirtioBlkReqHeader {}

mmio_struct! {
    #[repr(C)]
    struct virtio_blk_config {
        capacity: u64,
    }
}

#[allow(dead_code)]
pub struct BlockDevice {
    common_cfg: MMIO<virtio_pci_common_cfg>,
    blk_cfg: MMIO<virtio_blk_config>,
    request_queue: VirtQueue<EXPECTED_QUEUE_SIZE>,
    capacity_sectors: u64,
}

pub struct InitializedBlockDevice {
    pub device: BlockDevice,
    pub interrupt_status: MMIO<u32>,
}

static BLOCK_DEVICES: Spinlock<Vec<BlockDevice>> = Spinlock::new(Vec::new());
static BLOCK_INTERRUPT_COUNTER: AtomicU64 = AtomicU64::new(0);
static BLOCK_INTERRUPT_WAKERS: Spinlock<Vec<Waker>> = Spinlock::new(Vec::new());
static BLOCK_COMPLETIONS: Spinlock<BTreeMap<u16, UsedBuffer>> = Spinlock::new(BTreeMap::new());

/// Per-device IRQ handler. Holds the ISR MMIO register for this particular
/// block device; reads it to acknowledge the interrupt, then wakes the
/// shared bottom-half completion machinery.
pub struct BlockIrqHandler {
    isr_status: MMIO<u32>,
}

impl BlockIrqHandler {
    pub fn new(isr_status: MMIO<u32>) -> Self {
        Self { isr_status }
    }
}

impl driver_api::IrqHandler for BlockIrqHandler {
    fn handle(&self) {
        let _status = self.isr_status.read();
        BLOCK_INTERRUPT_COUNTER.fetch_add(1, Ordering::SeqCst);
        let wakers: Vec<Waker> = BLOCK_INTERRUPT_WAKERS.lock().drain(..).collect();
        for waker in wakers {
            waker.wake();
        }
    }
}

struct BlockInterruptWait {
    seen_counter: u64,
    registered: bool,
}

impl BlockInterruptWait {
    fn new(seen_counter: u64) -> Self {
        Self {
            seen_counter,
            registered: false,
        }
    }
}

impl Future for BlockInterruptWait {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current = BLOCK_INTERRUPT_COUNTER.load(Ordering::SeqCst);
        if current != self.seen_counter {
            return Poll::Ready(());
        }
        if !self.registered {
            BLOCK_INTERRUPT_WAKERS.lock().push(cx.waker().clone());
            self.registered = true;
            let current = BLOCK_INTERRUPT_COUNTER.load(Ordering::SeqCst);
            if current != self.seen_counter {
                return Poll::Ready(());
            }
        }
        Poll::Pending
    }
}

fn harvest_completions(device_index: usize) {
    let received = {
        let mut guard = BLOCK_DEVICES.lock();
        guard
            .get_mut(device_index)
            .map(|dev| dev.request_queue.receive_buffer())
            .unwrap_or_default()
    };
    if !received.is_empty() {
        let mut completions = BLOCK_COMPLETIONS.lock();
        for buf in received {
            completions.insert(buf.index, buf);
        }
    }
}

async fn wait_for_completion(device_index: usize, head_index: u16) -> UsedBuffer {
    loop {
        let seen = BLOCK_INTERRUPT_COUNTER.load(Ordering::SeqCst);
        harvest_completions(device_index);
        if let Some(result) = BLOCK_COMPLETIONS.lock().remove(&head_index) {
            return result;
        }
        BlockInterruptWait::new(seen).await;
    }
}

pub fn assign_block_device(device: BlockDevice) -> usize {
    let mut devices = BLOCK_DEVICES.lock();
    let index = devices.len();
    devices.push(device);
    index
}

/// Adapter that lets the virtio block driver be consumed through the
/// `driver_api::BlockDevice` trait. Records the device's index into the
/// global `BLOCK_DEVICES` and delegates to the existing free functions.
///
/// Phase 1 keeps the global table intact; eliminating it is a later phase.
pub struct BlockDeviceHandle {
    index: usize,
    name: String,
    capacity_sectors: u64,
    _irq: driver_api::IrqRegistration,
}

impl BlockDeviceHandle {
    pub fn new(index: usize, irq: driver_api::IrqRegistration) -> Self {
        assert!(index < 26, "block device index must be < 26 (a-z)");
        let suffix = (b'a' + index as u8) as char;
        let name = alloc::format!("vd{suffix}");
        let capacity_sectors = BLOCK_DEVICES
            .lock()
            .get(index)
            .expect("BlockDeviceHandle created for registered device")
            .capacity_sectors;
        Self {
            index,
            name,
            capacity_sectors,
            _irq: irq,
        }
    }
}

impl driver_api::BlockDevice for BlockDeviceHandle {
    fn name(&self) -> &str {
        &self.name
    }

    fn num_blocks(&self) -> u64 {
        self.capacity_sectors
    }

    fn block_size(&self) -> usize {
        SECTOR_SIZE
    }

    fn read<'a>(
        &'a self,
        offset_bytes: u64,
        buf: &'a mut [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, driver_api::IoError>> + Send + 'a>> {
        let index = self.index;
        Box::pin(async move { read(index, offset_bytes as usize, buf).await })
    }

    fn write<'a>(
        &'a self,
        offset_bytes: u64,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, driver_api::IoError>> + Send + 'a>> {
        let index = self.index;
        Box::pin(async move { write(index, offset_bytes as usize, data).await })
    }
}

async fn read(index: usize, offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
    let cap = {
        let guard = BLOCK_DEVICES.lock();
        let dev = guard.get(index).ok_or(Errno::ENODEV)?;
        dev.capacity_bytes() as usize
    };

    if offset >= cap {
        return Ok(0);
    }
    let read_len = core::cmp::min(buf.len(), cap - offset);
    if read_len == 0 {
        return Ok(0);
    }

    let start_sector = offset / SECTOR_SIZE;
    let offset_in_first_sector = offset % SECTOR_SIZE;
    let end = offset + read_len;
    let end_sector = end.div_ceil(SECTOR_SIZE);
    let num_sectors = end_sector - start_sector;

    let sector_buf_len = num_sectors * SECTOR_SIZE;
    let head_index = {
        let mut guard = BLOCK_DEVICES.lock();
        let dev = guard.get_mut(index).ok_or(Errno::ENODEV)?;
        dev.submit_read(
            u64::try_from(start_sector).expect("sector fits in u64"),
            sector_buf_len,
        )
    };

    let result = wait_for_completion(index, head_index).await;
    assert!(result.buffers.len() == 3, "Expected 3-descriptor chain");
    let status = result.buffers[2][0];
    assert!(
        status == VIRTIO_BLK_S_OK,
        "Block read failed with status {}",
        status
    );

    buf[..read_len].copy_from_slice(
        &result.buffers[1][offset_in_first_sector..offset_in_first_sector + read_len],
    );
    Ok(read_len)
}

async fn write(index: usize, offset: usize, data: &[u8]) -> Result<usize, Errno> {
    let cap = {
        let guard = BLOCK_DEVICES.lock();
        let dev = guard.get(index).ok_or(Errno::ENODEV)?;
        dev.capacity_bytes() as usize
    };

    if offset >= cap {
        return Ok(0);
    }
    let write_len = core::cmp::min(data.len(), cap - offset);
    if write_len == 0 {
        return Ok(0);
    }

    let start_sector = offset / SECTOR_SIZE;
    let offset_in_first_sector = offset % SECTOR_SIZE;
    let end = offset + write_len;
    let end_sector = end.div_ceil(SECTOR_SIZE);
    let num_sectors = end_sector - start_sector;

    // If not sector-aligned, read-modify-write
    let mut sector_buf = vec![0u8; num_sectors * SECTOR_SIZE];
    if offset_in_first_sector != 0 || !end.is_multiple_of(SECTOR_SIZE) {
        let head_index = {
            let mut guard = BLOCK_DEVICES.lock();
            let dev = guard.get_mut(index).ok_or(Errno::ENODEV)?;
            dev.submit_read(
                u64::try_from(start_sector).expect("sector fits in u64"),
                sector_buf.len(),
            )
        };
        let result = wait_for_completion(index, head_index).await;
        assert!(result.buffers.len() == 3, "Expected 3-descriptor chain");
        let status = result.buffers[2][0];
        assert!(
            status == VIRTIO_BLK_S_OK,
            "Block read (for RMW) failed with status {}",
            status
        );
        sector_buf.copy_from_slice(&result.buffers[1]);
    }

    sector_buf[offset_in_first_sector..offset_in_first_sector + write_len]
        .copy_from_slice(&data[..write_len]);

    let head_index = {
        let mut guard = BLOCK_DEVICES.lock();
        let dev = guard.get_mut(index).ok_or(Errno::ENODEV)?;
        dev.submit_write(
            u64::try_from(start_sector).expect("sector fits in u64"),
            &sector_buf,
        )
    };

    let result = wait_for_completion(index, head_index).await;
    assert!(result.buffers.len() == 3, "Expected 3-descriptor chain");
    let status = result.buffers[2][0];
    assert!(
        status == VIRTIO_BLK_S_OK,
        "Block write failed with status {}",
        status
    );

    Ok(write_len)
}

impl BlockDevice {
    pub fn is_virtio_block(bus: &dyn BusContext) -> bool {
        crate::virtio::capability::is_virtio_with_subsystem(bus, VIRTIO_BLOCK_SUBSYSTEM_ID)
    }

    pub fn initialize(bus: &dyn BusContext) -> Result<InitializedBlockDevice, &'static str> {
        let pci = bus.as_pci().ok_or("virtio-block requires a PCI bus")?;
        let mut virtio_capabilities: Vec<MMIO<virtio_pci_cap>> = pci
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

        // Setup single request queue at index 0
        common_cfg.queue_select().write(0);
        let mut request_queue: VirtQueue<EXPECTED_QUEUE_SIZE> =
            VirtQueue::new(common_cfg.queue_size().read(), 0);

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

        // Enable interrupts on request queue
        request_queue.enable_interrupts();

        // Read device config (capacity)
        let blk_cfg_cap = virtio_capabilities
            .iter_mut()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_DEVICE_CFG)
            .ok_or("Device configuration capability not found")?;

        let blk_config_bar = pci
            .map_bar(BarIndex(blk_cfg_cap.bar().read()))
            .map_err(|_| "Failed to map blk-cfg BAR")?;
        let blk_cfg: MMIO<virtio_blk_config> =
            MMIO::new(blk_config_bar.virt_base + blk_cfg_cap.offset().read() as usize);

        let capacity_sectors = blk_cfg.capacity().read();

        // Mark driver ready
        device_status |= DEVICE_STATUS_DRIVER_OK;
        assert!(
            device_status.read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );

        // Parse ISR status capability for interrupt acknowledgment
        let isr_cfg_cap = virtio_capabilities
            .iter()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_ISR_CFG)
            .ok_or("ISR configuration capability not found")?;

        let isr_bar = pci
            .map_bar(BarIndex(isr_cfg_cap.bar().read()))
            .map_err(|_| "Failed to map ISR BAR")?;
        let isr_status: MMIO<u32> =
            MMIO::new(isr_bar.virt_base + isr_cfg_cap.offset().read() as usize);

        // Enable Bus Master for DMA and clear Interrupt Disable for legacy INTx
        pci.set_command_bits(pci_command::BUS_MASTER);
        pci.clear_command_bits(pci_command::INTERRUPT_DISABLE);

        info!(
            "Successfully initialized block device: {} sectors ({} bytes)",
            capacity_sectors,
            capacity_sectors * u64::try_from(SECTOR_SIZE).expect("fits")
        );

        let device = BlockDevice {
            common_cfg,
            blk_cfg,
            request_queue,
            capacity_sectors,
        };

        Ok(InitializedBlockDevice {
            device,
            interrupt_status: isr_status,
        })
    }

    fn capacity_bytes(&self) -> u64 {
        self.capacity_sectors * u64::try_from(SECTOR_SIZE).expect("fits")
    }

    fn submit_read(&mut self, start_sector: u64, buf_len: usize) -> u16 {
        assert!(
            buf_len.is_multiple_of(SECTOR_SIZE),
            "Buffer must be sector-aligned"
        );
        let num_sectors = buf_len / SECTOR_SIZE;
        assert!(
            start_sector + u64::try_from(num_sectors).expect("fits") <= self.capacity_sectors,
            "Read beyond device capacity"
        );

        let header = VirtioBlkReqHeader {
            request_type: VIRTIO_BLK_T_IN,
            reserved: 0,
            sector: start_sector,
        };

        let header_buf = header.as_slice().to_vec();
        let data_buf = vec![0u8; buf_len];
        let status_buf = vec![0u8; 1];

        let chain = NonEmptyVec::new((header_buf, BufferDirection::DriverWritable))
            .push((data_buf, BufferDirection::DeviceWritable))
            .push((status_buf, BufferDirection::DeviceWritable));

        let head = self
            .request_queue
            .put_buffer_chain(chain)
            .expect("Must be able to submit block request");
        self.request_queue.notify();
        head
    }

    fn submit_write(&mut self, start_sector: u64, data: &[u8]) -> u16 {
        assert!(
            data.len().is_multiple_of(SECTOR_SIZE),
            "Data must be sector-aligned"
        );
        let num_sectors = data.len() / SECTOR_SIZE;
        assert!(
            start_sector + u64::try_from(num_sectors).expect("fits") <= self.capacity_sectors,
            "Write beyond device capacity"
        );

        let header = VirtioBlkReqHeader {
            request_type: VIRTIO_BLK_T_OUT,
            reserved: 0,
            sector: start_sector,
        };

        let header_buf = header.as_slice().to_vec();
        let data_buf = data.to_vec();
        let status_buf = vec![0u8; 1];

        let chain = NonEmptyVec::new((header_buf, BufferDirection::DriverWritable))
            .push((data_buf, BufferDirection::DriverWritable))
            .push((status_buf, BufferDirection::DeviceWritable));

        let head = self
            .request_queue
            .put_buffer_chain(chain)
            .expect("Must be able to submit block request");
        self.request_queue.notify();
        head
    }
}

impl Drop for BlockDevice {
    fn drop(&mut self) {
        info!("Reset block device because of drop");
        self.common_cfg.device_status().write(0x0);
    }
}

/// Catalog entry for the virtio-blk driver.
pub struct VirtioBlockFactory;

impl DriverFactory for VirtioBlockFactory {
    fn name(&self) -> &'static str {
        "virtio-blk"
    }

    fn probe(&self, bus: &dyn BusContext) -> bool {
        BlockDevice::is_virtio_block(bus)
    }

    fn attach(&self, bus: &dyn BusContext) -> Result<DriverInstance, ProbeError> {
        let plic_irq = bus
            .as_pci()
            .expect("virtio-blk requires a PCI bus")
            .plic_irq();
        let init = BlockDevice::initialize(bus).map_err(ProbeError::InitializationFailed)?;
        let irq_handler: Arc<dyn driver_api::IrqHandler> =
            Arc::new(BlockIrqHandler::new(init.interrupt_status));
        let irq = bus
            .register_irq(plic_irq, irq_handler)
            .expect("register irq");
        let idx = assign_block_device(init.device);
        let handle: Arc<dyn driver_api::BlockDevice> = Arc::new(BlockDeviceHandle::new(idx, irq));
        Ok(DriverInstance::Block(handle))
    }
}
