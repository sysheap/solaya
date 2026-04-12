//! Typed DMA buffer.
//!
//! `DmaBuffer` owns a page-aligned region suitable for device DMA: a virtual
//! address the CPU uses to read/write, and a physical address the device uses.
//! On the current target the kernel identity-maps physical RAM, so the two
//! addresses are numerically equal; the type exists so a future IOMMU or
//! non-identity mapping is a localized change inside this module rather than
//! an audit of every driver.
//!
//! Backed by `mm::page::PinnedHeapPages`, which allocates through the global
//! page allocator. `Drop` releases the pages automatically. `driver-api` keeps
//! `#![forbid(unsafe_code)]` because all `unsafe` lives inside `mm`.

use mm::page::{Pages, PagesAsSlice, PinnedHeapPages};

use crate::BusError;

/// Page-aligned DMA-capable buffer.
///
/// `len` is the requested byte length. The backing allocation is rounded up to
/// the next page boundary; the extra bytes are not exposed through `as_slice`
/// / `as_mut_slice` but remain valid until `Drop`. Physical address points at
/// byte 0 of the requested region.
pub struct DmaBuffer {
    pages: PinnedHeapPages,
    len: usize,
}

impl DmaBuffer {
    /// Allocate `len` bytes of coherent (cacheable today) DMA memory.
    ///
    /// The allocation is rounded up to the next page boundary. On today's
    /// target (RISC-V 64 with identity-mapped RAM and hardware-coherent DMA
    /// from QEMU virtio devices) "coherent" means the same cacheable memory
    /// the CPU uses — `sync_for_device` / `sync_for_cpu` are no-ops. A future
    /// port to a non-coherent platform implements cache maintenance here.
    pub fn new_coherent(len: usize) -> Result<DmaBuffer, BusError> {
        assert!(
            len > 0,
            "DmaBuffer::new_coherent requires a non-zero length"
        );
        let page_count = mm::util::minimum_amount_of_pages(len);
        let pages = PinnedHeapPages::new_pages(Pages::new(page_count));
        Ok(DmaBuffer { pages, len })
    }

    /// Physical address of the buffer (byte 0).
    ///
    /// Today this equals the virtual address because all physical RAM is
    /// identity-mapped. The type hides that assumption so an IOMMU swap is
    /// localized to this method.
    pub fn phys_addr(&self) -> u64 {
        self.pages.addr() as u64
    }

    /// Virtual (kernel) pointer to the start of the buffer.
    pub fn virt_addr(&self) -> *mut u8 {
        self.pages.addr() as *mut u8
    }

    /// Requested length in bytes (may be less than the page-rounded backing).
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.pages.as_u8_slice_ref()[..self.len]
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let len = self.len;
        &mut self.pages.as_u8_slice()[..len]
    }

    /// Ensure device sees CPU writes. No-op on today's coherent platform.
    pub fn sync_for_device(&self) {}

    /// Ensure CPU sees device writes. No-op on today's coherent platform.
    pub fn sync_for_cpu(&self) {}

    /// Reinterpret the buffer as a typed value of layout `T`.
    ///
    /// The allocation is zero-initialized and page-aligned (alignments up to
    /// 4 KiB are supported). Callers use this to project virtio ring headers,
    /// DWMAC descriptor rings, etc. onto DMA memory without a raw cast in the
    /// kernel crate.
    ///
    /// # Panics
    ///
    /// Panics if `size_of::<T>() > self.len()` or if `align_of::<T>() >
    /// PAGE_SIZE` (alignments beyond the page boundary cannot be guaranteed
    /// from the page-aligned base).
    pub fn as_typed_mut<T>(&mut self) -> &mut T {
        assert!(
            core::mem::size_of::<T>() <= self.len,
            "T does not fit in DmaBuffer"
        );
        assert!(
            core::mem::align_of::<T>() <= mm::PAGE_SIZE,
            "T alignment exceeds page size"
        );
        // SAFETY: `pages.addr()` is a page-aligned pointer to at least
        // `size_of::<T>()` zero-initialized bytes (PinnedHeapPages::new fills
        // with `Page::zero()`). T's size and alignment requirements are
        // checked above. The returned reference is tied to `&mut self` so no
        // aliasing is possible.
        unsafe { &mut *(self.pages.addr() as *mut T) }
    }

    /// Read-only sibling of `as_typed_mut`.
    pub fn as_typed<T>(&self) -> &T {
        assert!(
            core::mem::size_of::<T>() <= self.len,
            "T does not fit in DmaBuffer"
        );
        assert!(
            core::mem::align_of::<T>() <= mm::PAGE_SIZE,
            "T alignment exceeds page size"
        );
        // SAFETY: see `as_typed_mut`. The shared reference is tied to `&self`.
        unsafe { &*(self.pages.addr() as *const T) }
    }
}
