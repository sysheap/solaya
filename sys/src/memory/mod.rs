pub mod address;
pub mod heap;
pub mod page;
pub mod page_allocator;
pub mod page_table;

pub use address::{PhysAddr, VirtAddr};
pub use page::{PAGE_SIZE, Page, Pages, PagesAsSlice, PinnedHeapPages, page_slice_at_phys};

use core::mem::MaybeUninit;

/// Create a mutable slice of uninitialized bytes from a linker-defined region.
/// The caller must ensure the region [start, start+size) is validly mapped
/// and not aliased.
pub fn linker_region_as_uninit_slice(
    start: VirtAddr,
    size: usize,
) -> &'static mut [MaybeUninit<u8>] {
    // SAFETY: Linker-defined regions are statically mapped and MaybeUninit<u8>
    // has no validity requirements.
    unsafe { core::slice::from_raw_parts_mut(start.as_mut_ptr::<MaybeUninit<u8>>(), size) }
}
