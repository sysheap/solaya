//! Memory management primitives: page allocator, heap, and page types.
//!
//! Generic algorithms — not hardware-specific. Future driver crates can
//! depend on `mm` for memory allocation without pulling in the kernel.
//!
//! Layering invariant: may depend on `hal`, `klib`. May not depend on
//! `console`, `kernel`, or device drivers.
#![cfg_attr(not(any(miri, test)), no_std)]

extern crate alloc;

pub mod heap;
pub mod page;
pub mod page_allocator;
pub mod util;

pub use hal::memory::{PAGE_SIZE, PhysAddr, VirtAddr, linker_region_as_uninit_slice};
