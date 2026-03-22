use alloc::{boxed::Box, vec};
use core::ops::{Add, Deref, DerefMut};

use crate::klibc::util::copy_slice;

pub const PAGE_SIZE: usize = 4096;

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

#[cfg(test)]
mod tests {
    use super::{PAGE_SIZE, Page, PagesAsSlice, PinnedHeapPages};

    #[test]
    fn zero_page() {
        let page = Page::zero();
        assert!(page.iter().all(|&b| b == 0));
    }

    #[test]
    fn new() {
        let heap_pages = PinnedHeapPages::new(2);
        assert_eq!(heap_pages.len(), 2);
    }

    #[test]
    fn with_data() {
        let data = [1u8, 2, 3];
        let mut heap_pages = PinnedHeapPages::new(1);
        heap_pages.fill(&data, 0);
        assert_eq!(heap_pages.len(), 1);
        let heap_slice = heap_pages.as_u8_slice();
        assert_eq!(&heap_slice[..3], &data);
        assert_eq!(&heap_slice[3..], [0; PAGE_SIZE - 3])
    }

    #[test]
    fn with_offset() {
        let data = [1u8, 2, 3];
        let mut heap_pages = PinnedHeapPages::new(1);
        heap_pages.fill(&data, 3);
        assert_eq!(heap_pages.len(), 1);
        let heap_slice = heap_pages.as_u8_slice();
        assert_eq!(&heap_slice[..3], &[0, 0, 0]);
        assert_eq!(&heap_slice[3..6], &data);
        assert_eq!(&heap_slice[6..], [0; PAGE_SIZE - 6])
    }

    #[test]
    fn with_more_data() {
        const LENGTH: usize = PAGE_SIZE + 3;
        let data = [42u8; LENGTH];
        let mut heap_pages = PinnedHeapPages::new(2);
        heap_pages.fill(&data, 0);
        assert_eq!(heap_pages.len(), 2);
        let heap_slice = heap_pages.as_u8_slice();
        assert_eq!(&heap_slice[..LENGTH], &data);
        assert_eq!(&heap_slice[LENGTH..], [0; PAGE_SIZE - 3]);
    }

    #[test]
    fn as_u8_slice_works() {
        let mut heap_pages = PinnedHeapPages::new(2);
        let u8_slice = heap_pages.as_u8_slice();
        assert_eq!(u8_slice.len(), PAGE_SIZE * 2);
    }
}
