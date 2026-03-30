pub mod bochs_display;
pub mod virtio;

use alloc::vec::Vec;

use crate::{interrupts::plic, net, pci::PCIDevice, processes::kernel_tasks};

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
        net::assign_network_device(init.device);
        net::init_isr_status(init.interrupt_status);
        plic::register_interrupt(plic_irq, net::on_network_interrupt);
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
        virtio::block::register_isr_status(init.interrupt_status);
        let idx = virtio::block::assign_block_device(init.device);
        virtio::block::register_devfs_node(idx);
        plic::register_interrupt(plic_irq, virtio::block::on_block_interrupt);
    }

    // ext2 is mounted synchronously in kernel_init via block_on_early_init
}

fn init_display_device(pci_devices: &mut Vec<PCIDevice>) {
    if let Some(i) = pci_devices.iter().position(bochs_display::is_bochs_display) {
        let device = pci_devices.swap_remove(i);
        bochs_display::initialize(device);
        bochs_display::register_devfs_node();
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
        virtio::rng::set_device(rng);
        virtio::rng::register_devfs_node();
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
        virtio::input::init_isr_status(init.interrupt_status);
        virtio::input::set_device(init.device);
        plic::register_interrupt(plic_irq, virtio::input::on_input_interrupt);
        virtio::input::register_devfs_node();
    }
}
