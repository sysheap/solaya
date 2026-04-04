#![cfg_attr(not(miri), no_std)]
#![cfg_attr(kani, feature(maybe_uninit_slice))]
#![cfg_attr(not(feature = "riscv64"), allow(dead_code))]
#![cfg_attr(not(feature = "riscv64"), allow(unused_imports))]
#![cfg_attr(not(feature = "riscv64"), allow(unused_macros))]
#![feature(ptr_mask)]
#![feature(str_from_raw_parts)]
#![feature(macro_metavar_expr_concat)]
#![feature(macro_metavar_expr)]

extern crate alloc;

#[cfg(feature = "riscv64")]
mod asm;
pub mod cpu;
pub mod io;
pub mod klibc;
pub mod logging;
pub mod memory;
pub mod panic_support;

pub use klibc::{
    mmio::MMIO,
    spinlock::{Spinlock, SpinlockGuard},
};
