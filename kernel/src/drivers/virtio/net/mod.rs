#![allow(unsafe_code)]
use crate::{
    assert::static_assert_size,
    debug,
    drivers::virtio::{
        capability::{
            DEVICE_STATUS_ACKNOWLEDGE, DEVICE_STATUS_DRIVER, DEVICE_STATUS_DRIVER_OK,
            DEVICE_STATUS_FAILED, DEVICE_STATUS_FEATURES_OK, VIRTIO_DEVICE_ID, VIRTIO_F_VERSION_1,
            VIRTIO_PCI_CAP_COMMON_CFG, VIRTIO_PCI_CAP_DEVICE_CFG, VIRTIO_PCI_CAP_ISR_CFG,
            VIRTIO_PCI_CAP_NOTIFY_CFG, VIRTIO_VENDOR_ID, VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID,
            virtio_pci_cap, virtio_pci_capFields, virtio_pci_common_cfg,
            virtio_pci_common_cfgFields, virtio_pci_notify_cap, virtio_pci_notify_capFields,
        },
        virtqueue::{BufferDirection, VirtQueue},
    },
    info,
    klibc::{
        MMIO,
        util::{BufferExtension, ByteInterpretable, is_power_of_2_or_zero},
    },
    mmio_struct,
    net::mac::MacAddress,
    pci::{GeneralDevicePciHeaderFields, PCIAllocatedSpace, PCIDevice, PciCapabilityFields},
};
use alloc::vec::Vec;

use super::virtqueue::QueueError;

const EXPECTED_QUEUE_SIZE: usize = 0x100;

const VIRTIO_NET_F_MAC: u64 = 1 << 5;

#[allow(dead_code)]
pub struct NetworkDevice {
    device: PCIDevice,
    common_cfg: MMIO<virtio_pci_common_cfg>,
    net_cfg: MMIO<virtio_net_config>,
    notify_cfg: MMIO<virtio_pci_notify_cap>,
    transmit_queue: VirtQueue<EXPECTED_QUEUE_SIZE>,
    receive_queue: VirtQueue<EXPECTED_QUEUE_SIZE>,
    mac_address: MacAddress,
}

const VIRTIO_NETWORK_SUBSYSTEM_ID: u16 = 1;

pub struct InitializedNetworkDevice {
    pub device: NetworkDevice,
    pub interrupt_status: MMIO<u32>,
}

impl NetworkDevice {
    pub fn is_virtio_net(device: &PCIDevice) -> bool {
        let cs = device.configuration_space();
        cs.vendor_id().read() == VIRTIO_VENDOR_ID
            && VIRTIO_DEVICE_ID.contains(&cs.device_id().read())
            && cs.subsystem_id().read() == VIRTIO_NETWORK_SUBSYSTEM_ID
    }

    pub fn initialize(mut pci_device: PCIDevice) -> Result<InitializedNetworkDevice, &'static str> {
        let capabilities = pci_device.capabilities();
        let mut virtio_capabilities: Vec<MMIO<virtio_pci_cap>> = capabilities
            .filter(|cap| cap.id().read() == VIRTIO_VENDOR_SPECIFIC_CAPABILITY_ID)
            // SAFETY: VirtIO vendor-specific capabilities have the virtio_pci_cap
            // layout per the VirtIO spec.
            .map(|cap| unsafe { cap.new_type::<virtio_pci_cap>() })
            .collect();

