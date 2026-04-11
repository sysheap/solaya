#![cfg_attr(not(any(miri, test)), no_std)]

extern crate alloc;

pub mod heap;
pub mod page;
pub mod page_allocator;
pub mod util;

pub use hal::memory::{PAGE_SIZE, PhysAddr, VirtAddr, linker_region_as_uninit_slice};
