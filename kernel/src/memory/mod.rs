use crate::klibc::Spinlock;
#[cfg(target_arch = "riscv64")]
use crate::{device_tree, info};

use core::{ops::Range, ptr::NonNull};
#[cfg(target_arch = "riscv64")]
use linker_information::LinkerInformation;
use sys::memory::page_allocator::{MetadataPageAllocator, PageAllocator};

pub mod heap;
#[cfg(target_arch = "riscv64")]
pub mod linker_information;
pub mod page_table_entry;
#[cfg(target_arch = "riscv64")]
pub mod page_tables;
#[cfg(target_arch = "riscv64")]
mod runtime_mappings;

pub use sys::memory::{
    address::{PhysAddr, VirtAddr},
    page::{PAGE_SIZE, Page, Pages, PagesAsSlice, PinnedHeapPages, page_slice_at_phys},
};

#[cfg(target_arch = "riscv64")]
pub use runtime_mappings::initialize_runtime_mappings;

#[cfg(target_arch = "riscv64")]
pub fn kernel_device_mappings() -> alloc::vec::Vec<page_tables::MappingDescription> {
    use crate::{
        device_tree,
        interrupts::plic,
        io::{TEST_DEVICE_ADDRESS, uart::UART_BASE_ADDRESS},
        processes::timer,
    };
    use alloc::vec::Vec;

    let mut mappings = Vec::new();
    mappings.push(page_tables::MappingDescription {
        virtual_address_start: VirtAddr::new(*plic::PLIC_BASE),
        size: *plic::PLIC_SIZE,
        privileges: page_tables::XWRMode::ReadWrite,
        name: "PLIC",
    });
    if let Some((clint_base, clint_size)) = timer::clint_region() {
        mappings.push(page_tables::MappingDescription {
            virtual_address_start: VirtAddr::new(clint_base),
            size: clint_size,
            privileges: page_tables::XWRMode::ReadWrite,
            name: "CLINT",
        });
    }
    mappings.push(page_tables::MappingDescription {
        virtual_address_start: VirtAddr::new(UART_BASE_ADDRESS),
        size: PAGE_SIZE,
        privileges: page_tables::XWRMode::ReadWrite,
        name: "UART",
    });
    // Map ethernet controller and supporting hardware regions from device tree
    let soc = device_tree::THE.root_node().find_node("soc");
    if let Some(soc) = &soc {
        for child in soc.children() {
            if child.name.starts_with("ethernet@")
                && let Some(reg) = child.parse_reg_property()
            {
                mappings.push(page_tables::MappingDescription {
                    virtual_address_start: VirtAddr::new(reg.address),
                    size: reg.size,
                    privileges: page_tables::XWRMode::ReadWrite,
                    name: "Ethernet",
                });
            }
        }
        // JH7110 clock/reset generators, system controllers, and watchdog.
        // The clock-controller node has multiple reg entries (sys, stg, aon);
        // map all of them so every CRG region is accessible.
        if let Some(node) = soc.find_node("clock-controller") {
            for reg in node.parse_all_reg_properties() {
                let size = if reg.size < PAGE_SIZE {
                    PAGE_SIZE
                } else {
                    reg.size
                };
                mappings.push(page_tables::MappingDescription {
                    virtual_address_start: VirtAddr::new(reg.address),
                    size,
                    privileges: page_tables::XWRMode::ReadWrite,
                    name: "JH7110 CRG/SYSCON",
                });
            }
        }
        for name in ["sys_syscon", "aon_syscon", "wdog"] {
            if let Some(node) = soc.find_node(name)
                && let Some(reg) = node.parse_reg_property()
            {
                let size = if reg.size < PAGE_SIZE {
                    PAGE_SIZE
                } else {
                    reg.size
                };
                mappings.push(page_tables::MappingDescription {
                    virtual_address_start: VirtAddr::new(reg.address),
                    size,
                    privileges: page_tables::XWRMode::ReadWrite,
                    name: "JH7110 CRG/SYSCON",
                });
            }
        }
    }

    if device_tree::THE.root_node().find_node("test").is_some() {
        mappings.push(page_tables::MappingDescription {
            virtual_address_start: VirtAddr::new(TEST_DEVICE_ADDRESS),
            size: PAGE_SIZE,
            privileges: page_tables::XWRMode::ReadWrite,
            name: "Qemu Test Device",
        });
    }
    for mapping in runtime_mappings::get_runtime_mappings() {
        mappings.push(mapping.clone());
    }
    mappings
}

static PAGE_ALLOCATOR: Spinlock<MetadataPageAllocator> =
    Spinlock::new(MetadataPageAllocator::new());

pub struct StaticPageAllocator;

impl PageAllocator for StaticPageAllocator {
    fn alloc(number_of_pages_requested: usize) -> Option<Range<NonNull<Page>>> {
        PAGE_ALLOCATOR.lock().alloc(number_of_pages_requested)
    }

    fn dealloc(page: NonNull<Page>) -> usize {
        PAGE_ALLOCATOR.lock().dealloc(page)
    }
}

#[cfg(any(not(target_arch = "riscv64"), miri))]
pub fn heap_size() -> usize {
    crate::memory::PAGE_SIZE
}

#[cfg(all(target_arch = "riscv64", not(miri)))]
pub fn heap_size() -> usize {
    let memory_node = device_tree::THE
        .root_node()
        .find_node("memory")
        .expect("There must be a memory node");

    let reg = memory_node
        .parse_reg_property()
        .expect("Memory node must have a reg property");

    let ram_end_address = reg.address + reg.size;
    ram_end_address - LinkerInformation::__start_heap().as_usize()
}

#[cfg(target_arch = "riscv64")]
pub fn init_page_allocator(reserved_areas: &[Range<*const u8>]) {
    let heap_start = LinkerInformation::__start_heap();
    let heap_size = heap_size();

    info!("Initializing page allocator");
    info!(
        "Heap Start: {}-{} (size: {:#x} -> {})",
        heap_start,
        heap_start + heap_size,
        heap_size,
        crate::klibc::util::PrintMemorySizeHumanFriendly(heap_size)
    );

    let memory = sys::memory::linker_region_as_uninit_slice(
        sys::memory::VirtAddr::new(heap_start.as_usize()),
        heap_size,
    );
    PAGE_ALLOCATOR.lock().init(memory, reserved_areas);
}

pub fn used_heap_pages() -> usize {
    PAGE_ALLOCATOR.lock().used_heap_pages()
}

pub fn total_heap_pages() -> usize {
    PAGE_ALLOCATOR.lock().total_heap_pages()
}
