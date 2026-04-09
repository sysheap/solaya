use crate::mmio::MMIO;
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
    let mut flush64: MMIO<u64> = MMIO::new(base + FLUSH64_OFFSET);
    let aligned_start = start & !(CACHE_LINE_SIZE - 1);
    let end = start + size;

    let mut line = aligned_start;
    while line < end {
        flush64.write(line as u64);
        line += CACHE_LINE_SIZE;
    }
}
