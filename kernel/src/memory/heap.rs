#![allow(unsafe_code)]
use core::{
    alloc::{GlobalAlloc, Layout},
    marker::PhantomData,
    mem::{align_of, size_of},
    ptr::{NonNull, null_mut},
};

use crate::{
    assert::static_assert_size,
    klibc::{
        Spinlock,
        util::{align_up, minimum_amount_of_pages},
    },
};

use super::{PAGE_SIZE, page_allocator::PageAllocator};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
struct AlignedSizeWithMetadata {
    size: usize,
}

impl AlignedSizeWithMetadata {
    const fn new() -> Self {
        Self { size: 0 }
    }

    fn from_layout(layout: Layout) -> Self {
        assert!(FreeBlock::DATA_ALIGNMENT >= layout.align());
        let size = align_up(
            core::cmp::max(layout.size(), FreeBlock::MINIMUM_SIZE),
            FreeBlock::DATA_ALIGNMENT,
        );
        Self { size }
    }

    const fn from_pages(pages: usize) -> Self {
        Self {
            size: align_up(pages * PAGE_SIZE, FreeBlock::DATA_ALIGNMENT),
        }
    }

    const fn total_size(&self) -> usize {
        self.size
    }

    const fn get_remaining_size(&self, needed_size: AlignedSizeWithMetadata) -> Self {
        assert!(self.total_size() >= needed_size.total_size() + FreeBlock::MINIMUM_SIZE);
        Self {
            size: self.size - needed_size.size,
        }
    }
}

#[repr(C, align(8))]
struct FreeBlock {
    next: Option<NonNull<FreeBlock>>,
    size: AlignedSizeWithMetadata,
    // data: u64, This field is virtual because otherwise the offset calculation would be wrong
}

static_assert_size!(FreeBlock, 16);

impl FreeBlock {
    const METADATA_SIZE: usize = size_of::<Self>();
    const DATA_ALIGNMENT: usize = align_of::<usize>();
    const MINIMUM_SIZE: usize = Self::METADATA_SIZE + Self::DATA_ALIGNMENT;

    const fn new() -> Self {
        Self {
            next: None,
            size: AlignedSizeWithMetadata::new(),
        }
    }

    const fn new_with_size(size: AlignedSizeWithMetadata) -> Self {
        Self { next: None, size }
    }

    fn initialize(block_ptr: NonNull<FreeBlock>, size: AlignedSizeWithMetadata) {
        let data_size = size.total_size();

        assert!(data_size >= Self::MINIMUM_SIZE);

        assert!(data_size >= Self::DATA_ALIGNMENT, "FreeBlock too small");
        assert!(
            data_size.is_multiple_of(Self::DATA_ALIGNMENT),
            "FreeBlock not aligned (data_size={data_size})"
        );

        let block = FreeBlock::new_with_size(size);
        // SAFETY: block_ptr comes from the page allocator and has sufficient
        // size (verified by assertions above) and alignment for FreeBlock.
        unsafe {
            block_ptr.write(block);
        }
    }

    fn split(
        mut block_ptr: NonNull<FreeBlock>,
        requested_size: AlignedSizeWithMetadata,
    ) -> NonNull<FreeBlock> {
        // SAFETY: block_ptr is a valid heap block managed by our allocator.
        let block = unsafe { block_ptr.as_mut() };
        assert!(block.size.total_size() >= requested_size.total_size() + Self::MINIMUM_SIZE);
        assert!(
            requested_size
                .total_size()
                .is_multiple_of(Self::DATA_ALIGNMENT)
        );

        let remaining_size = block.size.get_remaining_size(requested_size);

        // SAFETY: The original block is large enough (checked above) so the
        // new pointer is within the same allocation.
        let new_block = unsafe { block_ptr.byte_add(requested_size.total_size()) };

        assert!(
            remaining_size
                .total_size()
                .is_multiple_of(Self::DATA_ALIGNMENT)
        );

        block.size = requested_size;

        Self::initialize(new_block, remaining_size);
        new_block
    }
}

