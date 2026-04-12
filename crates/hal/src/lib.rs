//! Hardware Abstraction Layer for Solaya.
//!
//! Owns everything that touches raw hardware: CSRs, MMIO, cache ops, SBI,
//! the trap cause enum, linker symbols, assembly stubs, and the RISC-V Sv39
//! page table types. Also owns primitives built directly on top of
//! hardware — `Spinlock` (needs `InterruptGuard`), the per-CPU accessors
//! that read `sscratch`, `ValidatedPtr`, and the panic-mode interrupt
//! disable wrapper.
//!
//! Layering invariant: may depend on `abi`, `klib`, `headers`. May not
//! depend on device drivers, I/O devices, `console`, `mm`, or `kernel`.
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

pub use abi::cpu::CpuId;
pub use per_cpu::cpu_id;
pub use spinlock::{Spinlock, SpinlockGuard};

/// Declare an MMIO-mapped register layout. For every field, a getter method
/// on `MMIO<Self>` returns an `MMIO<FieldType>` pointing at the same address
/// plus the field's byte offset, so callers can read/write each register
/// independently with the correct access width.
#[macro_export]
macro_rules! mmio_struct {
    {
        $(#[$meta:meta])*
        struct $name:ident {
            $($field_name:ident : $field_type:ty),* $(,)?
        }
    } => {
            $(#[$meta])*
            #[derive(Clone, Copy, Debug)]
            #[allow(non_camel_case_types, dead_code)]
            pub struct $name {
                $(
                    $field_name: $field_type,
                )*
            }

            #[allow(non_camel_case_types, dead_code)]
            pub trait ${concat($name, Fields)} {
                $(
                    fn $field_name(&self) -> $crate::mmio::MMIO<$field_type>;
                )*
            }

            impl ${concat($name, Fields)} for $crate::mmio::MMIO<$name> {
                $(
                    fn $field_name(&self) -> $crate::mmio::MMIO<$field_type> {
                        self.new_type_with_offset(core::mem::offset_of!($name, $field_name))
                    }
                )*
            }
        };
}
