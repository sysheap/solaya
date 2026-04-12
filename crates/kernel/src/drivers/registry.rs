//! Typed device registries.
//!
//! Concrete drivers push `Arc<dyn SubsystemTrait>` values into the matching
//! registry after initialization. Kernel subsystems (ext2, devfs, net, ...)
//! read from the registry to discover devices without reaching into
//! driver-specific modules.
//!
//! `BlockDeviceRegistry` landed in Phase 1, `NetDeviceRegistry` in Phase 2.
//! Additional registries (`CharDeviceRegistry`, ...) land in later phases.

use alloc::{sync::Arc, vec::Vec};
use driver_api::{BlockDevice, CharDevice, DisplayDevice, InputDevice, NetDevice, RngDevice};

use crate::klibc::Spinlock;

pub struct BlockDeviceRegistry {
    devices: Spinlock<Vec<Arc<dyn BlockDevice>>>,
}

impl BlockDeviceRegistry {
    const fn new() -> Self {
        Self {
            devices: Spinlock::new(Vec::new()),
        }
    }

    pub fn global() -> &'static Self {
        static REGISTRY: BlockDeviceRegistry = BlockDeviceRegistry::new();
        &REGISTRY
    }

    /// Register a block device and return its assigned index.
    pub fn register(&self, device: Arc<dyn BlockDevice>) -> usize {
        let mut guard = self.devices.lock();
        let index = guard.len();
        guard.push(device);
        index
    }

    pub fn get(&self, index: usize) -> Option<Arc<dyn BlockDevice>> {
        self.devices.lock().get(index).cloned()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().len()
    }
}

pub struct CharDeviceRegistry {
    devices: Spinlock<Vec<Arc<dyn CharDevice>>>,
}

impl CharDeviceRegistry {
    const fn new() -> Self {
        Self {
            devices: Spinlock::new(Vec::new()),
        }
    }

    pub fn global() -> &'static Self {
        static REGISTRY: CharDeviceRegistry = CharDeviceRegistry::new();
        &REGISTRY
    }

    pub fn register(&self, device: Arc<dyn CharDevice>) -> usize {
        let mut guard = self.devices.lock();
        let index = guard.len();
        guard.push(device);
        index
    }

    pub fn get(&self, index: usize) -> Option<Arc<dyn CharDevice>> {
        self.devices.lock().get(index).cloned()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().len()
    }
}

pub struct DisplayDeviceRegistry {
    devices: Spinlock<Vec<Arc<dyn DisplayDevice>>>,
}

impl DisplayDeviceRegistry {
    const fn new() -> Self {
        Self {
            devices: Spinlock::new(Vec::new()),
        }
    }

    pub fn global() -> &'static Self {
        static REGISTRY: DisplayDeviceRegistry = DisplayDeviceRegistry::new();
        &REGISTRY
    }

    pub fn register(&self, device: Arc<dyn DisplayDevice>) -> usize {
        let mut guard = self.devices.lock();
        let index = guard.len();
        guard.push(device);
        index
    }

    pub fn get(&self, index: usize) -> Option<Arc<dyn DisplayDevice>> {
        self.devices.lock().get(index).cloned()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().len()
    }
}

pub struct InputDeviceRegistry {
    devices: Spinlock<Vec<Arc<dyn InputDevice>>>,
}

impl InputDeviceRegistry {
    const fn new() -> Self {
        Self {
            devices: Spinlock::new(Vec::new()),
        }
    }

    pub fn global() -> &'static Self {
        static REGISTRY: InputDeviceRegistry = InputDeviceRegistry::new();
        &REGISTRY
    }

    pub fn register(&self, device: Arc<dyn InputDevice>) -> usize {
        let mut guard = self.devices.lock();
        let index = guard.len();
        guard.push(device);
        index
    }

    pub fn get(&self, index: usize) -> Option<Arc<dyn InputDevice>> {
        self.devices.lock().get(index).cloned()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().len()
    }
}

pub struct RngDeviceRegistry {
    devices: Spinlock<Vec<Arc<dyn RngDevice>>>,
}

impl RngDeviceRegistry {
    const fn new() -> Self {
        Self {
            devices: Spinlock::new(Vec::new()),
        }
    }

    pub fn global() -> &'static Self {
        static REGISTRY: RngDeviceRegistry = RngDeviceRegistry::new();
        &REGISTRY
    }

    pub fn register(&self, device: Arc<dyn RngDevice>) -> usize {
        let mut guard = self.devices.lock();
        let index = guard.len();
        guard.push(device);
        index
    }

    pub fn get(&self, index: usize) -> Option<Arc<dyn RngDevice>> {
        self.devices.lock().get(index).cloned()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().len()
    }

    pub fn primary(&self) -> Option<Arc<dyn RngDevice>> {
        self.devices.lock().first().cloned()
    }
}

pub struct NetDeviceRegistry {
    devices: Spinlock<Vec<Arc<dyn NetDevice>>>,
}

impl NetDeviceRegistry {
    const fn new() -> Self {
        Self {
            devices: Spinlock::new(Vec::new()),
        }
    }

    pub fn global() -> &'static Self {
        static REGISTRY: NetDeviceRegistry = NetDeviceRegistry::new();
        &REGISTRY
    }

    /// Register a network device and return its assigned index.
    pub fn register(&self, device: Arc<dyn NetDevice>) -> usize {
        let mut guard = self.devices.lock();
        let index = guard.len();
        guard.push(device);
        index
    }

    pub fn get(&self, index: usize) -> Option<Arc<dyn NetDevice>> {
        self.devices.lock().get(index).cloned()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().len()
    }
}
