//! Driver catalog — bus-agnostic probe/attach registration.
//!
//! A [`DriverFactory`] wraps one concrete driver's probe + attach entry
//! points. [`DriverCatalog`] owns a `Vec` of factories and walks them for
//! a given [`BusContext`]; the first factory whose `probe` returns `true`
//! gets its `attach` called. The returned [`DriverInstance`] carries the
//! typed `Arc<dyn …>` which the kernel routes into the matching registry.
//!
//! The kernel builds the catalog once at boot via
//! `drivers::register_builtin(&mut catalog)` and then loops over the
//! enumerated PCI devices asking the catalog to attach each one.

use alloc::{boxed::Box, sync::Arc, vec::Vec};

use crate::{
    BlockDevice, BusContext, CharDevice, DisplayDevice, InputDevice, NetDevice, ProbeError,
    RngDevice,
};

/// Typed outcome of a successful `DriverFactory::attach`. The kernel routes
/// each variant into the matching registry.
pub enum DriverInstance {
    Block(Arc<dyn BlockDevice>),
    Net(Arc<dyn NetDevice>),
    Char(Arc<dyn CharDevice>),
    Display(Arc<dyn DisplayDevice>),
    Input(Arc<dyn InputDevice>),
    Rng(Arc<dyn RngDevice>),
}

/// One concrete driver's registration entry.
///
/// `probe` is cheap and side-effect-free — it inspects IDs / capabilities
/// on the bus and reports whether this driver claims the device.
/// `attach` does the full initialization (DMA setup, IRQ registration,
/// virtqueue wiring, handle construction) and returns the typed
/// `DriverInstance`.
pub trait DriverFactory: Send + Sync {
    /// Human-readable driver name, used for logging.
    fn name(&self) -> &'static str;

    /// Return `true` if this driver matches the device exposed by `bus`.
    fn probe(&self, bus: &dyn BusContext) -> bool;

    /// Initialize the device and return the typed handle.
    fn attach(&self, bus: &dyn BusContext) -> Result<DriverInstance, ProbeError>;
}

/// Ordered list of registered drivers. Factories are tried in insertion
/// order; the first `probe` match wins.
pub struct DriverCatalog {
    factories: Vec<Box<dyn DriverFactory>>,
}

impl DriverCatalog {
    pub fn new() -> Self {
        Self {
            factories: Vec::new(),
        }
    }

    /// Append a factory to the catalog. Registration order determines
    /// probe precedence.
    pub fn register(&mut self, factory: Box<dyn DriverFactory>) {
        self.factories.push(factory);
    }

    /// Walk the catalog; on the first `probe` match, call `attach` and
    /// return its result. Returns `None` when no factory claims the
    /// device.
    pub fn attach_first_match(
        &self,
        bus: &dyn BusContext,
    ) -> Option<Result<DriverInstance, ProbeError>> {
        for factory in &self.factories {
            if factory.probe(bus) {
                return Some(factory.attach(bus));
            }
        }
        None
    }
}

impl Default for DriverCatalog {
    fn default() -> Self {
        Self::new()
    }
}
