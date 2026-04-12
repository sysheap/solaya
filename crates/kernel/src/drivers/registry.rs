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
use driver_api::{BlockDevice, NetDevice};

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
