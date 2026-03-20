#![allow(unsafe_code)]
use super::page::Page;
use crate::{
    debug,
    klibc::util::{align_down_ptr, minimum_amount_of_pages},
    memory::PAGE_SIZE,
};
use core::{
    fmt::Debug,
    mem::MaybeUninit,
    ops::Range,
    ptr::{NonNull, null_mut},
};

#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
enum PageStatus {
    Free,
    Used,
    Last,
}

impl PageStatus {
    fn is_free(&self) -> bool {
        matches!(self, Self::Free)
    }
}

pub(super) struct MetadataPageAllocator<'a> {
    metadata: &'a mut [PageStatus],
    pages: Range<*mut MaybeUninit<Page>>,
}

// SAFETY: The metadata page allocator can be accessed from any thread
unsafe impl Send for MetadataPageAllocator<'_> {}

impl Debug for MetadataPageAllocator<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PageAllocator")
            .field("metadata", &self.metadata.as_ptr())
            .field("pages", &self.pages)
            .finish()
    }
}

impl<'a> MetadataPageAllocator<'a> {
    pub(super) const fn new() -> Self {
        Self {
            metadata: &mut [],
            pages: null_mut()..null_mut(),
        }
    }

    pub(super) fn init(
        &mut self,
        memory: &'a mut [MaybeUninit<u8>],
        reserved_areas: &[Range<*const u8>],
    ) {
        let heap_size = memory.len();
        let number_of_heap_pages = heap_size / (PAGE_SIZE + 1); // We need one byte per page as metadata

        let (metadata, heap) = memory.split_at_mut(number_of_heap_pages);

        // SAFETY: PageStatus is repr(u8), so any u8 alignment is sufficient.
        // We assert begin/end are empty to confirm perfect alignment.
        let (begin, metadata, end) = unsafe { metadata.align_to_mut::<MaybeUninit<PageStatus>>() };
        assert!(begin.is_empty());
        assert!(end.is_empty());

        // SAFETY: Page is repr(C, align(4096)). The heap region starts at a
        // page-aligned address (verified by the assert below).
        let (_begin, heap, _end) = unsafe { heap.align_to_mut::<MaybeUninit<Page>>() };
        assert!(metadata.len() <= heap.len());
        assert!((heap[0].as_ptr() as usize).is_multiple_of(PAGE_SIZE));

        let size_metadata = core::mem::size_of_val(metadata);
        let size_heap = core::mem::size_of_val(heap);
        assert!(size_metadata + size_heap <= heap_size);

        metadata.iter_mut().for_each(|x| {
            x.write(PageStatus::Free);
        });

        // SAFETY: All elements were initialized via MaybeUninit::write above.
        self.metadata = unsafe { metadata.assume_init_mut() };

        self.pages = heap.as_mut_ptr_range();

        // Set reserved areas to used
        for area in reserved_areas {
            self.mark_pointer_range_as_used_without_initialize(area);
        }

        debug!("Page allocator initalized");
        debug!("Metadata start:\t\t{:p}", self.metadata);
        debug!("Heap start:\t\t{:p}", self.pages.start);
        debug!("Number of pages:\t{}\n", self.total_heap_pages());
    }

    pub fn total_heap_pages(&self) -> usize {
        self.metadata.len()
    }

    pub fn used_heap_pages(&self) -> usize {
        self.metadata.iter().filter(|m| !m.is_free()).count()
    }

    fn page_idx_to_pointer(&self, page_index: usize) -> NonNull<MaybeUninit<Page>> {
        // SAFETY: page_index is within the metadata bounds, so the resulting
        // pointer is within the heap pages allocation.
        unsafe {
            NonNull::new(self.pages.start.add(page_index))
                .expect("Heap pointer from add() must be non-null")
        }
    }

    fn page_pointer_to_page_idx(&self, page: NonNull<MaybeUninit<Page>>) -> usize {
        let heap_start = self.pages.start;
        let heap_end = self.pages.end;
        let page_ptr = page.as_ptr();
        assert!(page_ptr >= heap_start);
        assert!(page_ptr < heap_end);
        assert!(page_ptr.is_aligned());
        // SAFETY: Both pointers are within the same heap allocation, verified
        // by the assertions above.
        let offset = unsafe { page_ptr.offset_from(heap_start) };
        offset.cast_unsigned()
    }

