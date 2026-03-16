use core::arch::asm;

const SIE_STIE: usize = 5;
const SSTATUS_SPP: usize = 8;
const SIP_SSIP: usize = 1;

macro_rules! read_csr {
    ($name:ident) => {
        #[allow(dead_code)]
        pub fn ${concat(read_, $name)}() -> usize {
            let value: usize;
            // SAFETY: Reading a CSR has no memory side-effects; the value is
            // returned in a general-purpose register.
            unsafe {
                asm!(concat!("csrr {}, ", stringify!($name)), out(reg) value);
            }
            value
        }
    };
}

macro_rules! write_csr {
    ($name:ident) => {
        #[allow(dead_code)]
        pub fn ${concat(write_, $name)}(value: usize) {
            // SAFETY: Writing a CSR is a privileged operation with no memory
            // aliasing concerns. Callers are responsible for semantic correctness.
            unsafe {
                asm!(concat!("csrw ", stringify!($name), ", {}"), in(reg) value);
            }
        }

        #[allow(dead_code)]
        pub fn ${concat(csrs_, $name)}(mask: usize) {
            // SAFETY: csrs (set bits) is a privileged CSR operation with no
            // memory aliasing concerns.
            unsafe {
                asm!(concat!("csrs ", stringify!($name), ", {}"), in(reg) mask);
            }
        }

        #[allow(dead_code)]
        pub fn ${concat(csrc_, $name)}(mask: usize) {
            // SAFETY: csrc (clear bits) is a privileged CSR operation with no
            // memory aliasing concerns.
            unsafe {
                asm!(concat!("csrc ", stringify!($name), ", {}"), in(reg) mask);
            }
        }
    };
}

read_csr!(satp);
read_csr!(stval);
read_csr!(sepc);
read_csr!(scause);
read_csr!(sscratch);
read_csr!(sie);
read_csr!(sstatus);

write_csr!(satp);
write_csr!(sepc);
write_csr!(sscratch);
write_csr!(sstatus);
write_csr!(sie);
write_csr!(sip);

/// # Safety
/// Caller must ensure `satp_val` points to a valid page table.
pub unsafe fn write_satp_and_fence(satp_val: usize) {
    write_satp(satp_val);
    // SAFETY: sfence.vma flushes the TLB; required after changing satp.
    unsafe {
        asm!("sfence.vma");
    }
}

pub fn memory_fence() {
    // SAFETY: `fence` is a memory ordering instruction with no operands.
    unsafe {
        asm!("fence");
    }
}

/// # Safety
/// Must only be called during panic or shutdown paths where no further
/// interrupt handling is expected.
pub unsafe fn disable_global_interrupts() {
    csrc_sstatus(0b10);
    write_sie(0);
}

pub fn wait_for_interrupt() {
    // SAFETY: `wfi` halts the hart until an interrupt arrives; it has
    // no memory side-effects.
    unsafe {
        asm!("wfi");
    }
}

#[allow(dead_code)]
pub fn is_timer_enabled() -> bool {
    let sie = read_sie();
    (sie & (1 << SIE_STIE)) > 0
}

pub fn enable_timer_interrupt() {
    csrs_sie(1 << SIE_STIE);
}

pub fn clear_supervisor_software_interrupt() {
    csrc_sip(1 << SIP_SSIP);
}

#[allow(dead_code)]
pub fn is_in_kernel_mode() -> bool {
    let sstatus = read_sstatus();
    (sstatus & (1 << SSTATUS_SPP)) > 0
}

pub fn set_ret_to_kernel_mode(kernel_mode: bool) {
    if kernel_mode {
        csrs_sstatus(1 << SSTATUS_SPP);
    } else {
        csrc_sstatus(1 << SSTATUS_SPP);
    }
}

pub fn trigger_supervisor_software_interrupt() {
    csrs_sip(1 << SIP_SSIP);
}

pub struct InterruptGuard {
    was_enabled: bool,
}

impl InterruptGuard {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let sstatus = read_sstatus();
        let was_enabled = (sstatus & 0b10) != 0;
        if was_enabled {
            csrc_sstatus(0b10);
        }
        Self { was_enabled }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.was_enabled {
            csrs_sstatus(0b10);
        }
    }
}
