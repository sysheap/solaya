use core::{
    cell::UnsafeCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

const NO_OWNER: usize = usize::MAX;

#[derive(Debug)]
pub struct Spinlock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
    owner_cpu: AtomicUsize,
}

impl<T> Spinlock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
            owner_cpu: AtomicUsize::new(NO_OWNER),
        }
    }

    pub fn with_lock<'a, R>(&'a self, f: impl FnOnce(SpinlockGuard<'a, T>) -> R) -> R {
        let lock = self.lock();
        f(lock)
    }

    pub fn try_with_lock<'a, R>(&'a self, f: impl FnOnce(SpinlockGuard<'a, T>) -> R) -> Option<R> {
        let interrupt_guard = arch::cpu::InterruptGuard::new();
        let value = self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed);
        if value.is_ok() {
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
        let interrupt_guard = arch::cpu::InterruptGuard::new();
        self.detect_same_cpu_deadlock();
        let mut spin_count: u64 = 0;
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
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
        if self.locked.load(Ordering::Relaxed) {
            let cpu_id = crate::cpu::cpu_id().as_usize();
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
            let cpu_id = crate::cpu::cpu_id();
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
            .store(crate::cpu::cpu_id().as_usize(), Ordering::Relaxed);
    }

    #[cfg(any(not(target_arch = "riscv64"), miri))]
    fn set_owner(&self) {}

    fn clear_owner(&self) {
        self.owner_cpu.store(NO_OWNER, Ordering::Relaxed);
    }

    /// # Safety
    /// Forcibly releases the lock regardless of who holds it.
    /// Only safe during panic when the holder will never resume.
    pub unsafe fn force_unlock(&self) {
        self.owner_cpu.store(NO_OWNER, Ordering::Relaxed);
        self.locked.store(false, Ordering::Release);
    }

    /// Forcibly unlocks during a panic. The lock holder will never resume.
    pub fn panic_force_unlock(&self) {
        // SAFETY: Called during panic — the lock holder will never resume.
        unsafe { self.force_unlock() }
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

// SAFETY: Spinlock provides mutual exclusion via an atomic lock, so it is safe
// to share across threads as long as the inner type can be sent between threads.
unsafe impl<T: Send> Sync for Spinlock<T> {}
// SAFETY: Same reasoning as Sync — the Spinlock serializes all access.
unsafe impl<T: Send> Send for Spinlock<T> {}

pub struct SpinlockGuard<'a, T> {
    spinlock: &'a Spinlock<T>,
    _interrupt_guard: arch::cpu::InterruptGuard,
}

impl<T> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.spinlock.clear_owner();
        self.spinlock.locked.store(false, Ordering::Release);
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

#[cfg(test)]
mod tests {
    use super::Spinlock;
    use alloc::format;

    #[test]
    fn with_lock() {
        let spinlock = Spinlock::new(42);
        let result = spinlock.with_lock(|mut d| {
            *d = 45;
            *d
        });
        assert_eq!(result, 45);
        assert_eq!(spinlock.into_inner(), 45);
    }

    #[test]
    fn check_lock_and_unlock() {
        let spinlock = Spinlock::new(42);
        {
            let mut locked = spinlock.lock();
            *locked = 1;
        }
        {
            let mut locked = spinlock.lock();
            assert_eq!(*locked, 1);
            *locked = 42;
            assert_eq!(*locked, 42);
        }
        assert_eq!(spinlock.into_inner(), 42);
    }

    #[test]
    fn force_unlock_allows_reacquire() {
        let spinlock = Spinlock::new(42);
        let lock = spinlock.lock();
        core::mem::forget(lock);
        spinlock.panic_force_unlock();
        let _lock2 = spinlock.lock();
    }

    #[test]
    fn print_doesnt_deadlock() {
        let spinlock = Spinlock::new(42);
        let _ = format!("{spinlock:?}");
        let spinlock_guard = spinlock.lock();
        let _ = format!("{spinlock_guard:?}");
    }

    #[test]
    fn try_with_lock_succeeds() {
        let spinlock = Spinlock::new(42);
        let result = spinlock.try_with_lock(|d| *d);
        assert_eq!(result, Some(42));
    }
}
