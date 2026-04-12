//! System bring-up policy.
//!
//! Phase 8 split: `drivers::*` does pure mechanism (enumerate buses, construct
//! `BusContext`s, call each driver's `initialize`, push into the typed
//! `Registry`). This module reads those registries and applies policy —
//! populate devfs, mount the root filesystem, spawn the network RX task, etc.
//!
//! Called from `kernel_init` *after* all driver enumeration has completed, so
//! the registries are populated before policy runs.

use crate::{
    drivers::{
        BlockDeviceRegistry, DisplayDeviceRegistry, InputDeviceRegistry, NetDeviceRegistry,
        RngDeviceRegistry,
    },
    fs, net,
    processes::kernel_tasks,
};

/// Apply bring-up policy on top of the populated device registries.
pub fn bring_up_system() {
    expose_devices_in_devfs();
    mount_root_filesystem();
    spawn_network_rx_task();
}

fn expose_devices_in_devfs() {
    let block = BlockDeviceRegistry::global();
    for i in 0..block.len() {
        if let Some(dev) = block.get(i) {
            fs::devfs::register_block_device(dev);
        }
    }

    let display = DisplayDeviceRegistry::global();
    for i in 0..display.len() {
        if let Some(dev) = display.get(i) {
            fs::devfs::register_display_device(dev);
        }
    }

    let input = InputDeviceRegistry::global();
    for i in 0..input.len() {
        if let Some(dev) = input.get(i) {
            fs::devfs::register_input_device(dev);
        }
    }

    let rng = RngDeviceRegistry::global();
    for i in 0..rng.len() {
        if let Some(dev) = rng.get(i) {
            fs::devfs::register_rng_device(dev);
        }
    }
}

fn mount_root_filesystem() {
    if let Some(dev) = BlockDeviceRegistry::global().get(0) {
        kernel_tasks::spawn(fs::ext2::mount_ext2(dev));
    }
}

fn spawn_network_rx_task() {
    if NetDeviceRegistry::global().len() > 0 {
        kernel_tasks::spawn(net::network_rx_task());
    }
}
