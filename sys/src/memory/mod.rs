pub mod address;
pub mod heap;
pub mod page;
pub mod page_allocator;

pub use address::{PhysAddr, VirtAddr};
pub use page::{PAGE_SIZE, Page, Pages, PagesAsSlice, PinnedHeapPages};

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

/// Create an immutable byte slice from a linker-defined section.
/// The caller must ensure the region [start, start+size) is validly mapped.
pub fn linker_region_as_slice(start: VirtAddr, size: usize) -> &'static [u8] {
    // SAFETY: Linker-defined sections are statically mapped and immutable
    // after boot.
    unsafe { core::slice::from_raw_parts(start.as_ptr::<u8>(), size) }
}

/// Create an immutable byte slice from a firmware-provided pointer.
/// Panics if ptr is null. The caller must ensure the region [ptr, ptr+size)
/// is validly mapped and immutable for the lifetime of the kernel.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn firmware_blob_as_slice(ptr: *const u8, size: usize) -> &'static [u8] {
    assert!(!ptr.is_null(), "firmware blob pointer must not be null");
    // SAFETY: Firmware guarantees the blob is a contiguous, immutable region.
    // The function is intentionally safe — it encapsulates the unsafety of the
    // firmware → kernel boundary.
    unsafe { core::slice::from_raw_parts(ptr, size) }
}
