#![no_std]
#![feature(macro_metavar_expr_concat)]

pub mod isa;

#[cfg(feature = "riscv64")]
mod riscv64;
#[cfg(feature = "riscv64")]
pub use riscv64::*;

#[cfg(not(feature = "riscv64"))]
mod stub;
#[cfg(not(feature = "riscv64"))]
pub use stub::*;

pub use common::cpu::CpuId;
