pub use sys::memory::page::{PAGE_SIZE, Page, Pages, PagesAsSlice, PinnedHeapPages};

#[cfg(test)]
mod tests {
    use super::{PAGE_SIZE, Page, PagesAsSlice, PinnedHeapPages};

    #[test_case]
    fn zero_page() {
        let page = Page::zero();
        assert!(page.iter().all(|&b| b == 0));
    }

    #[test_case]
    fn new() {
        let heap_pages = PinnedHeapPages::new(2);
        assert_eq!(heap_pages.len(), 2);
    }

    #[test_case]
    fn with_data() {
        let data = [1u8, 2, 3];
        let mut heap_pages = PinnedHeapPages::new(1);
        heap_pages.fill(&data, 0);
        assert_eq!(heap_pages.len(), 1);
        let heap_slice = heap_pages.as_u8_slice();
        assert_eq!(&heap_slice[..3], &data);
        assert_eq!(&heap_slice[3..], [0; PAGE_SIZE - 3])
    }

    #[test_case]
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

    #[test_case]
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

    #[test_case]
    fn as_u8_slice_works() {
        let mut heap_pages = PinnedHeapPages::new(2);
        let u8_slice = heap_pages.as_u8_slice();
        assert_eq!(u8_slice.len(), PAGE_SIZE * 2);
    }
}
