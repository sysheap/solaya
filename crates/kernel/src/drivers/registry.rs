//! Typed device registries.
//!
//! Concrete drivers push `Arc<dyn SubsystemTrait>` values into the matching
//! registry after initialization. Kernel subsystems (ext2, devfs, net, ...)
//! read from the registry to discover devices without reaching into
//! driver-specific modules.
//!
//! The registry is a single generic `Registry<T>` parameterised over the
//! device trait object. Per-trait statics are wired via `RegistryKind`; call
//! sites use `<dyn BlockDevice as RegistryKind>::registry()` (or the thin
//! helper [`registry`]).

use alloc::{sync::Arc, vec::Vec};
use driver_api::{BlockDevice, CharDevice, DisplayDevice, InputDevice, NetDevice, RngDevice};

use hal::spinlock::Spinlock;

pub struct Registry<T: ?Sized + Send + Sync> {
    devices: Spinlock<Vec<Arc<T>>>,
}

impl<T: ?Sized + Send + Sync> Registry<T> {
    pub const fn new() -> Self {
        Self {
            devices: Spinlock::new(Vec::new()),
        }
    }

    /// Append a device and return its assigned index.
    pub fn register(&self, device: Arc<T>) -> usize {
        let mut guard = self.devices.lock();
        let index = guard.len();
        guard.push(device);
        index
    }

    pub fn get(&self, index: usize) -> Option<Arc<T>> {
        self.devices.lock().get(index).cloned()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().len()
    }
}

/// Marker trait for device trait objects that have a global `Registry`.
/// One impl per device subsystem supplies the backing static.
pub trait RegistryKind: Send + Sync {
    fn registry() -> &'static Registry<Self>;
}

/// Convenience: `registry::<dyn BlockDevice>()` instead of the angle-bracket
/// `<dyn BlockDevice as RegistryKind>::registry()`.
pub fn registry<T: ?Sized + RegistryKind>() -> &'static Registry<T> {
    T::registry()
}

impl RegistryKind for dyn BlockDevice {
    fn registry() -> &'static Registry<Self> {
        static R: Registry<dyn BlockDevice> = Registry::new();
        &R
    }
}

impl RegistryKind for dyn CharDevice {
    fn registry() -> &'static Registry<Self> {
        static R: Registry<dyn CharDevice> = Registry::new();
        &R
    }
}

impl RegistryKind for dyn DisplayDevice {
    fn registry() -> &'static Registry<Self> {
        static R: Registry<dyn DisplayDevice> = Registry::new();
        &R
    }
}

impl RegistryKind for dyn InputDevice {
    fn registry() -> &'static Registry<Self> {
        static R: Registry<dyn InputDevice> = Registry::new();
        &R
    }
}

impl RegistryKind for dyn RngDevice {
    fn registry() -> &'static Registry<Self> {
        static R: Registry<dyn RngDevice> = Registry::new();
        &R
    }
}

impl RegistryKind for dyn NetDevice {
    fn registry() -> &'static Registry<Self> {
        static R: Registry<dyn NetDevice> = Registry::new();
        &R
    }
}

impl Registry<dyn RngDevice> {
    /// Convenience accessor: first registered RNG, if any.
    pub fn primary() -> Option<Arc<dyn RngDevice>> {
        <dyn RngDevice as RegistryKind>::registry().get(0)
    }
}
