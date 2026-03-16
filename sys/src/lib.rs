#![no_std]
#![feature(macro_metavar_expr_concat)]

#[cfg(all(target_arch = "riscv64", not(miri)))]
mod riscv64;
#[cfg(all(target_arch = "riscv64", not(miri)))]
pub use riscv64::*;

#[cfg(any(not(target_arch = "riscv64"), miri))]
mod stub;
#[cfg(any(not(target_arch = "riscv64"), miri))]
pub use stub::*;

pub mod array_vec;
pub mod runtime_initialized;
pub mod spinlock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuId(usize);

impl CpuId {
    pub fn from_hart_id(hart_id: usize) -> Self {
        Self(hart_id)
    }

    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl core::fmt::Display for CpuId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}
