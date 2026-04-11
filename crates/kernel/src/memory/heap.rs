use mm::heap::SpinlockHeap;

#[cfg(not(miri))]
#[global_allocator]
static HEAP: SpinlockHeap<super::StaticPageAllocator> = SpinlockHeap::new();
