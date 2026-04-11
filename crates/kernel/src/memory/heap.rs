use mm::heap::SpinlockHeap;

#[cfg(not(any(kani, miri)))]
#[global_allocator]
static HEAP: SpinlockHeap<super::StaticPageAllocator> = SpinlockHeap::new();
