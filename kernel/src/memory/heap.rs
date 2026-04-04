use sys::memory::heap::SpinlockHeap;

#[cfg(not(any(kani, miri)))]
#[global_allocator]
static HEAP: SpinlockHeap<super::StaticPageAllocator> = SpinlockHeap::new();

#[cfg(not(any(kani, miri)))]
pub fn allocated_size() -> usize {
    HEAP.inner.lock().allocated_memory()
}

#[cfg(any(kani, miri))]
pub fn allocated_size() -> usize {
    0
}