struct Heap<Allocator: PageAllocator> {
    genesis_block: FreeBlock,
    allocator: PhantomData<Allocator>,
    allocated_memory: usize,
}

impl<Allocator: PageAllocator> Heap<Allocator> {
    const fn new() -> Self {
        Self {
            genesis_block: FreeBlock::new(),
            allocator: PhantomData,
            allocated_memory: 0,
        }
    }

    pub fn allocated_memory(&self) -> usize {
        self.allocated_memory
    }

    fn is_page_allocator_allocation(&self, layout: &Layout) -> bool {
        layout.size() >= PAGE_SIZE || layout.align() > FreeBlock::DATA_ALIGNMENT
    }

    fn alloc(&mut self, layout: core::alloc::Layout) -> *mut u8 {
        if self.is_page_allocator_allocation(&layout) {
            // Allocate directly from the page allocator
            let pages = minimum_amount_of_pages(layout.size());
            if let Some(allocation) = Allocator::alloc(pages) {
                self.allocated_memory += pages * PAGE_SIZE;
                return allocation.start.cast().as_ptr();
            }
            return null_mut();
        }

        let requested_size = AlignedSizeWithMetadata::from_layout(layout);
        let block = if let Some(block) = self.find_and_remove(requested_size) {
            block
        } else {
            let pages = minimum_amount_of_pages(requested_size.total_size());
            let allocation = if let Some(allocation) = Allocator::alloc(pages) {
                allocation
            } else {
                return null_mut();
            };
            let free_block_ptr = allocation.start.cast();
            FreeBlock::initialize(free_block_ptr, AlignedSizeWithMetadata::from_pages(pages));
            free_block_ptr
        };

        // Make smaller if needed
        self.split_if_necessary(block, requested_size);

        self.allocated_memory += requested_size.total_size();

        block.cast().as_ptr()
    }

    fn dealloc(&mut self, ptr: *mut u8, layout: core::alloc::Layout) {
        assert!(!ptr.is_null());
        if self.is_page_allocator_allocation(&layout) {
            // SAFETY: ptr was returned by alloc and is non-null (asserted above).
            unsafe {
                let pages = Allocator::dealloc(NonNull::new_unchecked(ptr).cast());
                self.allocated_memory -= pages * PAGE_SIZE;
            }
            return;
        }
        let size = AlignedSizeWithMetadata::from_layout(layout);
        // SAFETY: ptr is non-null (asserted above) and was allocated with
        // FreeBlock alignment by our alloc method.
        let free_block_ptr = unsafe { NonNull::new_unchecked(ptr).cast() };
        let free_block = FreeBlock::new_with_size(size);
        // SAFETY: The pointer is valid and has sufficient size/alignment for
        // FreeBlock (it was originally allocated as one).
        unsafe {
            free_block_ptr.write(free_block);
            self.insert(free_block_ptr);
        }
        self.allocated_memory -= size.total_size();
    }

    fn insert(&mut self, mut block_ptr: NonNull<FreeBlock>) {
        // SAFETY: block_ptr is a valid heap block initialized by our allocator.
        let block = unsafe { block_ptr.as_mut() };
        assert!(block.next.is_none(), "Heap metadata corruption");
        block.next = self.genesis_block.next.take();
        self.genesis_block.next = Some(block_ptr);
    }

    fn split_if_necessary(
        &mut self,
        block_ptr: NonNull<FreeBlock>,
        requested_size: AlignedSizeWithMetadata,
    ) {
        // SAFETY: block_ptr is a valid heap block managed by our allocator.
        let block = unsafe { block_ptr.as_ref() };
        let current_block_size = block.size;
        assert!(current_block_size >= requested_size);
        if (current_block_size.total_size() - requested_size.total_size()) < FreeBlock::MINIMUM_SIZE
        {
            return;
        }
        let new_block = FreeBlock::split(block_ptr, requested_size);
        self.insert(new_block);
    }

