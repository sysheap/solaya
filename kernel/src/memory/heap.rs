use sys::memory::heap::SpinlockHeap;

#[cfg(all(target_arch = "riscv64", not(miri)))]
#[global_allocator]
static HEAP: SpinlockHeap<super::StaticPageAllocator> = SpinlockHeap::new();
