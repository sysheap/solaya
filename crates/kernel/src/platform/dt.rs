//! Platform initialisation driven by device-tree nodes: SoC-wide
//! infrastructure that must come up before driver enumeration (today:
//! the L2 cache controller, for DMA coherency).

use crate::{device_tree, info};

/// Apply platform-level device-tree setup. Safe to call once at boot before
/// any driver enumeration; later calls are no-ops.
pub fn init_from_device_tree() {
    init_l2_cache();
}

fn init_l2_cache() {
    let Some(soc) = device_tree::THE.root_node().find_node("soc") else {
        return;
    };
    if let Some(node) = soc.find_node("cache-controller")
        && let Some(reg) = node.parse_reg_property()
    {
        hal::cache::init(reg.address);
        info!("L2 cache controller initialized at {:#x}", reg.address);
    }
}
