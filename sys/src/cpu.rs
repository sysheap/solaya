use core::ptr::addr_of;

pub use arch::CpuId;

use crate::klibc::runtime_initialized::RuntimeInitializedData;

pub static STARTING_CPU_ID: RuntimeInitializedData<CpuId> = RuntimeInitializedData::new();

/// Reads the current CPU ID.
/// Before the per-CPU struct is set up, returns STARTING_CPU_ID.
/// After setup, reads the cpu_id field from the per-CPU struct pointed to by sscratch.
///
/// This works because the kernel's Cpu struct starts with the same layout as
/// what sys expects: the cpu_id field is at a known offset from the sscratch pointer.
/// The kernel must ensure cpu_id is at `CPU_ID_OFFSET` bytes from the struct base.
#[cfg(all(target_arch = "riscv64", not(miri)))]
pub fn cpu_id() -> CpuId {
    let ptr = arch::cpu::read_sscratch() as *const u8;
    if ptr.is_null() {
        return *STARTING_CPU_ID;
    }
    // SAFETY: The per-CPU struct is statically allocated via Box::leak.
    // CPU_ID_OFFSET gives the correct byte offset of the cpu_id field.
    unsafe { *addr_of!((*(ptr as *const CpuIdLayout)).cpu_id) }
}

#[cfg(any(not(target_arch = "riscv64"), miri))]
pub fn cpu_id() -> CpuId {
    CpuId::from_hart_id(0)
}

/// Disable interrupts and halt forever. Used for shutdown paths.
pub fn disable_interrupts_and_halt() -> ! {
    // SAFETY: We are shutting down — disabling interrupts prevents further preemption.
    unsafe {
        arch::cpu::disable_global_interrupts();
    }
    loop {
        arch::cpu::wait_for_interrupt();
    }
}

/// Layout prefix of the per-CPU struct. The kernel's Cpu struct must have
/// these fields in exactly this order as its first fields (via #[repr(C)]).
#[repr(C)]
struct CpuIdLayout {
    _kernel_page_tables_satp_value: usize,
    _trap_frame: common::syscalls::trap_frame::TrapFrame,
    cpu_id: CpuId,
}