    pub fn alloc(&mut self, number_of_pages_requested: usize) -> Option<Range<NonNull<Page>>> {
        assert!(number_of_pages_requested > 0, "Cannot allocate zero pages");
        let total_pages = self.total_heap_pages();
        if number_of_pages_requested > total_pages {
            return None;
        }
        (0..=(self.total_heap_pages() - number_of_pages_requested))
            .find(|&idx| self.is_range_free(idx, number_of_pages_requested))
            .map(|start_idx| {
                self.mark_range_as_used(start_idx, number_of_pages_requested, true);
                // NonNull<MaybeUninit<Page>> can be cast to NonNull<Page> because they are
                // initialized in mark_range_as_used
                self.page_idx_to_pointer(start_idx).cast()
                    ..self
                        .page_idx_to_pointer(start_idx + number_of_pages_requested)
                        .cast()
            })
    }

    fn is_range_free(&self, start_idx: usize, number_of_pages: usize) -> bool {
        (start_idx..start_idx + number_of_pages).all(|idx| self.metadata[idx].is_free())
    }

    fn mark_range_as_used(
        &mut self,
        start_idx: usize,
        number_of_pages: usize,
        initialize_if_needed: bool,
    ) {
        // It is clearer to express this the current way it is
        #[allow(clippy::needless_range_loop)]
        for idx in start_idx..start_idx + number_of_pages {
            if initialize_if_needed {
                let page = self.page_idx_to_pointer(idx);
                // SAFETY: We know that this is a valid pointer inside the heap
                unsafe {
                    page.write(MaybeUninit::zeroed());
                }
            }
            let status = if idx == start_idx + number_of_pages - 1 {
                PageStatus::Last
            } else {
                PageStatus::Used
            };

            self.metadata[idx] = status;
        }
    }

    fn range_to_start_aligned_and_number_of_pages<T>(
        &self,
        range: &Range<*const T>,
    ) -> (usize, usize) {
        let start_aligned = align_down_ptr(range.start, PAGE_SIZE);
        // We don't use the offset_from pointer functions because this requires
        // that both pointers point to the same allocation which is not the case
        let new_length = range.end as usize - start_aligned as usize;
        let number_of_pages = minimum_amount_of_pages(new_length);
        let start_idx = self.page_pointer_to_page_idx(
            NonNull::new(start_aligned as *mut _).expect("start_aligned is not allowed to be NULL"),
        );
        (start_idx, number_of_pages)
    }

    fn mark_pointer_range_as_used_without_initialize<T>(&mut self, range: &Range<*const T>) {
        let (start_idx, number_of_pages) = self.range_to_start_aligned_and_number_of_pages(range);
        assert!(
            self.is_range_free(start_idx, number_of_pages),
            "Reserved area should be free. Otherwise with have problems with overlapping LAST bits"
        );
        self.mark_range_as_used(start_idx, number_of_pages, false);
    }

    pub fn dealloc(&mut self, page: NonNull<Page>) -> usize {
        let mut count = 0;
        let mut idx = self.page_pointer_to_page_idx(page.cast());

        assert!(
            self.metadata[idx] == PageStatus::Used || self.metadata[idx] == PageStatus::Last,
            "Double-free detected: page at index {idx} has status {:?}",
            self.metadata[idx]
        );

        while self.metadata[idx] != PageStatus::Last {
            self.metadata[idx] = PageStatus::Free;
            idx += 1;
            count += 1;
        }
        self.metadata[idx] = PageStatus::Free;
        count += 1;
        count
    }
}

pub trait PageAllocator {
    fn alloc(number_of_pages_requested: usize) -> Option<Range<NonNull<Page>>>;
    fn dealloc(page: NonNull<Page>) -> usize;
}

#[cfg(test)]
mod tests {
    use super::{MetadataPageAllocator, PAGE_SIZE, Page};
    use crate::{klibc::Spinlock, memory::page_allocator::PageStatus};
    use core::{
        mem::MaybeUninit,
        ops::Range,
        ptr::{NonNull, addr_of, addr_of_mut},
    };

    const MEMORY_PATTERN: u8 = 0x42;

    static mut PAGE_ALLOC_MEMORY: [MaybeUninit<u8>; PAGE_SIZE * 8] =
        [const { MaybeUninit::uninit() }; _];
    static PAGE_ALLOC: Spinlock<MetadataPageAllocator> =
        Spinlock::new(MetadataPageAllocator::new());

    fn init_allocator(fill: bool, reserved_areas: &[Range<*const u8>]) {
        unsafe {
            if fill {
                // Miri will catch if there is a bug here. Let's take the easy way.
                #[allow(static_mut_refs)]
                PAGE_ALLOC_MEMORY.fill(MaybeUninit::new(MEMORY_PATTERN));
            }
            PAGE_ALLOC
                .lock()
                .init(&mut *addr_of_mut!(PAGE_ALLOC_MEMORY), reserved_areas);
        }
    }

    fn alloc(number_of_pages: usize) -> Option<Range<NonNull<Page>>> {
        PAGE_ALLOC.lock().alloc(number_of_pages)
    }

