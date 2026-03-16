#![allow(clippy::not_unsafe_ptr_arg_deref)]

//! Safe wrappers for common unsafe pointer operations.
//! Moves the `unsafe` boundary into the sys crate so the kernel
//! can use `#![deny(unsafe_code)]`.

/// Creates a shared slice from a raw pointer and length.
///
/// Caller must ensure:
/// - `ptr` is valid for reads of `len * size_of::<T>()` bytes
/// - The memory is properly initialized
/// - The returned reference's lifetime is valid
pub fn slice_from_raw_parts<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    // SAFETY: Caller guarantees pointer validity and initialization.
    unsafe { core::slice::from_raw_parts(ptr, len) }
}

/// Creates a mutable slice from a raw pointer and length.
///
/// Caller must ensure:
/// - `ptr` is valid for reads and writes of `len * size_of::<T>()` bytes
/// - The memory is properly initialized
/// - No other references to the memory exist
pub fn slice_from_raw_parts_mut<'a, T>(ptr: *mut T, len: usize) -> &'a mut [T] {
    // SAFETY: Caller guarantees pointer validity, initialization, and exclusivity.
    unsafe { core::slice::from_raw_parts_mut(ptr, len) }
}

/// Reads a value from an unaligned pointer.
///
/// Caller must ensure:
/// - `ptr` is valid for reads of `size_of::<T>()` bytes
/// - The memory is properly initialized
pub fn read_unaligned<T>(ptr: *const T) -> T {
    // SAFETY: Caller guarantees pointer validity and initialization.
    unsafe { core::ptr::read_unaligned(ptr) }
}

/// Copies `count` bytes from `src` to `dst`.
///
/// Caller must ensure:
/// - Both `src` and `dst` are valid for the given `count`
/// - The regions do not overlap
pub fn copy_nonoverlapping(src: *const u8, dst: *mut u8, count: usize) {
    // SAFETY: Caller guarantees pointer validity and non-overlap.
    unsafe { core::ptr::copy_nonoverlapping(src, dst, count) }
}

/// Creates a `CStr` from a pointer to a null-terminated string.
///
/// Caller must ensure:
/// - `ptr` is non-null and valid
/// - The string is null-terminated within accessible memory
pub fn cstr_from_ptr<'a>(ptr: *const core::ffi::c_char) -> &'a core::ffi::CStr {
    // SAFETY: Caller guarantees the pointer is valid and null-terminated.
    unsafe { core::ffi::CStr::from_ptr(ptr) }
}

/// Reconstructs a `Vec<u8>` from raw parts.
///
/// Caller must ensure:
/// - `ptr` was originally obtained from `Vec::into_raw_parts` or equivalent
/// - `length` and `capacity` match the original allocation
pub fn vec_from_raw_parts(ptr: *mut u8, length: usize, capacity: usize) -> alloc::vec::Vec<u8> {
    // SAFETY: Caller guarantees the raw parts match a valid Vec allocation.
    unsafe { alloc::vec::Vec::from_raw_parts(ptr, length, capacity) }
}

/// Dereferences a raw pointer to a shared reference.
///
/// Caller must ensure:
/// - `ptr` is non-null, aligned, and valid for the type
/// - The pointed-to value is initialized
pub fn ref_from_raw<'a, T>(ptr: *const T) -> &'a T {
    // SAFETY: Caller guarantees pointer validity and initialization.
    unsafe { &*ptr }
}

/// Dereferences a raw pointer to a mutable reference.
///
/// Caller must ensure:
/// - `ptr` is non-null, aligned, and valid for the type
/// - No other references to the value exist
pub fn mut_from_raw<'a, T>(ptr: *mut T) -> &'a mut T {
    // SAFETY: Caller guarantees pointer validity and exclusivity.
    unsafe { &mut *ptr }
}

/// Reads a value from a raw pointer using volatile semantics.
///
/// Caller must ensure:
/// - `ptr` is valid for reads
pub fn read_volatile<T: Copy>(ptr: *const T) -> T {
    // SAFETY: Caller guarantees pointer validity.
    unsafe { ptr.read_volatile() }
}

/// Writes a value to a raw pointer using volatile semantics.
///
/// Caller must ensure:
/// - `ptr` is valid for writes
pub fn write_volatile<T: Copy>(ptr: *mut T, value: T) {
    // SAFETY: Caller guarantees pointer validity.
    unsafe { ptr.write_volatile(value) }
}

/// Interprets a byte slice as a reference to type T.
///
/// Caller must ensure:
/// - The slice is at least `size_of::<T>()` bytes
/// - The data is properly aligned for T
/// - The memory contains a valid T value
pub fn interpret_bytes_as<T>(bytes: &[u8]) -> &T {
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
/// Caller must ensure:
/// - The type has no padding with uninitialized bytes (or doesn't care about them)
pub fn as_byte_slice<T>(val: &T) -> &[u8] {
    // SAFETY: Any allocated struct can be interpreted as bytes.
    unsafe {
        core::slice::from_raw_parts((val as *const T).cast::<u8>(), core::mem::size_of_val(val))
    }
}

extern crate alloc;
