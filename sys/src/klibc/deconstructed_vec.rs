use alloc::vec::Vec;

/// A Vec<u8> decomposed into its raw parts for safe storage across FFI/DMA
/// boundaries. Safe by RAII: from_vec captures the raw parts, into_vec_with_len
/// reconstructs with a bounds check.
pub struct DeconstructedVec {
    ptr: *mut u8,
    length: usize,
    capacity: usize,
}

// SAFETY: A deconstructed Vec<u8> can be sent to other threads just like Vec<u8>.
unsafe impl Send for DeconstructedVec {}

impl DeconstructedVec {
    pub fn from_vec(vec: Vec<u8>) -> Self {
        let (ptr, length, capacity) = vec.into_raw_parts();
        Self {
            ptr,
            length,
            capacity,
        }
    }

    pub fn into_vec_with_len(self, length: usize) -> Vec<u8> {
        assert!(
            length <= self.capacity,
            "Length must be smaller or equal capacity"
        );
        // SAFETY: ptr/capacity were obtained from Vec::into_raw_parts in
        // from_vec. length is bounds-checked above.
        let vec = unsafe { Vec::from_raw_parts(self.ptr, length, self.capacity) };
        core::mem::forget(self);
        vec
    }

    pub fn length(&self) -> usize {
        self.length
    }
}

impl Drop for DeconstructedVec {
    fn drop(&mut self) {
        // SAFETY: Reconstruct the original Vec to free the allocation.
        unsafe {
            let _ = Vec::from_raw_parts(self.ptr, self.length, self.capacity);
        }
    }
}
