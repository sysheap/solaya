pub mod registry;

pub use drivers::dwmac;

pub use registry::{
    BlockDeviceRegistry, CharDeviceRegistry, DisplayDeviceRegistry, InputDeviceRegistry,
    NetDeviceRegistry, RngDeviceRegistry,
};

use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    device_tree::{self, DtBusContext},
    info,
    net::mac::MacAddress,
    pci::{PCIDevice, PciBusContext},
};
use driver_api::{BusContext, DriverCatalog, DriverInstance, IrqId, ProbeError};
use klib::big_endian::BigEndian;

/// Enumerate every discovered PCI device through the driver catalog. For
/// each device, ask the catalog for the first matching factory; route the
/// returned `DriverInstance` into the typed registry. Devices that no
/// factory claims are left behind (logged as unclaimed).
pub fn init_all_pci_devices(mut pci_devices: Vec<PCIDevice>) {
    let mut catalog = DriverCatalog::new();
    drivers::register_builtin(&mut catalog);

    while let Some(result) = attach_one(&catalog, &mut pci_devices) {
        match result {
            Ok(instance) => route_instance(instance),
            Err(ProbeError::InitializationFailed(msg)) => {
                info!("driver attach failed: {}", msg);
            }
            Err(ProbeError::DoesNotMatch) => {}
        }
    }
}

fn attach_one(
    catalog: &DriverCatalog,
    pci_devices: &mut Vec<PCIDevice>,
) -> Option<Result<DriverInstance, ProbeError>> {
    for i in 0..pci_devices.len() {
        let result = {
            let bus = PciBusContext::new(&mut pci_devices[i]);
            catalog.attach_first_match(&bus)
        };
        if let Some(result) = result {
            pci_devices.swap_remove(i);
            return Some(result);
        }
    }
    None
}

fn route_instance(instance: DriverInstance) {
    match instance {
        DriverInstance::Block(d) => {
            BlockDeviceRegistry::global().register(d);
        }
        DriverInstance::Net(d) => {
            NetDeviceRegistry::global().register(d);
        }
        DriverInstance::Char(d) => {
            CharDeviceRegistry::global().register(d);
        }
        DriverInstance::Display(d) => {
            DisplayDeviceRegistry::global().register(d);
        }
        DriverInstance::Input(d) => {
            InputDeviceRegistry::global().register(d);
        }
        DriverInstance::Rng(d) => {
            RngDeviceRegistry::global().register(d);
        }
    }
}

/// Discover and initialize DWMAC ethernet controllers from the device tree.
/// Only registers the first successfully initialized port with the network stack.
pub fn init_dwmac_devices() {
    if NetDeviceRegistry::global().len() > 0 {
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

        if NetDeviceRegistry::global().len() == 0 {
            let handle = Arc::new(dwmac::DwmacHandle::new(device));
            let irq_handler: Arc<dyn driver_api::IrqHandler> = handle.clone();
            let bus = DtBusContext::new(reg.address, reg.size);
            let registration = bus
                .register_irq(IrqId(plic_irq), irq_handler)
                .expect("register irq");
            handle.set_irq_registration(registration);
            let net_device: Arc<dyn driver_api::NetDevice> = handle;
            NetDeviceRegistry::global().register(net_device);
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
