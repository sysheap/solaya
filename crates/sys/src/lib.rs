#![cfg_attr(not(any(miri, test)), no_std)]
#![cfg_attr(kani, feature(maybe_uninit_slice))]
#![cfg_attr(kani, allow(dead_code))]
#![cfg_attr(kani, allow(unused_imports))]
#![cfg_attr(kani, allow(unused_macros))]
#![feature(ptr_mask)]
#![feature(str_from_raw_parts)]
#![feature(macro_metavar_expr_concat)]
#![feature(macro_metavar_expr)]

extern crate alloc;

#[cfg(not(kani))]
mod asm;
pub mod klibc;
pub mod memory;

pub use hal::{Spinlock, SpinlockGuard, cpu_id, mmio, mmio::MMIO, panic_support, per_cpu as cpu};
