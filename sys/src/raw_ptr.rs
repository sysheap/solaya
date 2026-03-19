//! Type interpretation utilities.

/// Interprets a byte slice as a reference to type T.
///
/// # Safety
///
/// Caller must ensure:
/// - The slice is at least `size_of::<T>()` bytes
/// - The data is properly aligned for T
/// - The memory contains a valid T value
pub unsafe fn interpret_bytes_as<T>(bytes: &[u8]) -> &T {
    assert!(bytes.len() >= core::mem::size_of::<T>());
    let ptr: *const T = bytes.as_ptr().cast::<T>();
    assert!(
        ptr.is_aligned(),
        "pointer not aligned for {}",
        core::any::type_name::<T>()
    );
    // SAFETY: Size and alignment are verified by assertions above.
    unsafe { &*ptr }
}

/// Interprets any struct as a byte slice.
///
/// # Safety
///
/// Caller must ensure the type has no padding with uninitialized bytes,
/// or that the caller does not care about them.
pub unsafe fn as_byte_slice<T>(val: &T) -> &[u8] {
    // SAFETY: Any allocated struct can be interpreted as bytes; caller
    // is responsible for ensuring no uninitialized padding is read.
    unsafe {
        core::slice::from_raw_parts((val as *const T).cast::<u8>(), core::mem::size_of_val(val))
    }
}
