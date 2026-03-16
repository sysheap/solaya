use core::{
    cell::UnsafeCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct Spinlock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    pub fn try_acquire(&self) -> bool {
        self.locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    pub fn release(&self) {
        self.locked.store(false, Ordering::Release);
    }

    /// Returns a raw pointer to the inner data.
    /// # Safety
    /// Caller must hold the lock.
    pub fn data_ptr(&self) -> *mut T {
        self.data.get()
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    /// # Safety
    /// Forcibly releases the lock regardless of who holds it.
    /// Only safe during panic when the holder will never resume.
    pub unsafe fn force_unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

impl<T: Debug> Debug for Spinlock<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(guard) = self.try_lock() {
            write!(f, "Spinlock {{ {:?} }}", &*guard)
        } else {
            write!(f, "Spinlock {{ <locked> }}")
        }
    }
}

// SAFETY: Spinlock provides mutual exclusion via an atomic lock, so it is safe
// to share across threads as long as the inner type can be sent between threads.
unsafe impl<T: Send> Sync for Spinlock<T> {}
// SAFETY: Same reasoning as Sync — the Spinlock serializes all access.
unsafe impl<T: Send> Send for Spinlock<T> {}

pub struct SpinlockGuard<'a, T> {
    spinlock: &'a Spinlock<T>,
}

impl<T> Spinlock<T> {
    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        while !self.try_acquire() {
            core::hint::spin_loop();
        }
        SpinlockGuard { spinlock: self }
    }

    pub fn try_lock(&self) -> Option<SpinlockGuard<'_, T>> {
        if self.try_acquire() {
            Some(SpinlockGuard { spinlock: self })
        } else {
            None
        }
    }
}

impl<T> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.spinlock.release();
    }
}

impl<T> Deref for SpinlockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: SpinlockGuard has exclusive access to the data
        unsafe { &*self.spinlock.data.get() }
    }
}

impl<T> DerefMut for SpinlockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: SpinlockGuard has exclusive access to the data
        unsafe { &mut *self.spinlock.data.get() }
    }
}

impl<T: Debug> Debug for SpinlockGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // SAFETY: SpinlockGuard has exclusive access to the data
        unsafe { writeln!(f, "SpinlockGuard {{\n{:?}\n}}", *self.spinlock.data.get()) }
    }
}
