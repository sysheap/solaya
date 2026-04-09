#[cfg(all(not(miri), target_arch = "riscv64"))]
core::arch::global_asm!(
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
