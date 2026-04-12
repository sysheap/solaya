//! Hardware-free Rust utilities shared across the kernel-side crates.
//!
//! Layering invariant: may depend on `abi`. May not touch CSRs, MMIO,
//! assembly, or statics that represent hardware state.

#![cfg_attr(not(any(miri, test)), no_std)]
#![feature(ptr_mask)]

extern crate alloc;

pub mod array_vec;
pub mod big_endian;
pub mod btreemap;
pub mod deconstructed_vec;
pub mod non_empty_vec;
pub mod parser;
pub mod runtime_initialized;
pub mod send_sync;
pub mod sizes;
pub mod util;
