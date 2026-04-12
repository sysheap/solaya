//! Concrete device drivers for Solaya.
//!
//! Each submodule implements one driver against the `driver-api` traits
//! (`BlockDevice`, `NetDevice`, `DisplayDevice`, `InputDevice`, `RngDevice`)
//! and the `BusContext` surface. The kernel (`solaya`) enumerates devices
//! on each bus and calls into each driver's `initialize` entry point; no
//! driver reaches back into kernel internals.
//!
//! Layering invariant: depends only on `driver-api`, `hal`, `mm`, `console`,
//! `abi`, `headers`, `klib`. Never on `solaya`.
//!
//! Unsafe policy: `#![deny(unsafe_op_in_unsafe_fn)]` (not `forbid`) — drivers
//! need `unsafe` for MMIO / DMA address manipulation. Each `unsafe` block
//! carries a `// SAFETY:` comment. The kernel crate stays
//! `#![forbid(unsafe_code)]`.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![feature(macro_metavar_expr_concat)]

extern crate alloc;

pub mod bochs_display;
pub mod dwmac;
pub mod virtio;

use alloc::boxed::Box;
use driver_api::DriverCatalog;

/// Register every built-in driver factory with `catalog`. Insertion order
/// determines probe precedence when two factories claim the same device.
/// DWMAC is intentionally omitted — it's device-tree-walked, not
/// PCI-enumerated, so it has its own bring-up path in the kernel.
pub fn register_builtin(catalog: &mut DriverCatalog) {
    catalog.register(Box::new(virtio::block::VirtioBlockFactory));
    catalog.register(Box::new(virtio::net::VirtioNetFactory));
    catalog.register(Box::new(virtio::input::VirtioInputFactory));
    catalog.register(Box::new(virtio::rng::VirtioRngFactory));
    catalog.register(Box::new(bochs_display::BochsDisplayFactory));
}
