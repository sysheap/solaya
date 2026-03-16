use core::{
    fmt::Debug,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(all(target_arch = "riscv64", not(miri)))]
use crate::cpu::Cpu;

const NO_OWNER: usize = usize::MAX;

pub struct Spinlock<T> {
    inner: sys::spinlock::Spinlock<T>,
    owner_cpu: AtomicUsize,
}

impl<T: Debug> Debug for Spinlock<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.inner)
    }
}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            inner: sys::spinlock::Spinlock::new(data),
            owner_cpu: AtomicUsize::new(NO_OWNER),
        }
    }

    pub fn with_lock<'a, R>(&'a self, f: impl FnOnce(SpinlockGuard<'a, T>) -> R) -> R {
        let lock = self.lock();
        f(lock)
    }

    pub fn try_with_lock<'a, R>(&'a self, f: impl FnOnce(SpinlockGuard<'a, T>) -> R) -> Option<R> {
        let interrupt_guard = sys::cpu::InterruptGuard::new();
        if self.inner.try_acquire() {
            self.set_owner();
            let lock = SpinlockGuard {
                spinlock: self,
                _interrupt_guard: interrupt_guard,
            };
            return Some(f(lock));
        }
        None
    }

    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        let interrupt_guard = sys::cpu::InterruptGuard::new();
        self.detect_same_cpu_deadlock();
        let mut spin_count: u64 = 0;
        while !self.inner.try_acquire() {
            spin_count += 1;
            self.warn_possible_deadlock(spin_count);
            core::hint::spin_loop();
        }
        self.set_owner();
        SpinlockGuard {
            spinlock: self,
            _interrupt_guard: interrupt_guard,
        }
    }

    #[cfg(all(target_arch = "riscv64", not(miri)))]
    fn detect_same_cpu_deadlock(&self) {
        if self.inner.is_locked() {
            let cpu_id = Cpu::cpu_id().as_usize();
            assert_ne!(
                self.owner_cpu.load(Ordering::Relaxed),
                cpu_id,
                "Spinlock deadlock: CPU {cpu_id} tried to re-acquire a lock it already holds"
            );
        }
    }

    #[cfg(any(not(target_arch = "riscv64"), miri))]
    fn detect_same_cpu_deadlock(&self) {}

    #[cfg(all(target_arch = "riscv64", not(miri)))]
    fn warn_possible_deadlock(&self, spin_count: u64) {
        if spin_count.is_multiple_of(10_000_000) {
            let cpu_id = Cpu::cpu_id();
            let owner = self.owner_cpu.load(Ordering::Relaxed);
            crate::warn!(
                "Spinlock likely deadlocked: CPU {} waiting for lock held by CPU {} ({} spins)",
                cpu_id,
                owner,
                spin_count
            );
        }
    }

    #[cfg(any(not(target_arch = "riscv64"), miri))]
    fn warn_possible_deadlock(&self, _spin_count: u64) {}

    #[cfg(all(target_arch = "riscv64", not(miri)))]
    fn set_owner(&self) {
        self.owner_cpu
            .store(Cpu::cpu_id().as_usize(), Ordering::Relaxed);
    }

    #[cfg(any(not(target_arch = "riscv64"), miri))]
    fn set_owner(&self) {}

    fn clear_owner(&self) {
        self.owner_cpu.store(NO_OWNER, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    /// # Safety
    /// Forcibly releases the lock regardless of who holds it.
    /// Only safe during panic when the holder will never resume.
    pub unsafe fn force_unlock(&self) {
        self.owner_cpu.store(NO_OWNER, Ordering::Relaxed);
        // SAFETY: Caller guarantees this is only used during panic
        unsafe { self.inner.force_unlock() };
    }
}

// SAFETY: Spinlock provides mutual exclusion via an atomic lock, so it is safe
// to share across threads as long as the inner type can be sent between threads.
unsafe impl<T: Send> Sync for Spinlock<T> {}
// SAFETY: Same reasoning as Sync — the Spinlock serializes all access.
unsafe impl<T: Send> Send for Spinlock<T> {}

pub struct SpinlockGuard<'a, T> {
    spinlock: &'a Spinlock<T>,
    _interrupt_guard: sys::cpu::InterruptGuard,
}

impl<T> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.spinlock.clear_owner();
        self.spinlock.inner.release();
    }
}

impl<T> Deref for SpinlockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: SpinlockGuard has exclusive access; the lock is held.
        unsafe { &*self.spinlock.inner.data_ptr() }
    }
}

impl<T> DerefMut for SpinlockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: SpinlockGuard has exclusive access; the lock is held.
        unsafe { &mut *self.spinlock.inner.data_ptr() }
    }
}

impl<T: Debug> Debug for SpinlockGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // SAFETY: SpinlockGuard has exclusive access; the lock is held.
        unsafe {
            writeln!(
                f,
                "SpinlockGuard {{\n{:?}\n}}",
                *self.spinlock.inner.data_ptr()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::Ordering;

    use super::{NO_OWNER, Spinlock};
    use crate::debug;

    #[test_case]
    fn with_lock() {
        let spinlock = Spinlock::new(42);
        assert!(!spinlock.inner.is_locked());
        let result = spinlock.with_lock(|mut d| {
            *d = 45;
            *d
        });
        assert!(!spinlock.inner.is_locked());
        assert_eq!(result, 45);
    }

    #[test_case]
    fn check_lock_and_unlock() {
        let spinlock = Spinlock::new(42);
        assert!(!spinlock.inner.is_locked());
        {
            let mut locked = spinlock.lock();
            assert!(spinlock.inner.is_locked());
            *locked = 1;
        }
        assert!(!spinlock.inner.is_locked());
        assert_eq!(*spinlock.lock(), 1);
        let mut locked = spinlock.lock();
        *locked = 42;
        assert!(spinlock.inner.is_locked());
        assert_eq!(*locked, 42);
    }

    #[test_case]
    fn force_unlock_allows_reacquire() {
        let spinlock = Spinlock::new(42);
        let lock = spinlock.lock();
        core::mem::forget(lock);
        unsafe {
            spinlock.force_unlock();
        }
        let _lock2 = spinlock.lock();
    }

    #[test_case]
    fn print_doesnt_deadlock() {
        let spinlock = Spinlock::new(42);
        debug!("{spinlock:?}");
        let spinlock_guard = spinlock.lock();
        debug!("{spinlock_guard:?}");
    }

    #[test_case]
    fn owner_cpu_cleared_after_unlock() {
        let spinlock = Spinlock::new(42);
        assert_eq!(spinlock.owner_cpu.load(Ordering::Relaxed), NO_OWNER);
        {
            let _lock = spinlock.lock();
        }
        assert_eq!(spinlock.owner_cpu.load(Ordering::Relaxed), NO_OWNER);
    }

    #[test_case]
    fn try_with_lock_clears_owner() {
        let spinlock = Spinlock::new(42);
        spinlock.try_with_lock(|_| {});
        assert_eq!(spinlock.owner_cpu.load(Ordering::Relaxed), NO_OWNER);
    }
}