    fn find_and_remove(
        &mut self,
        requested_size: AlignedSizeWithMetadata,
    ) -> Option<NonNull<FreeBlock>> {
        let mut current = &mut self.genesis_block;
        // SAFETY: Each block in the free list was inserted by our allocator
        // and remains valid until removed.
        while let Some(potential_block) = current.next.map(|mut block| unsafe { block.as_mut() }) {
            if potential_block.size < requested_size {
                current = potential_block;
                continue;
            }

            // Take the block out of the list
            let block = current.next.take();
            current.next = potential_block.next.take();
            return block;
        }
        None
    }
}

struct SpinlockHeap<Allocator: PageAllocator> {
    inner: Spinlock<Heap<Allocator>>,
}

// SAFETY: Heap is only accessed through a Spinlock, which provides mutual
// exclusion. The raw pointers in the free list are not shared.
unsafe impl<Allocator: PageAllocator> Send for Heap<Allocator> {}

impl<Allocator: PageAllocator> SpinlockHeap<Allocator> {
    const fn new() -> Self {
        Self {
            inner: Spinlock::new(Heap::new()),
        }
    }
}

// SAFETY: GlobalAlloc requires thread-safe alloc/dealloc. We delegate to a
// Spinlock-protected Heap which serializes all access.
unsafe impl<Allocator: PageAllocator> GlobalAlloc for SpinlockHeap<Allocator> {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        self.inner.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        self.inner.lock().dealloc(ptr, layout)
    }
}

#[cfg(all(target_arch = "riscv64", not(miri)))]
#[global_allocator]
static HEAP: SpinlockHeap<super::StaticPageAllocator> = SpinlockHeap::new();

#[cfg(all(target_arch = "riscv64", not(miri)))]
pub fn allocated_size() -> usize {
    HEAP.inner.lock().allocated_memory()
}

#[cfg(any(not(target_arch = "riscv64"), miri))]
pub fn allocated_size() -> usize {
    0
}

#[cfg(test)]
mod test {
    use super::{FreeBlock, PAGE_SIZE, SpinlockHeap};
    use crate::{
        klibc::Spinlock,
        memory::{
            page::Page,
            page_allocator::{MetadataPageAllocator, PageAllocator},
        },
    };
    use core::{
        alloc::GlobalAlloc,
        mem::MaybeUninit,
        ops::Range,
        ptr::{NonNull, addr_of_mut},
    };

    const HEAP_PAGES: usize = 8;
    const HEAP_SIZE: usize = (HEAP_PAGES - 1) * PAGE_SIZE;

    static mut PAGE_ALLOC_MEMORY: [MaybeUninit<u8>; PAGE_SIZE * HEAP_PAGES] =
        [const { MaybeUninit::uninit() }; PAGE_SIZE * HEAP_PAGES];
    static PAGE_ALLOC: Spinlock<MetadataPageAllocator> =
        Spinlock::new(MetadataPageAllocator::new());

    struct TestAllocator;
    impl PageAllocator for TestAllocator {
        fn alloc(number_of_pages_requested: usize) -> Option<Range<NonNull<Page>>> {
            PAGE_ALLOC.lock().alloc(number_of_pages_requested)
        }

        fn dealloc(page: NonNull<Page>) -> usize {
            PAGE_ALLOC.lock().dealloc(page)
        }
    }

    fn init_allocator() {
        unsafe {
            PAGE_ALLOC
                .lock()
                .init(&mut *addr_of_mut!(PAGE_ALLOC_MEMORY), &[]);
        }
    }

    fn create_heap() -> SpinlockHeap<TestAllocator> {
        init_allocator();
        SpinlockHeap::<TestAllocator>::new()
    }

    fn alloc<T>(heap: &SpinlockHeap<TestAllocator>) -> *mut T {
        let layout = core::alloc::Layout::new::<T>();
        unsafe { heap.alloc(layout).cast::<T>() }
    }

    fn dealloc<T>(heap: &SpinlockHeap<TestAllocator>, ptr: *mut T) {
        let layout = core::alloc::Layout::new::<T>();
        unsafe { heap.dealloc(ptr.cast::<u8>(), layout) };
    }

