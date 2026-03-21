use alloc::vec::Vec;

pub fn read_validated_slice<T: Clone>(ptr: *const T, len: usize) -> Vec<T> {
    assert!(!ptr.is_null(), "read_validated_slice: null pointer");
    // SAFETY: Caller validated via page table lookup. Pointer is kernel-mapped.
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
    slice.to_vec()
}

pub fn write_validated_slice<T: Copy>(ptr: *mut T, data: &[T]) {
    assert!(!ptr.is_null(), "write_validated_slice: null pointer");
    // SAFETY: Caller validated via page table lookup. Pointer is kernel-mapped.
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr, data.len()) };
    slice.copy_from_slice(data);
}

pub fn read_validated_value<T: Copy>(ptr: *const T) -> T {
    assert!(!ptr.is_null(), "read_validated_value: null pointer");
    // SAFETY: Caller validated via page table lookup. Pointer is kernel-mapped.
    unsafe { ptr.read() }
}

pub fn write_validated_value<T>(ptr: *mut T, value: T) {
    assert!(!ptr.is_null(), "write_validated_value: null pointer");
    // SAFETY: Caller validated via page table lookup. Pointer is kernel-mapped.
    unsafe { ptr.write(value) }
}
