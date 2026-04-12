use alloc::{boxed::Box, vec};
use core::ops::{Add, Deref, DerefMut};

use hal::memory::{PAGE_SIZE, PhysAddr};
use klib::util::copy_slice;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pages(usize);

impl Pages {
    pub const fn new(count: usize) -> Self {
        Self(count)
    }

    pub const fn count(self) -> usize {
        self.0
    }

    pub const fn as_bytes(self) -> usize {
        self.0 * PAGE_SIZE
    }
}

impl Add<Pages> for usize {
    type Output = usize;

    fn add(self, rhs: Pages) -> Self::Output {
        rhs.as_bytes() + self
    }
}

#[derive(PartialEq, Eq, Clone)]
#[repr(C, align(4096))]
pub struct Page([u8; PAGE_SIZE]);

impl Deref for Page {
    type Target = [u8; PAGE_SIZE];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Page {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl core::fmt::Debug for Page {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Page({:p})", self.0.as_ptr())
    }
}

impl Page {
    pub fn zero() -> Self {
        Self([0; PAGE_SIZE])
    }
}

pub trait PagesAsSlice {
    fn as_u8_slice(&mut self) -> &mut [u8];
    fn as_u8_slice_ref(&self) -> &[u8];
}

impl PagesAsSlice for [Page] {
    fn as_u8_slice(&mut self) -> &mut [u8] {
        // SAFETY: Page is repr(C, align(4096)) containing [u8; PAGE_SIZE],
        // so reinterpreting &mut [Page] as &mut [u8] is valid. The lifetime
        // is tied to &mut self.
        unsafe {
            core::slice::from_raw_parts_mut(
                self.as_mut_ptr().cast::<u8>(),
                core::mem::size_of_val(self),
            )
        }
    }

    fn as_u8_slice_ref(&self) -> &[u8] {
        // SAFETY: Same invariants as as_u8_slice, reinterpreting &[Page] as
        // &[u8] via an identical layout (repr(C, align(4096)) over [u8; N]).
        unsafe {
            core::slice::from_raw_parts(self.as_ptr().cast::<u8>(), core::mem::size_of_val(self))
        }
    }
}

/// Returns a reference to the 4K page at the given physical address.
/// Works because the kernel identity-maps all physical memory.
pub fn page_slice_at_phys(addr: PhysAddr) -> &'static [u8; PAGE_SIZE] {
    assert!(addr.is_page_aligned());
    // SAFETY: The kernel identity-maps all physical RAM, so the physical
    // address equals the virtual address. The returned reference is valid
    // for the lifetime of the kernel (physical pages are never moved).
    unsafe { &*(addr.as_usize() as *const [u8; PAGE_SIZE]) }
}

#[derive(Debug)]
pub struct PinnedHeapPages {
    allocation: Box<[Page]>,
}

impl PinnedHeapPages {
    pub fn new(number_of_pages: usize) -> Self {
        assert!(number_of_pages > 0);
        let allocation = vec![Page::zero(); number_of_pages].into_boxed_slice();
        Self { allocation }
    }

    pub fn new_pages(pages: Pages) -> Self {
        Self::new(pages.count())
    }

    pub fn fill(&mut self, data: &[u8], offset: usize) {
        copy_slice(data, &mut self.as_u8_slice()[offset..offset + data.len()]);
    }

    pub fn addr(&self) -> usize {
        self.allocation.as_ptr() as usize
    }

    pub fn size(&self) -> usize {
        self.allocation.len() * PAGE_SIZE
    }
}

impl Deref for PinnedHeapPages {
    type Target = [Page];

    fn deref(&self) -> &Self::Target {
        &self.allocation
    }
}

impl DerefMut for PinnedHeapPages {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.allocation
    }
}
