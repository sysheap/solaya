pub mod bochs_display;
pub mod dwmac;
pub mod jh7110;
pub mod registry;
pub mod virtio;

pub use registry::{
    BlockDeviceRegistry, CharDeviceRegistry, DisplayDeviceRegistry, InputDeviceRegistry,
    NetDeviceRegistry, RngDeviceRegistry,
};

use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    device_tree, fs, info,
    interrupts::plic,
    klibc::big_endian::BigEndian,
    net::{self, mac::MacAddress},
    pci::PCIDevice,
    processes::kernel_tasks,
};

pub fn init_all_pci_devices(mut pci_devices: Vec<PCIDevice>) {
    init_network_device(&mut pci_devices);
    init_block_devices(&mut pci_devices);
    init_display_device(&mut pci_devices);
    init_rng_device(&mut pci_devices);
    init_input_device(&mut pci_devices);
}

fn init_network_device(pci_devices: &mut Vec<PCIDevice>) {
    if let Some(i) = pci_devices
        .iter()
        .position(virtio::net::NetworkDevice::is_virtio_net)
    {
        let device = pci_devices.swap_remove(i);
        let plic_irq = device.plic_interrupt_id();
        let init =
            virtio::net::NetworkDevice::initialize(device).expect("Initialization must work.");
        let handle = Arc::new(virtio::net::VirtioNetHandle::new(
            init.device,
            init.interrupt_status,
        ));
        let irq_handler: Arc<dyn driver_api::IrqHandler> = handle.clone();
        let registration = plic::register(plic_irq, irq_handler);
        handle.set_irq_registration(registration);
        let net_device: Arc<dyn driver_api::NetDevice> = handle;
        NetDeviceRegistry::global().register(net_device);
        kernel_tasks::spawn(net::network_rx_task());
    }
}

fn init_block_devices(pci_devices: &mut Vec<PCIDevice>) {
    while let Some(i) = pci_devices
        .iter()
        .position(virtio::block::BlockDevice::is_virtio_block)
    {
        let device = pci_devices.swap_remove(i);
        let plic_irq = device.plic_interrupt_id();
        let init = virtio::block::BlockDevice::initialize(device)
            .expect("Block device initialization must work.");
        let irq_handler: Arc<dyn driver_api::IrqHandler> =
            Arc::new(virtio::block::BlockIrqHandler::new(init.interrupt_status));
        let irq = plic::register(plic_irq, irq_handler);
        let idx = virtio::block::assign_block_device(init.device);
        let handle: Arc<dyn driver_api::BlockDevice> =
            Arc::new(virtio::block::BlockDeviceHandle::new(idx, irq));
        let registered_idx = BlockDeviceRegistry::global().register(handle.clone());
        assert!(
            registered_idx == idx,
            "registry index must match virtio BLOCK_DEVICES index during Phase 1"
        );
        fs::devfs::register_block_device(handle);
    }

    if BlockDeviceRegistry::global().len() > 0
        && let Some(primary) = BlockDeviceRegistry::global().get(0)
    {
        kernel_tasks::spawn(fs::ext2::mount_ext2(primary));
    }
}

fn init_display_device(pci_devices: &mut Vec<PCIDevice>) {
    if let Some(i) = pci_devices.iter().position(bochs_display::is_bochs_display) {
        let device = pci_devices.swap_remove(i);
        let handle = bochs_display::initialize(device);
        DisplayDeviceRegistry::global().register(handle.clone());
        fs::devfs::register_display_device(handle);
    }
}

fn init_rng_device(pci_devices: &mut Vec<PCIDevice>) {
    if let Some(i) = pci_devices
        .iter()
        .position(virtio::rng::RngDevice::is_virtio_rng)
    {
        let device = pci_devices.swap_remove(i);
        let rng = virtio::rng::RngDevice::initialize(device)
            .expect("RNG device initialization must work.");
        let handle: Arc<dyn driver_api::RngDevice> =
            Arc::new(virtio::rng::VirtioRngHandle::new(rng));
        RngDeviceRegistry::global().register(handle.clone());
        fs::devfs::register_rng_device(handle);
    }
}

