use crate::{device_tree, info};
use hal::mmio::MMIO;
use klib::big_endian::BigEndian;

/// Trigger a system reset using the best available mechanism:
/// 1. Device tree `syscon-reboot` node (QEMU + generic boards)
/// 2. JH7110 AON SYSCON software reset (StarFive VisionFive 2)
/// 3. SBI System Reset extension (platform-independent fallback)
pub fn trigger_reset() -> ! {
    if try_syscon_reboot() {
        spin_forever();
    }
    if try_jh7110_aon_reset() {
        spin_forever();
    }
    info!("Falling back to SBI SRST");
    let _ = hal::sbi::extensions::srst_extension::sbi_system_reset(0, 0);
    spin_forever();
}

fn spin_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Look for a `syscon-reboot` node, resolve its regmap phandle to a
/// syscon base address, and write the reboot value at the given offset.
fn try_syscon_reboot() -> bool {
    let root = device_tree::THE.root_node();
    let Some(reboot_node) = root.find_node("reboot") else {
        return false;
    };
    let Some(mut compat) = reboot_node.get_property("compatible") else {
        return false;
    };
    if compat.consume_str() != Some("syscon-reboot") {
        return false;
    }

    let Some(mut regmap_prop) = reboot_node.get_property("regmap") else {
        return false;
    };
    let Some(regmap_phandle) = regmap_prop.consume_sized_type::<BigEndian<u32>>() else {
        return false;
    };

    let offset = reboot_node
        .get_property("offset")
        .and_then(|mut p| p.consume_sized_type::<BigEndian<u32>>())
        .map(|v| v.get() as usize)
        .unwrap_or(0);

    let value = reboot_node
        .get_property("value")
        .and_then(|mut p| p.consume_sized_type::<BigEndian<u32>>())
        .map(|v| v.get())
        .unwrap_or(0);

    let root = device_tree::THE.root_node();
    let Some(syscon_node) = root.find_node_by_phandle(regmap_phandle.get()) else {
        return false;
    };
    let Some(reg) = syscon_node.parse_reg_property() else {
        return false;
    };

    info!(
        "syscon-reboot: writing {:#x} to {:#x}+{:#x}",
        value, reg.address, offset
    );
    MMIO::<u32>::new(reg.address + offset).write(value);
    true
}

/// JH7110 AON SYSCON software reset: clear bit 5 of SYSCFG 40.
fn try_jh7110_aon_reset() -> bool {
    const AON_SYSCON_BASE: usize = 0x1701_0000;
    const AON_SYSCFG_40: usize = 0x28;
    const AON_SW_RESET_BIT: u32 = 1 << 5;

    let root = device_tree::THE.root_node();
    let Some(mut compat) = root.get_property("compatible") else {
        return false;
    };
    let Some(compat_str) = compat.consume_str() else {
        return false;
    };
    if !compat_str.contains("starfive") && !compat_str.contains("jh7110") {
        return false;
    }

    info!(
        "JH7110 AON SYSCON reset: clearing bit 5 at {:#x}",
        AON_SYSCON_BASE + AON_SYSCFG_40
    );
    let mut reg = MMIO::<u32>::new(AON_SYSCON_BASE + AON_SYSCFG_40);
    reg &= !AON_SW_RESET_BIT;
    true
}
