#![allow(unsafe_code)]
use core::arch::{asm, global_asm};

use crate::cpu;

global_asm!(include_str!("boot.S"), KERNEL_PAGE_TABLES_SATP_OFFSET = const cpu::KERNEL_PAGE_TABLES_SATP_OFFSET);
global_asm!(include_str!("trap.S"), TRAP_FRAME_OFFSET = const cpu::TRAP_FRAME_OFFSET, KERNEL_PAGE_TABLES_SATP_OFFSET = const cpu::KERNEL_PAGE_TABLES_SATP_OFFSET);
global_asm!(include_str!("powersave.S"));
global_asm!(include_str!("panic.S"));

pub fn asm_panic_rust() {
    let ra: usize;
    // SAFETY: Reads the return address register to report the faulting location.
    unsafe {
        asm!("mv {}, ra", out(reg)ra);
    }
    panic!("Panic from asm code (ra={ra:#x})");
}

#[unsafe(naked)]
pub extern "C" fn wfi_loop() -> ! {
    core::arch::naked_asm!(
        "
        0:
            wfi
            j 0
        "
    )
}