    fn dealloc(pages: Range<NonNull<Page>>) -> usize {
        PAGE_ALLOC.lock().dealloc(pages.start)
    }

    #[test_case]
    fn clean_start() {
        init_allocator(false, &[]);
        assert!(
            PAGE_ALLOC
                .lock()
                .metadata
                .iter()
                .all(|s| *s == PageStatus::Free)
        );
    }

    #[test_case]
    fn exhaustion_allocation() {
        init_allocator(false, &[]);
        let number_of_pages = PAGE_ALLOC.lock().total_heap_pages();
        let _pages = alloc(number_of_pages).expect("Allocation must succeed");
        assert!(alloc(1).is_none());
        let allocator = PAGE_ALLOC.lock();
        let (last, all_metadata_except_last) = allocator
            .metadata
            .split_last()
            .expect("Metadata must not be empty");
        assert!(
            all_metadata_except_last
                .iter()
                .all(|s| *s == PageStatus::Used)
        );
        assert_eq!(*last, PageStatus::Last);
    }

    #[test_case]
    fn beyond_capacity() {
        init_allocator(false, &[]);
        let number_of_pages = PAGE_ALLOC.lock().total_heap_pages();
        let pages = alloc(number_of_pages + 1);
        assert!(pages.is_none());
    }

    #[test_case]
    fn all_single_allocations() {
        init_allocator(false, &[]);
        let number_of_pages = PAGE_ALLOC.lock().total_heap_pages();
        for _ in 0..number_of_pages {
            assert!(alloc(1).is_some());
        }
        assert!(alloc(1).is_none());
    }

    #[test_case]
    fn metadata_integrity() {
        init_allocator(false, &[]);
        let page1 = alloc(1).expect("Single page allocation must succeed");
        assert_eq!(PAGE_ALLOC.lock().metadata[0], PageStatus::Last);
        assert!(
            PAGE_ALLOC.lock().metadata[1..]
                .iter()
                .all(|s| *s == PageStatus::Free)
        );
        let page2 = alloc(2).expect("Two page allocation must succeed");
        assert_eq!(
            PAGE_ALLOC.lock().metadata[..3],
            [PageStatus::Last, PageStatus::Used, PageStatus::Last]
        );
        assert!(
            PAGE_ALLOC.lock().metadata[3..]
                .iter()
                .all(|s| *s == PageStatus::Free)
        );
        let page3 = alloc(3).expect("Three page allocation must succeed");
        assert_eq!(
            PAGE_ALLOC.lock().metadata[..6],
            [
                PageStatus::Last,
                PageStatus::Used,
                PageStatus::Last,
                PageStatus::Used,
                PageStatus::Used,
                PageStatus::Last
            ]
        );
        assert!(
            PAGE_ALLOC.lock().metadata[6..]
                .iter()
                .all(|s| *s == PageStatus::Free),
        );
        dealloc(page2);
        assert_eq!(
            PAGE_ALLOC.lock().metadata[..6],
            [
                PageStatus::Last,
                PageStatus::Free,
                PageStatus::Free,
                PageStatus::Used,
                PageStatus::Used,
                PageStatus::Last
            ]
        );
        dealloc(page1);
        assert_eq!(
            PAGE_ALLOC.lock().metadata[..6],
            [
                PageStatus::Free,
                PageStatus::Free,
                PageStatus::Free,
                PageStatus::Used,
                PageStatus::Used,
                PageStatus::Last
            ]
        );
        dealloc(page3);
        assert_eq!(
            PAGE_ALLOC.lock().metadata[..6],
            [
                PageStatus::Free,
                PageStatus::Free,
                PageStatus::Free,
                PageStatus::Free,
                PageStatus::Free,
                PageStatus::Free
            ]
        );
    }

    #[test_case]
    fn zero_overwrite() {
        init_allocator(true, &[]);
        let first_page = PAGE_ALLOC.lock().pages.start as *const u8;
        unsafe { assert_eq!((*first_page), MEMORY_PATTERN) }
        let page = PAGE_ALLOC
            .lock()
            .alloc(1)
            .expect("Single page allocation must succeed")
            .start;
        unsafe {
            assert_eq!(page.read(), Page::zero());
        }
    }

    #[test_case]
    fn reserved_pages() {
        init_allocator(false, &[]);

        let address = PAGE_ALLOC.lock().pages.start as *const u8;
        init_allocator(true, &[address..address.wrapping_add(1)]);

        let first_page = PAGE_ALLOC.lock().pages.start as *const u8;
        unsafe { assert_eq!((*first_page), MEMORY_PATTERN) }

        let _page = PAGE_ALLOC
            .lock()
            .alloc(1)
            .expect("Single page allocation must succeed")
            .start;
        unsafe { assert_eq!((*first_page), MEMORY_PATTERN) }
    }
}
