#![allow(unsafe_code)]
use core::arch::{asm, global_asm};

use crate::cpu;

global_asm!(include_str!("boot.S"), KERNEL_PAGE_TABLES_SATP_OFFSET = const cpu::KERNEL_PAGE_TABLES_SATP_OFFSET);
global_asm!(include_str!("trap.S"), TRAP_FRAME_OFFSET = const cpu::TRAP_FRAME_OFFSET, KERNEL_PAGE_TABLES_SATP_OFFSET = const cpu::KERNEL_PAGE_TABLES_SATP_OFFSET);
global_asm!(include_str!("powersave.S"));
global_asm!(include_str!("panic.S"));

#[cfg(not(miri))]
global_asm!(
    ".pushsection .text",
    ".balign {PAGE_SIZE}",
    "__signal_trampoline:",
    "li a7, {NR_RT_SIGRETURN}",
    "ecall",
    ".skip {PAGE_SIZE} - (. - __signal_trampoline)",
    ".popsection",
    PAGE_SIZE = const crate::memory::PAGE_SIZE,
    NR_RT_SIGRETURN = const headers::syscalls::SYSCALL_NR_RT_SIGRETURN,
);

#[cfg(not(miri))]
pub fn signal_trampoline_phys_addr() -> crate::memory::PhysAddr {
    unsafe extern "C" {
        static __signal_trampoline: u8;
    }
    crate::memory::PhysAddr::new(core::ptr::addr_of!(__signal_trampoline) as usize)
}

#[cfg(miri)]
pub fn signal_trampoline_phys_addr() -> crate::memory::PhysAddr {
    crate::memory::PhysAddr::new(0x1000)
}

pub fn powersave_fn_addr() -> usize {
    unsafe extern "C" {
        fn powersave();
    }
    powersave as *const () as usize
}

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
