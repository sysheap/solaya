#![allow(unsafe_code)]
use crate::{
    debug, info,
    klibc::{MMIO, Spinlock},
    mmio_struct, pci,
};
use alloc::{collections::BTreeMap, vec::Vec};

pub mod address;
mod allocator;
mod devic_tree_parser;
mod lookup;

use lookup::lookup;

pub use address::{PciAddr, PciCpuAddr};
pub use devic_tree_parser::parse;

use self::allocator::PCIAllocator;
pub use self::{
    allocator::PCIAllocatedSpace,
    devic_tree_parser::{PCIBitField, PCIInformation, PCIRange},
};

pub static PCI_ALLOCATOR_64_BIT: Spinlock<PCIAllocator> = Spinlock::new(PCIAllocator::new());
pub static PCI_ALLOCATOR_32_BIT: Spinlock<PCIAllocator> = Spinlock::new(PCIAllocator::new());

const INVALID_VENDOR_ID: u16 = 0xffff;

const GENERAL_DEVICE_TYPE: u8 = 0x0;
const GENERAL_DEVICE_TYPE_MASK: u8 = !0x80;

const CAPABILITY_POINTER_MASK: u8 = !0x3;

pub mod command_register {
    pub const IO_SPACE: u16 = 1 << 0;
    pub const MEMORY_SPACE: u16 = 1 << 1;
    pub const BUS_MASTER: u16 = 1 << 2;
    pub const INTERRUPT_DISABLE: u16 = 1 << 10;
}

mmio_struct! {
    #[repr(C)]
    struct GeneralDevicePciHeader {
        vendor_id: u16,
        device_id: u16,
        command_register: u16,
        status_register: u16,
        revision_id: u8,
        programming_interface_byte: u8,
        subclass: u8,
        class_code: u8,
        cache_line_size: u8,
        latency_timer: u8,
        header_type: u8,
        built_in_self_test: u8,
        bars: [u32; 6],
        cardbus_cis_pointer: u32,
        subsystem_vendor_id: u16,
        subsystem_id: u16,
        expnasion_rom_base_address: u32,
        capabilities_pointer: u8,
    }
}

impl MMIO<GeneralDevicePciHeader> {
    pub fn bar(&self, index: u8) -> u32 {
        assert!(index < 6);
        self.bars().read_index(index as usize)
    }

    pub fn write_bar(&mut self, index: u8, value: u32) {
        assert!(index < 6);
        self.bars().write_index(index as usize, value);
    }

    pub fn set_command_register_bits(&mut self, bits: u16) {
        let mut command_register = self.command_register();
        command_register |= bits;
    }

    pub fn clear_command_register_bits(&mut self, bits: u16) {
        let mut command_register = self.command_register();
        command_register &= !bits;
    }
}

pub struct PciCapabilityIter<'a> {
    pci_device: &'a PCIDevice,
    next_offset: u8, // 0 means there is no next pointer
}

mmio_struct! {
    #[repr(C)]
    struct PciCapability {
        id: u8,
        next: u8,
    }
}

impl Iterator for PciCapabilityIter<'_> {
    type Item = MMIO<PciCapability>;

    fn next(&mut self) -> Option<Self::Item> {
        let capability: MMIO<PciCapability> = match self.next_offset {
            0 => return None,
            // SAFETY: next_offset is read from PCI capability linked list.
            // The offset is within the 256-byte configuration space.
            _ => unsafe {
                self.pci_device
                    .configuration_space
                    .new_type_with_offset(self.next_offset as usize)
            },
        };
        self.next_offset = capability.next().read();
        Some(capability)
    }
}

pub struct PCIDevice {
    configuration_space: MMIO<GeneralDevicePciHeader>,
    initialized_bars: BTreeMap<u8, PCIAllocatedSpace>,
    device_number: u8,
}

impl PCIDevice {
    pub fn configuration_space_mut(&mut self) -> &mut MMIO<GeneralDevicePciHeader> {
        &mut self.configuration_space
    }

    pub fn configuration_space(&self) -> &MMIO<GeneralDevicePciHeader> {
        &self.configuration_space
    }

    // QEMU riscv virt PCI interrupt mapping: IRQ = 32 + (device + pin - 1) % 4
    // where pin is the PCI INTx pin (1=INTA, 2=INTB, etc.)
    pub fn plic_interrupt_id(&self) -> u32 {
        const PLIC_PCI_BASE: u32 = 32;
        // VirtIO PCI devices use INTA (pin 1)
        let pin: u32 = 1;
        PLIC_PCI_BASE + (u32::from(self.device_number) + pin - 1) % 4
    }

