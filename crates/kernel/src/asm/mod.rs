pub use arch::{
    cpu::{asm_panic_rust, wfi_loop},
    linker_symbols::powersave_fn_addr,
};

use crate::memory::PhysAddr;

pub fn signal_trampoline_phys_addr() -> PhysAddr {
    PhysAddr::new(arch::linker_symbols::signal_trampoline_addr())
}
