pub use sys::klibc::spinlock::{Spinlock, SpinlockGuard};

#[cfg(test)]
mod tests {
    use core::sync::atomic::Ordering;

    use super::Spinlock;
    use crate::debug;

    #[test_case]
    fn with_lock() {
        let spinlock = Spinlock::new(42);
        let result = spinlock.with_lock(|mut d| {
            *d = 45;
            *d
        });
        assert_eq!(result, 45);
        assert_eq!(spinlock.into_inner(), 45);
    }

    #[test_case]
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

    #[test_case]
    fn force_unlock_allows_reacquire() {
        let spinlock = Spinlock::new(42);
        let lock = spinlock.lock();
        core::mem::forget(lock);
        spinlock.panic_force_unlock();
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
    fn try_with_lock_succeeds() {
        let spinlock = Spinlock::new(42);
        let result = spinlock.try_with_lock(|d| *d);
        assert_eq!(result, Some(42));
    }
}
