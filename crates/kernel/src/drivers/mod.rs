pub mod registry;

pub use registry::{Registry, registry};

use alloc::vec::Vec;

use driver_api::{
    BlockDevice, CharDevice, DisplayDevice, DriverCatalog, DriverInstance, InputDevice, NetDevice,
    ProbeError, RngDevice,
};

use crate::{
    device_tree::{self, DtBusContext},
    info,
    pci::{PCIDevice, PciBusContext},
};

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

/// Walk every child of the `soc` device-tree node through the driver
/// catalog. DT-bound drivers (today: DWMAC) probe via
/// `BusContext::as_dt()` just like PCI drivers probe via `as_pci()`.
pub fn init_all_dt_devices() {
    let mut catalog = DriverCatalog::new();
    drivers::register_builtin(&mut catalog);

    let Some(soc) = device_tree::THE.root_node().find_node("soc") else {
        return;
    };

    for node in soc.children() {
        let Some(reg) = node.parse_reg_property() else {
            continue;
        };
        let bus = DtBusContext::new(node, reg.address, reg.size);
        match catalog.attach_first_match(&bus) {
            Some(Ok(instance)) => route_instance(instance),
            Some(Err(ProbeError::InitializationFailed(msg))) => {
                info!("DT driver attach failed: {}", msg);
            }
            Some(Err(ProbeError::DoesNotMatch)) | None => {}
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
            registry::<dyn BlockDevice>().register(d);
        }
        DriverInstance::Net(d) => {
            registry::<dyn NetDevice>().register(d);
        }
        DriverInstance::Char(d) => {
            registry::<dyn CharDevice>().register(d);
        }
        DriverInstance::Display(d) => {
            registry::<dyn DisplayDevice>().register(d);
        }
        DriverInstance::Input(d) => {
            registry::<dyn InputDevice>().register(d);
        }
        DriverInstance::Rng(d) => {
            registry::<dyn RngDevice>().register(d);
        }
    }
}