/// Discover and initialize DWMAC ethernet controllers from the device tree.
/// Only registers the first successfully initialized port with the network stack.
pub fn init_dwmac_devices() {
    if net::has_network_device() {
        return;
    }

    let Some(soc) = device_tree::THE.root_node().find_node("soc") else {
        return;
    };

    for child in soc.children() {
        if !child.name.starts_with("ethernet@") {
            continue;
        }

        let Some(mut compat) = child.get_property("compatible") else {
            continue;
        };
        let Some(compat_str) = compat.consume_str() else {
            continue;
        };
        if compat_str != "starfive,jh7110-eqos-5.20" {
            continue;
        }

        let Some(reg) = child.parse_reg_property() else {
            continue;
        };

        // Parse MAC address from local-mac-address property (6 bytes)
        let Some(mac_prop) = child.get_property("local-mac-address") else {
            continue;
        };
        let mac_bytes = mac_prop.buffer();
        if mac_bytes.len() < 6 {
            continue;
        }
        let mac = MacAddress::new([
            mac_bytes[0],
            mac_bytes[1],
            mac_bytes[2],
            mac_bytes[3],
            mac_bytes[4],
            mac_bytes[5],
        ]);

        // Parse first interrupt number (macirq)
        let Some(mut irq_prop) = child.get_property("interrupts") else {
            continue;
        };
        let Some(plic_irq) = irq_prop.consume_sized_type::<BigEndian<u32>>() else {
            continue;
        };
        let plic_irq = plic_irq.get();

        // Parse clock IDs: sequence of (phandle, clock_id) pairs
        let clock_ids = parse_phandle_ids(&child, "clocks");
        let reset_ids = parse_phandle_ids(&child, "resets");

        // Determine GMAC index from base address
        let gmac_index = match reg.address {
            0x1604_0000 => 1u8, // we only use one ethernet port right now
            _ => continue,
        };

        info!(
            "DWMAC: found GMAC{} at {:#x}, IRQ {}, MAC {}",
            gmac_index, reg.address, plic_irq, mac
        );

        // Initialize clocks, resets, and syscon
        dwmac::jh7110::init_gmac(gmac_index, &clock_ids, &reset_ids);

        // Initialize L2 cache controller for DMA coherency (once)
        init_l2_cache_from_device_tree(&soc);

        // PHY address matches GMAC index (ethernet-phy@0 / ethernet-phy@1)
        let phy_addr = gmac_index as u32;

        // Initialize the DWMAC hardware (may fail if DMA reset is stuck)
        let Some(device) = dwmac::DwmacDevice::new(reg.address, mac, phy_addr) else {
            continue;
        };

        if !net::has_network_device() {
            let handle = Arc::new(dwmac::DwmacHandle::new(device));
            let irq_handler: Arc<dyn driver_api::IrqHandler> = handle.clone();
            let registration = plic::register(plic_irq, irq_handler);
            handle.set_irq_registration(registration);
            let net_device: Arc<dyn driver_api::NetDevice> = handle;
            NetDeviceRegistry::global().register(net_device);
            kernel_tasks::spawn(net::network_rx_task());
            info!("DWMAC: GMAC{} registered as network device", gmac_index);
        }
    }
}

fn parse_phandle_ids(node: &device_tree::Node<'_>, prop_name: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    let Some(mut prop) = node.get_property(prop_name) else {
        return ids;
    };
    // Each entry is (phandle: u32, id: u32) — we skip the phandle and collect the ID
    while let Some(_phandle) = prop.consume_sized_type::<BigEndian<u32>>() {
        if let Some(id) = prop.consume_sized_type::<BigEndian<u32>>() {
            ids.push(id.get());
        }
    }
    ids
}

fn init_l2_cache_from_device_tree(soc: &device_tree::Node<'_>) {
    static INITIALIZED: AtomicBool = AtomicBool::new(false);
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(node) = soc.find_node("cache-controller")
        && let Some(reg) = node.parse_reg_property()
    {
        hal::cache::init(reg.address);
        info!("L2 cache controller initialized at {:#x}", reg.address);
    }
}

fn init_input_device(pci_devices: &mut Vec<PCIDevice>) {
    if let Some(i) = pci_devices
        .iter()
        .position(virtio::input::InputDevice::is_virtio_input)
    {
        let device = pci_devices.swap_remove(i);
        let plic_irq = device.plic_interrupt_id();
        let init = virtio::input::InputDevice::initialize(device)
            .expect("Input device initialization must work.");
        let handle = Arc::new(virtio::input::VirtioInputHandle::new(
            init.device,
            init.interrupt_status,
        ));
        let irq_handler: Arc<dyn driver_api::IrqHandler> = handle.clone();
        let registration = plic::register(plic_irq, irq_handler);
        handle.set_irq_registration(registration);
        let trait_handle: Arc<dyn driver_api::InputDevice> = handle;
        InputDeviceRegistry::global().register(trait_handle.clone());
        fs::devfs::register_input_device(trait_handle);
    }
}
