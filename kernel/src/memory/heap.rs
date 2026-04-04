use sys::memory::heap::SpinlockHeap;

#[cfg(all(feature = "riscv64", not(miri)))]
#[global_allocator]
static HEAP: SpinlockHeap<super::StaticPageAllocator> = SpinlockHeap::new();

#[cfg(all(feature = "riscv64", not(miri)))]
pub fn allocated_size() -> usize {
    HEAP.inner.lock().allocated_memory()
}

#[cfg(any(not(feature = "riscv64"), miri))]
pub fn allocated_size() -> usize {
    0
}
