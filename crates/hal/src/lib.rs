//! Hardware Abstraction Layer for Solaya.
//!
//! Owns everything that touches raw hardware: CSRs, MMIO, cache ops, SBI,
//! the trap cause enum, linker symbols, assembly stubs. Also owns the
//! primitives that are built directly on top of those — `Spinlock`
//! (needs `InterruptGuard`), the per-CPU accessors that read `sscratch`,
//! and the panic-mode interrupt disable wrapper.
//!
//! Layering invariant: may depend on `common`, `klib`. May not depend on
//! device drivers, I/O devices, or anything that owns global state beyond
//! the panic-mode flag.
#![no_std]
#![feature(macro_metavar_expr_concat)]

extern crate alloc;

pub mod isa;
pub mod memory;
pub mod mmio;
pub mod panic_support;
pub mod per_cpu;
pub mod spinlock;
pub mod validated_ptr;

#[cfg(feature = "riscv64")]
mod riscv64;
#[cfg(feature = "riscv64")]
pub use riscv64::*;

#[cfg(not(feature = "riscv64"))]
mod stub;
#[cfg(not(feature = "riscv64"))]
pub use stub::*;

pub use common::cpu::CpuId;
pub use per_cpu::cpu_id;
pub use spinlock::{Spinlock, SpinlockGuard};
