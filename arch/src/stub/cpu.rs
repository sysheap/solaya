macro_rules! read_csr_stub {
    ($name:ident) => {
        #[allow(dead_code)]
        pub fn ${concat(read_, $name)}() -> usize {
            0
        }
    };
}

macro_rules! write_csr_stub {
    ($name:ident) => {
        #[allow(dead_code)]
        pub fn ${concat(write_, $name)}(_value: usize) {}

        #[allow(dead_code)]
        pub fn ${concat(csrs_, $name)}(_mask: usize) {}

        #[allow(dead_code)]
        pub fn ${concat(csrc_, $name)}(_mask: usize) {}
    };
}

read_csr_stub!(satp);
read_csr_stub!(stval);
read_csr_stub!(sepc);
read_csr_stub!(scause);
read_csr_stub!(sscratch);
read_csr_stub!(sie);
read_csr_stub!(sstatus);
read_csr_stub!(stvec);

write_csr_stub!(satp);
write_csr_stub!(sepc);
write_csr_stub!(sscratch);
write_csr_stub!(sstatus);
write_csr_stub!(sie);
write_csr_stub!(sip);

#[allow(dead_code)]
pub unsafe fn write_satp_and_fence(_satp_val: usize) {}

#[allow(dead_code)]
pub fn memory_fence() {}
pub fn io_fence() {}

#[allow(dead_code)]
pub unsafe fn disable_global_interrupts() {}

#[allow(dead_code)]
pub fn wait_for_interrupt() {}

#[allow(dead_code)]
pub fn is_timer_enabled() -> bool {
    false
}

#[allow(dead_code)]
pub fn enable_timer_interrupt() {}

#[allow(dead_code)]
pub fn clear_supervisor_software_interrupt() {}

#[allow(dead_code)]
pub fn is_in_kernel_mode() -> bool {
    false
}

#[allow(dead_code)]
pub fn set_ret_to_kernel_mode(_kernel_mode: bool) {}

#[allow(dead_code)]
pub fn trigger_supervisor_software_interrupt() {}

#[allow(dead_code)]
pub extern "C" fn wfi_loop() -> ! {
    panic!("wfi_loop is not available on this target");
}

#[allow(dead_code)]
pub fn asm_panic_rust() {
    panic!("Panic from asm code");
}

pub struct InterruptGuard;

impl InterruptGuard {
    pub fn new() -> Self {
        Self
    }
}
