use common::cpu::{KERNEL_PAGE_TABLES_SATP_OFFSET, TRAP_FRAME_OFFSET};
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