        let common_cfg = virtio_capabilities
            .iter()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_COMMON_CFG)
            .ok_or("Common configuration capability not found")?;

        debug!("Common configuration capability found at {:?}", common_cfg);

        let config_bar = pci_device.get_or_initialize_bar(common_cfg.bar().read());

        let common_cfg: MMIO<virtio_pci_common_cfg> =
            MMIO::new((config_bar.cpu_address + common_cfg.offset().read() as usize).as_usize());

        debug!("Common config: {:#x?}", common_cfg);

        Self::reset_and_acknowledge(&common_cfg);
        Self::negotiate_features(&common_cfg);

        let notify_cfg = virtio_capabilities
            .iter()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_NOTIFY_CFG)
            .ok_or("Notification capability not found")?;

        // SAFETY: The notify capability extends virtio_pci_cap with an
        // additional notify_off_multiplier field per the VirtIO spec.
        let notify_cfg = unsafe { notify_cfg.new_type::<virtio_pci_notify_cap>() };

        assert!(
            is_power_of_2_or_zero(notify_cfg.notify_off_multiplier().read()),
            "Notify offset multiplier must be a power of 2 or zero"
        );

        assert!(
            notify_cfg.cap().offset().read().is_multiple_of(16),
            "Notify offset must be 16 byte aligned"
        );

        assert!(
            notify_cfg.cap().length().read() >= 2,
            "Notify length must be at least 2"
        );

        let notify_bar = pci_device.get_or_initialize_bar(notify_cfg.cap().bar().read());

        let (mut receive_queue, transmit_queue) =
            Self::setup_virtqueues(&common_cfg, &notify_cfg, &notify_bar);

        receive_queue.enable_interrupts();

        let mut device_status = common_cfg.device_status();
        device_status |= DEVICE_STATUS_DRIVER_OK;

        assert!(
            device_status.read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );

        assert!(
            device_status.read() & DEVICE_STATUS_DRIVER_OK != 0,
            "Device driver not ok"
        );

        debug!("Device initialized: {:#x?}", device_status);

        // Parse ISR status capability for interrupt acknowledgment
        let isr_cfg_cap = virtio_capabilities
            .iter()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_ISR_CFG)
            .ok_or("ISR configuration capability not found")?;

        let isr_bar = pci_device.get_or_initialize_bar(isr_cfg_cap.bar().read());
        let isr_status: MMIO<u32> =
            MMIO::new((isr_bar.cpu_address + isr_cfg_cap.offset().read() as usize).as_usize());

        // Get net configuration
        let net_cfg_cap = virtio_capabilities
            .iter_mut()
            .find(|cap| cap.cfg_type().read() == VIRTIO_PCI_CAP_DEVICE_CFG)
            .ok_or("Device configuration capability not found")?;

        debug!("Device configuration capability found at {:?}", net_cfg_cap);

        let net_config_bar = pci_device.get_or_initialize_bar(net_cfg_cap.bar().read());

        let net_cfg: MMIO<virtio_net_config> = MMIO::new(
            (net_config_bar.cpu_address + net_cfg_cap.offset().read() as usize).as_usize(),
        );

        debug!("Net config: {:#x?}", net_cfg);

        Self::fill_receive_buffers(&mut receive_queue);

        let mac_address = net_cfg.mac().read();

        // Enable Bus Master for DMA and clear Interrupt Disable for legacy INTx
        pci_device
            .configuration_space_mut()
            .set_command_register_bits(crate::pci::command_register::BUS_MASTER);
        pci_device
            .configuration_space_mut()
            .clear_command_register_bits(crate::pci::command_register::INTERRUPT_DISABLE);

        info!(
            "Successfully initialized network device at {:p} with mac {}",
            *pci_device.configuration_space(),
            mac_address
        );

        let device = Self {
            device: pci_device,
            common_cfg,
            net_cfg,
            notify_cfg,
            mac_address,
            receive_queue,
            transmit_queue,
        };

        Ok(InitializedNetworkDevice {
            device,
            interrupt_status: isr_status,
        })
    }

    fn reset_and_acknowledge(common_cfg: &MMIO<virtio_pci_common_cfg>) {
        common_cfg.device_status().write(0x0);

        #[allow(clippy::while_immutable_condition)]
        while common_cfg.device_status().read() != 0x0 {}

        let mut device_status = common_cfg.device_status();
        device_status |= DEVICE_STATUS_ACKNOWLEDGE;

        assert!(
            common_cfg.device_status().read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );

        device_status |= DEVICE_STATUS_DRIVER;

        assert!(
            common_cfg.device_status().read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );
    }

    fn negotiate_features(common_cfg: &MMIO<virtio_pci_common_cfg>) {
        common_cfg.device_feature_select().write(0);
        let mut device_features = common_cfg.device_feature().read() as u64;

        common_cfg.device_feature_select().write(1);
        device_features |= (common_cfg.device_feature().read() as u64) << 32;

        assert!(
            device_features & VIRTIO_F_VERSION_1 != 0,
            "Virtio version 1 not supported"
        );

        let wanted_features: u64 = VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC;

        assert!(
            device_features & wanted_features == wanted_features,
            "Device does not support wanted features"
        );

        common_cfg.driver_feature_select().write(0);
        common_cfg
            .driver_feature()
            .write(u32::try_from(wanted_features & 0xFFFF_FFFF).expect("masked to 32 bits"));

        common_cfg.driver_feature_select().write(1);
        common_cfg
            .driver_feature()
            .write(u32::try_from(wanted_features >> 32).expect("high 32 bits fit in u32"));

        let mut device_status = common_cfg.device_status();
        device_status |= DEVICE_STATUS_FEATURES_OK;

        assert!(
            device_status.read() & DEVICE_STATUS_FAILED == 0,
            "Device failed"
        );

        assert!(
            device_status.read() & DEVICE_STATUS_FEATURES_OK != 0,
            "Device features not ok"
        );
    }

    fn setup_virtqueues(
        common_cfg: &MMIO<virtio_pci_common_cfg>,
        notify_cfg: &MMIO<virtio_pci_notify_cap>,
        notify_bar: &PCIAllocatedSpace,
    ) -> (
        VirtQueue<EXPECTED_QUEUE_SIZE>,
        VirtQueue<EXPECTED_QUEUE_SIZE>,
    ) {
        common_cfg.queue_select().write(0);
        let receive_queue: VirtQueue<EXPECTED_QUEUE_SIZE> =
            VirtQueue::new(common_cfg.queue_size().read(), 0);

        common_cfg.queue_select().write(1);
        let mut transmit_queue: VirtQueue<EXPECTED_QUEUE_SIZE> =
            VirtQueue::new(common_cfg.queue_size().read(), 1);

        assert!(
            notify_cfg.cap().length().read()
                >= common_cfg.queue_notify_off().read() as u32
                    * notify_cfg.notify_off_multiplier().read()
                    + 2,
            "Notify length must be at least the notify offset"
        );

        let transmit_notify: MMIO<u16> = MMIO::new(
            notify_bar.cpu_address.as_usize()
                + notify_cfg.cap().offset().read() as usize
                + common_cfg.queue_notify_off().read() as usize
                    * notify_cfg.notify_off_multiplier().read() as usize,
        );

        transmit_queue.set_notify(transmit_notify);

        Self::configure_queue_on_device(common_cfg, &receive_queue, 0);
        Self::configure_queue_on_device(common_cfg, &transmit_queue, 1);

        (receive_queue, transmit_queue)
    }

    fn configure_queue_on_device(
        common_cfg: &MMIO<virtio_pci_common_cfg>,
        queue: &VirtQueue<EXPECTED_QUEUE_SIZE>,
        index: u16,
    ) {
        common_cfg.queue_select().write(index);
        common_cfg
            .queue_desc()
            .write(queue.descriptor_area_physical_address());
        common_cfg
            .queue_driver()
            .write(queue.driver_area_physical_address());
        common_cfg
            .queue_device()
            .write(queue.device_area_physical_address());
        common_cfg.queue_enable().write(1);
    }

    fn fill_receive_buffers(receive_queue: &mut VirtQueue<EXPECTED_QUEUE_SIZE>) {
        for _ in 0..EXPECTED_QUEUE_SIZE {
            let receive_buffer = vec![0xffu8; 1526];
            receive_queue
                .put_buffer(receive_buffer, BufferDirection::DeviceWritable)
                .expect("Receive buffer must be insertable to the queue");
        }
    }

    pub fn receive_packets(&mut self) -> Vec<Vec<u8>> {
        let new_receive_buffers = self.receive_queue.receive_buffer();
        let mut received_packets = Vec::new();

        for receive_buffer in new_receive_buffers {
            assert!(
                receive_buffer.buffers.len() == 1,
                "Net receive uses single-descriptor buffers"
            );
            let buffer = receive_buffer.buffers.into_first();
            let (net_hdr, data_bytes) = buffer.split_as::<virtio_net_hdr>();

            assert!(net_hdr.gso_type == VIRTIO_NET_HDR_GSO_NONE);
            assert!(net_hdr.flags == 0);

            let data = data_bytes.to_vec();
            received_packets.push(data);

            // Put a fresh buffer back into receive queue
            let receive_buffer = vec![0xffu8; 1526];
            self.receive_queue
                .put_buffer(receive_buffer, BufferDirection::DeviceWritable)
                .expect("Receive buffer must be insertable into the queue.");
        }

        received_packets
    }

    pub fn send_packet(&mut self, data: Vec<u8>) -> Result<u16, QueueError> {
        // First free all already transmitted packets
        debug!("Going to free all buffers which were used to send packets.");
        for transmitted_packet in self.transmit_queue.receive_buffer() {
            debug!("Transmitted packet: {:?}", transmitted_packet.index);
            drop(transmitted_packet);
        }

        let header = virtio_net_hdr {
            flags: 0,
            gso_type: VIRTIO_NET_HDR_GSO_NONE,
            hdr_len: 0,
            gso_size: 0,
            csum_start: 0,
            csum_offset: 0,
            num_buffers: 0,
        };

        let data = [header.as_slice(), data.as_slice()].concat();
        let index = self
            .transmit_queue
            .put_buffer(data, BufferDirection::DriverWritable);

        // Notify device
        self.transmit_queue.notify();

        index
    }

    pub fn get_mac_address(&self) -> MacAddress {
        self.mac_address
    }
}

impl Drop for NetworkDevice {
    fn drop(&mut self) {
        info!("Reset network device becuase of drop");
        self.common_cfg.device_status().write(0x0);
    }
}

mmio_struct! {
    #[repr(C)]
    struct virtio_net_config {
        mac: crate::net::mac::MacAddress,
        status: u16,
        max_virtqueue_pairs: u16,
        mtu: u16,
        speed: u32,
        duplex: u8,
        rss_max_key_size: u8,
        rss_max_indirection_table_length: u16,
        supported_hash_types: u32,
    }
}

const VIRTIO_NET_HDR_GSO_NONE: u8 = 0;

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Debug)]
struct virtio_net_hdr {
    flags: u8,
    gso_type: u8,
    hdr_len: u16,
    gso_size: u16,
    csum_start: u16,
    csum_offset: u16,
    num_buffers: u16,
    // hash_value: u32,
    // hash_report: u16,
    // padding_reserved: u16,
}

static_assert_size!(virtio_net_hdr, 12);

impl ByteInterpretable for virtio_net_hdr {}
