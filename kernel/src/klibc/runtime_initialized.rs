pub use sys::klibc::runtime_initialized::RuntimeInitializedData;

#[cfg(test)]
mod tests {
    use super::RuntimeInitializedData;

    #[test_case]
    fn check_initialized_value() {
        let runtime_init = RuntimeInitializedData::<u8>::new();
        assert!(!runtime_init.is_initialized());
        runtime_init.initialize(42);
        assert!(runtime_init.is_initialized());
    }

    #[test_case]
    fn check_return_value() {
        let runtime_init = RuntimeInitializedData::<u8>::new();
        runtime_init.initialize(42);
        assert_eq!(*runtime_init, 42);
    }
}
