//! Hardware-free Rust utilities shared across the kernel-side crates.
//!
//! Layering invariant: this crate must not depend on anything that touches
//! hardware (no CSR, no MMIO, no assembly) and must compile on the host
//! (`cargo test` works without cross-compiling). May depend on `common`
//! for shared ABI types.

#![cfg_attr(not(any(miri, test)), no_std)]
#![feature(ptr_mask)]

extern crate alloc;

pub mod array_vec;
pub mod btreemap;
pub mod deconstructed_vec;
pub mod non_empty_vec;
pub mod runtime_initialized;
pub mod send_sync;
pub mod sizes;
pub mod util;
