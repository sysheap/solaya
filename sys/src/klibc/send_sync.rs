use core::ops::{Deref, DerefMut};

/// Wrapper that asserts a type is safe to Send/Sync.
/// The caller must ensure the wrapped type is actually thread-safe
/// (e.g., access is serialized through a Spinlock).
pub struct AssertSendSync<T>(pub T);

// SAFETY: The caller guarantees thread-safety (e.g., via external Spinlock).
unsafe impl<T> Send for AssertSendSync<T> {}
// SAFETY: The caller guarantees thread-safety (e.g., via external Spinlock).
unsafe impl<T> Sync for AssertSendSync<T> {}

impl<T> Deref for AssertSendSync<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for AssertSendSync<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
