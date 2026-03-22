use sys::memory::heap::SpinlockHeap;

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
