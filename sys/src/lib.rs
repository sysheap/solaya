#![no_std]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]
#![cfg_attr(not(target_arch = "riscv64"), allow(unused_imports))]
#![cfg_attr(not(target_arch = "riscv64"), allow(unused_macros))]
#![cfg_attr(kani, feature(maybe_uninit_slice))]
#![feature(ptr_mask)]
#![feature(str_from_raw_parts)]
#![feature(macro_metavar_expr_concat)]

extern crate alloc;

pub mod cpu;
pub mod io;
pub mod klibc;
pub mod logging;
pub mod memory;

pub use klibc::{
    mmio::MMIO,
    spinlock::{Spinlock, SpinlockGuard},
};