    #[test_case]
    fn empty_heap() {
        let heap = create_heap();
        assert!(heap.inner.lock().genesis_block.next.is_none());
    }

    #[test_case]
    fn single_allocation() {
        let heap = create_heap();
        let ptr = alloc::<u8>(&heap);
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(0x42);
        };
        let heap = heap.inner.lock();
        let free_block = unsafe {
            heap.genesis_block
                .next
                .expect("Free list must have a block")
                .as_ref()
        };
        assert!(free_block.next.is_none());
        assert_eq!(
            free_block.size.total_size(),
            PAGE_SIZE - FreeBlock::METADATA_SIZE - FreeBlock::DATA_ALIGNMENT
        );
    }

    #[test_case]
    fn split_block() {
        let heap = create_heap();
        let ptr1 = alloc::<u8>(&heap);
        assert!(!ptr1.is_null());
        unsafe {
            ptr1.write(0x42);
        };

        let ptr2 = alloc::<u8>(&heap);
        assert!(!ptr2.is_null());
        unsafe {
            ptr2.write(0x42);
        };

        let heap = heap.inner.lock();
        let free_block = unsafe {
            heap.genesis_block
                .next
                .expect("Free list must have a block")
                .as_ref()
        };
        assert!(free_block.next.is_none());
        assert_eq!(
            free_block.size.total_size(),
            PAGE_SIZE - (2 * FreeBlock::METADATA_SIZE) - (2 * FreeBlock::DATA_ALIGNMENT)
        );
    }

    #[test_case]
    fn deallocation() {
        let heap = create_heap();
        let ptr = alloc::<u8>(&heap);
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(0x42);
        };

        dealloc(&heap, ptr);
        let heap = heap.inner.lock();
        let free_block1 = unsafe {
            heap.genesis_block
                .next
                .expect("Free list must have a block after dealloc")
                .as_ref()
        };
        assert_eq!(free_block1.size.total_size(), FreeBlock::MINIMUM_SIZE);

        let free_block2 = unsafe {
            free_block1
                .next
                .expect("Free list must have a second block")
                .as_ref()
        };
        assert!(free_block2.next.is_none());
        assert_eq!(
            free_block2.size.total_size(),
            PAGE_SIZE - FreeBlock::METADATA_SIZE - FreeBlock::DATA_ALIGNMENT
        );
    }

    #[test_case]
    fn test_page_allocator_directly() {
        let heap = create_heap();
        let ptr = alloc::<[u8; HEAP_SIZE]>(&heap);
        assert!(!ptr.is_null());
        unsafe {
            core::ptr::write_bytes(ptr.cast::<u8>(), 0x42, HEAP_SIZE);
        }
        dealloc(&heap, ptr);

        let heap_lock = heap.inner.lock();
        assert!(heap_lock.genesis_block.next.is_none());
    }

    #[test_case]
    fn alloc_exhaustion() {
        let heap = create_heap();
        // One page is metadata
        let ptr = alloc::<[u8; HEAP_SIZE]>(&heap);
        assert!(!ptr.is_null());
        unsafe {
            core::ptr::write_bytes(ptr.cast::<u8>(), 0x42, HEAP_SIZE);
        }

        let ptr2 = alloc::<u8>(&heap);
        assert!(ptr2.is_null());

        let heap_lock = heap.inner.lock();
        assert!(heap_lock.genesis_block.next.is_none());
        drop(heap_lock);

        dealloc(&heap, ptr);

        let ptr = alloc::<u8>(&heap);
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(0x42);
        }

        let heap_lock = heap.inner.lock();
        let free_block = unsafe {
            heap_lock
                .genesis_block
                .next
                .expect("Free list must have a block after partial realloc")
                .as_ref()
        };
        assert!(free_block.next.is_none());
        // Because we use the page allocator directly there should be only one page allocated to the heap
        assert_eq!(
            free_block.size.total_size(),
            PAGE_SIZE - FreeBlock::MINIMUM_SIZE
        );
    }
}
