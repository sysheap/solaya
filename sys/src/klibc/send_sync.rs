use core::ops::{Deref, DerefMut};

/// Wrapper that asserts a type is safe to Send/Sync.
/// The caller must ensure the wrapped type is actually thread-safe
/// (e.g., access is serialized through a Spinlock).
pub struct UnsafeSendSync<T>(pub T);

impl<T: core::fmt::Debug> core::fmt::Debug for UnsafeSendSync<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: Clone> Clone for UnsafeSendSync<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Copy> Copy for UnsafeSendSync<T> {}

// SAFETY: The caller guarantees thread-safety (e.g., via external Spinlock).
unsafe impl<T> Send for UnsafeSendSync<T> {}
// SAFETY: The caller guarantees thread-safety (e.g., via external Spinlock).
unsafe impl<T> Sync for UnsafeSendSync<T> {}

impl<T> Deref for UnsafeSendSync<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for UnsafeSendSync<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
