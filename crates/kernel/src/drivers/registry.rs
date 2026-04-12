//! Typed device registries.
//!
//! Concrete drivers push `Arc<dyn SubsystemTrait>` values into the matching
//! registry after initialization. Kernel subsystems (ext2, devfs, net, ...)
//! read from the registry to discover devices without reaching into
//! driver-specific modules.
//!
//! Only `BlockDeviceRegistry` exists in Phase 1. Additional registries
//! (`NetDeviceRegistry`, `CharDeviceRegistry`, ...) land in later phases.

use alloc::{sync::Arc, vec::Vec};
use driver_api::BlockDevice;

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
