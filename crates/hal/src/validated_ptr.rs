use alloc::vec::Vec;
use common::pointer::Pointer;
use headers::errno::Errno;

pub trait PtrValidator {
    fn validate_userspace<PTR: Pointer>(&self, ptr: PTR, len: usize) -> Result<PTR, Errno>;
    fn validate_kernel<PTR: Pointer>(&self, ptr: PTR, len: usize) -> Result<PTR, Errno>;
}

pub struct ValidatedPtr<T> {
    ptr: *mut T,
}

impl<T> ValidatedPtr<T> {
    pub fn from_userspace(
        raw: impl Pointer,
        len: usize,
        validator: &impl PtrValidator,
    ) -> Result<Self, Errno> {
        let translated = validator.validate_userspace(raw, len)?;
        Ok(Self {
            ptr: translated.as_raw() as *mut T,
        })
    }

    pub fn from_kernel(
        raw: impl Pointer,
        len: usize,
        validator: &impl PtrValidator,
    ) -> Result<Self, Errno> {
        validator.validate_kernel(raw, len)?;
        Ok(Self {
            ptr: raw.as_raw() as *mut T,
        })
    }

    pub fn from_trusted(ptr: *const T) -> Self {
        assert!(!ptr.is_null());
        assert!(ptr.is_aligned());
        Self { ptr: ptr as *mut T }
    }
}

impl<T: Copy> ValidatedPtr<T> {
    pub fn read(&self) -> T {
        // SAFETY: Pointer was validated at construction time.
        unsafe { self.ptr.cast_const().read() }
    }

    pub fn write_slice(&self, data: &[T]) {
        // SAFETY: Pointer was validated at construction time for the required length.
        let slice = unsafe { core::slice::from_raw_parts_mut(self.ptr, data.len()) };
        slice.copy_from_slice(data);
    }
}

impl<T> ValidatedPtr<T> {
    pub fn write(&self, value: T) {
        // SAFETY: Pointer was validated at construction time.
        unsafe { self.ptr.write(value) }
    }

    /// Returns a static reference to the pointed-to data.
    /// Only valid for pointers to statically-allocated memory (firmware blobs,
    /// linker regions, leaked allocations). The caller must ensure the memory
    /// outlives 'static.
    pub fn as_static_ref(&self) -> &'static T {
        // SAFETY: Pointer was validated at construction time. Caller guarantees
        // the memory is statically allocated.
        unsafe { &*self.ptr.cast_const() }
    }

    /// Returns a static slice of the pointed-to data.
    /// Only valid for pointers to statically-allocated memory.
    pub fn as_static_slice(&self, len: usize) -> &'static [T] {
        // SAFETY: Pointer was validated at construction time. Caller guarantees
        // the memory is statically allocated and valid for `len` elements.
        unsafe { core::slice::from_raw_parts(self.ptr.cast_const(), len) }
    }
}

impl<T: Clone> ValidatedPtr<T> {
    pub fn read_slice(&self, len: usize) -> Vec<T> {
        // SAFETY: Pointer was validated at construction time for the required length.
        let slice = unsafe { core::slice::from_raw_parts(self.ptr.cast_const(), len) };
        slice.to_vec()
    }
}
