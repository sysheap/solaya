use core::sync::atomic::{AtomicUsize, Ordering};

static L2_CACHE_BASE: AtomicUsize = AtomicUsize::new(0);

const FLUSH64_OFFSET: usize = 0x200;
const CACHE_LINE_SIZE: usize = 64;

/// Initialize the L2 cache flush facility.
/// `base` is the MMIO base address of the SiFive cache controller.
pub fn init(base: usize) {
    L2_CACHE_BASE.store(base, Ordering::Relaxed);
}

/// Flush (write-back + invalidate) all cache lines covering [start, start+size).
/// Ensures RAM contains the CPU's latest writes and the CPU cache no longer
/// holds stale copies. Used for DMA coherency on non-coherent platforms.
pub fn flush_range(start: usize, size: usize) {
    let base = L2_CACHE_BASE.load(Ordering::Relaxed);
    if base == 0 || size == 0 {
        return;
    }
    let flush64 = (base + FLUSH64_OFFSET) as *mut u64;
    let aligned_start = start & !(CACHE_LINE_SIZE - 1);
    let end = start + size;

    // SAFETY: fence ensures all prior stores are visible before flushing.
    unsafe { core::arch::asm!("fence rw, rw", options(nostack, preserves_flags)) };

    let mut line = aligned_start;
    while line < end {
        // SAFETY: flush64 points to the L2 cache controller FLUSH64 MMIO register,
        // mapped during kernel init. Writing a physical address flushes that cache line.
        unsafe { core::ptr::write_volatile(flush64, line as u64) };
        // SAFETY: fence between flushes ensures each is processed before the next.
        unsafe { core::arch::asm!("fence rw, rw", options(nostack, preserves_flags)) };
        line += CACHE_LINE_SIZE;
    }
}