    /// # Safety
    /// `address` must point to a valid PCI configuration space MMIO region.
    unsafe fn try_new(address: PciCpuAddr, device_number: u8) -> Option<Self> {
        let pci_device: MMIO<GeneralDevicePciHeader> = MMIO::new(address.as_usize());
        if pci_device.vendor_id().read() == INVALID_VENDOR_ID {
            return None;
        }
        assert!(pci_device.header_type().read() & GENERAL_DEVICE_TYPE_MASK == GENERAL_DEVICE_TYPE);
        Some(Self {
            configuration_space: pci_device,
            initialized_bars: BTreeMap::new(),
            device_number,
        })
    }

    const CAPABILITIES_LIST_BIT: u16 = 1 << 4;
    pub fn capabilities(&self) -> PciCapabilityIter<'_> {
        if self.configuration_space.status_register().read() & Self::CAPABILITIES_LIST_BIT == 0 {
            PciCapabilityIter {
                pci_device: self,
                next_offset: 0,
            }
        } else {
            PciCapabilityIter {
                pci_device: self,
                next_offset: self.configuration_space.capabilities_pointer().read()
                    & CAPABILITY_POINTER_MASK,
            }
        }
    }

    pub fn get_or_initialize_bar(&mut self, index: u8) -> PCIAllocatedSpace {
        if let Some(allocated_space) = self.initialized_bars.get(&index) {
            return *allocated_space;
        }

        let configuration_space = self.configuration_space_mut();

        configuration_space.clear_command_register_bits(
            command_register::IO_SPACE | command_register::MEMORY_SPACE,
        );

        let original_bar_value = configuration_space.bar(index);
        assert!(original_bar_value & 0x1 == 0, "Bar must be memory mapped");
        let bar_type = (original_bar_value & 0b110) >> 1;
        let is_64bit = bar_type == 0x2;

        // Determine size of bar
        configuration_space.write_bar(index, 0xffffffff);
        let bar_value = configuration_space.bar(index);

        // Mask out the 4 lower bits because they describe the type of the bar
        // Invert the value and add 1 to get the size (because the bits that are not set are zero because of alignment)
        let size = !(bar_value & !0b1111) + 1;

        debug!("Bar {} size: {:#x} 64bit={}", index, size, is_64bit);

        let space = if is_64bit {
            pci::PCI_ALLOCATOR_64_BIT.lock().allocate(size as usize)
        } else {
            pci::PCI_ALLOCATOR_32_BIT.lock().allocate(size as usize)
        }
        .expect("There must be enough space for the bar");

        configuration_space.write_bar(
            index,
            u32::try_from(space.pci_address.as_usize() & 0xFFFF_FFFF).expect("masked to 32 bits"),
        );
        if is_64bit {
            configuration_space.write_bar(
                index + 1,
                u32::try_from(space.pci_address.as_usize() >> 32).expect("high 32 bits fit in u32"),
            );
        }

        configuration_space.set_command_register_bits(command_register::MEMORY_SPACE);

        assert!(
            !self.initialized_bars.contains_key(&index),
            "Bar is already initialized"
        );
        self.initialized_bars.insert(index, space);

        space
    }
}

pub fn enumerate_devices(pci_information: &PCIInformation) -> Vec<PCIDevice> {
    let mut pci_devices = Vec::new();
    for bus in 0..255 {
        for device in 0..32 {
            for function in 0..8 {
                let address = pci_address(
                    pci_information.pci_host_bridge_address,
                    bus,
                    device,
                    function,
                );
                // SAFETY: address is computed from the PCI host bridge base
                // and valid bus/device/function numbers.
                let maybe_device = unsafe { PCIDevice::try_new(address, device) };
                if let Some(device) = maybe_device {
                    let vendor_id = device.configuration_space.vendor_id().read();
                    let device_id = device.configuration_space.device_id().read();
                    let name = lookup(vendor_id, device_id).unwrap_or_else(|| {
                        alloc::format!("Unknown device {vendor_id:#06x}:{device_id:#06x}")
                    });
                    info!(
                        "PCI Device {:#x}:{:#x} found at {} ({})",
                        vendor_id, device_id, address, name
                    );
                    pci_devices.push(device);
                }
            }
        }
    }
    pci_devices
}

fn pci_address(starting_address: PciCpuAddr, bus: u8, device: u8, function: u8) -> PciCpuAddr {
    assert!(device < 32);
    assert!(function < 8);
    let offset = ((bus as usize) << 20) | ((device as usize) << 15) | ((function as usize) << 12);
    starting_address + offset
}
