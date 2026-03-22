use core::{cell::UnsafeCell, mem::MaybeUninit, ops::Deref, sync::atomic::AtomicBool};

pub struct RuntimeInitializedData<T> {
    initialized: AtomicBool,
    data: UnsafeCell<MaybeUninit<T>>,
}

// SAFETY: After initialization (SeqCst write), the inner data is only read
// through &self. The AtomicBool ensures the write happens-before any read.
// T: Sync is required because we hand out &T to multiple threads.
unsafe impl<T: Sync> Sync for RuntimeInitializedData<T> {}

impl<T> RuntimeInitializedData<T> {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            data: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn initialize(&self, value: T) {
        if self
            .initialized
            .swap(true, core::sync::atomic::Ordering::SeqCst)
        {
            panic!("RuntimeInitializedData already initialized");
        }
        // SAFETY: The atomic swap above guarantees we are the only writer.
        // No reader can exist yet because initialized was false before the swap.
        unsafe {
            self.data.get().write(MaybeUninit::new(value));
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(core::sync::atomic::Ordering::SeqCst)
    }
}

impl<T> Deref for RuntimeInitializedData<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        assert!(
            self.initialized.load(core::sync::atomic::Ordering::SeqCst),
            "RuntimeInitializedData not initialized",
        );
        // SAFETY: The assert above confirms initialization has completed.
        // After SeqCst store in initialize(), the data is immutable.
        unsafe { (*self.data.get()).assume_init_ref() }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeInitializedData;

    #[test]
    fn check_initialized_value() {
        let runtime_init = RuntimeInitializedData::<u8>::new();
        assert!(!runtime_init.is_initialized());
        runtime_init.initialize(42);
        assert!(runtime_init.is_initialized());
    }

    #[test]
    fn check_return_value() {
        let runtime_init = RuntimeInitializedData::<u8>::new();
        runtime_init.initialize(42);
        assert_eq!(*runtime_init, 42);
    }
}
