use core::arch::asm;

// SAFETY: Called from assembly panic handler; must use C name to be reachable.
#[unsafe(no_mangle)]
pub fn asm_panic_rust() {
    let ra: usize;
    // SAFETY: Reads the return address register to report the faulting location.
    unsafe {
        asm!("mv {}, ra", out(reg) ra);
    }
    panic!("Panic from asm code (ra={ra:#x})");
}

// SAFETY: Called from scheduler as the idle loop; must use C name. The naked
// attribute is required because the function never returns and needs no prologue.
#[unsafe(no_mangle)]
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
