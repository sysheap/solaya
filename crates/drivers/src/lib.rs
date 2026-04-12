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

extern crate alloc;

pub mod bochs_display;
