/// This trait both abstracts *const T and *mut T
/// It can be used if a method can receive both types of pointers
pub trait Pointer: Clone + Copy + core::fmt::Pointer {
    type Pointee;
    const WRITABLE: bool = false;

    fn as_raw(&self) -> usize;
    fn as_pointer(ptr: usize) -> Self;
}

impl<T> Pointer for *const T {
    type Pointee = T;

    fn as_raw(&self) -> usize {
        *self as usize
    }

    fn as_pointer(ptr: usize) -> Self {
        core::ptr::with_exposed_provenance(ptr)
    }
}

impl<T> Pointer for *mut T {
    type Pointee = T;
    const WRITABLE: bool = true;

    fn as_raw(&self) -> usize {
        *self as usize
    }

    fn as_pointer(ptr: usize) -> Self {
        ptr as *mut T
    }
}
