use abi::cpu::{KERNEL_PAGE_TABLES_SATP_OFFSET, TRAP_FRAME_OFFSET};
use core::arch::global_asm;

global_asm!(
    include_str!("boot.S"),
    KERNEL_PAGE_TABLES_SATP_OFFSET = const KERNEL_PAGE_TABLES_SATP_OFFSET,
);
global_asm!(
    include_str!("trap.S"),
    TRAP_FRAME_OFFSET = const TRAP_FRAME_OFFSET,
    KERNEL_PAGE_TABLES_SATP_OFFSET = const KERNEL_PAGE_TABLES_SATP_OFFSET,
);
global_asm!(include_str!("powersave.S"));
global_asm!(include_str!("panic.S"));
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
