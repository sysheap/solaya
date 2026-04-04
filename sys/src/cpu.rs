pub use common::cpu::{
    CPU_ID_OFFSET, CpuBase, CpuId, KERNEL_PAGE_TABLES_SATP_OFFSET, TRAP_FRAME_OFFSET,
};

use crate::klibc::runtime_initialized::RuntimeInitializedData;

pub static STARTING_CPU_ID: RuntimeInitializedData<CpuId> = RuntimeInitializedData::new();

/// Reads the current CPU ID.
/// Before the per-CPU struct is set up, returns STARTING_CPU_ID.
/// After setup, reads the cpu_id field from the per-CPU struct pointed to by sscratch.
#[cfg(all(target_arch = "riscv64", not(miri)))]
pub fn cpu_id() -> CpuId {
    let ptr = arch::cpu::read_sscratch() as *const u8;
    if ptr.is_null() {
        return *STARTING_CPU_ID;
    }
    // SAFETY: The per-CPU struct is statically allocated via Box::leak.
    // CPU_ID_OFFSET is a compile-time constant from offset_of!(CpuBase, cpu_id).
    unsafe { *ptr.add(CPU_ID_OFFSET).cast::<CpuId>() }
}

#[cfg(any(not(target_arch = "riscv64"), miri))]
pub fn cpu_id() -> CpuId {
    CpuId::from_hart_id(0)
}

/// Return a reference to the per-CPU struct pointed to by sscratch.
/// Panics if sscratch is null or unaligned.
pub fn per_cpu_ref<T>() -> &'static T {
    let ptr = arch::cpu::read_sscratch() as *const T;
    assert!(!ptr.is_null() && ptr.is_aligned());
    // SAFETY: The per-CPU struct is statically allocated via Box::leak and
    // never freed. Non-null and aligned checked above.
    unsafe { &*ptr }
}

/// Return a reference to the per-CPU struct, or None if sscratch is null/unaligned.
pub fn try_per_cpu_ref<T>() -> Option<&'static T> {
    let ptr = arch::cpu::read_sscratch() as *const T;
    if ptr.is_null() || !ptr.is_aligned() {
        return None;
    }
    // SAFETY: Non-null and aligned checked above. Per-CPU struct is statically
    // allocated via Box::leak and never freed.
    Some(unsafe { &*ptr })
}

/// Read a field from the per-CPU struct using a volatile read.
/// `offset` is the byte offset of the field from the struct start.
pub fn per_cpu_volatile_read<T>(offset: usize) -> T {
    let ptr = arch::cpu::read_sscratch() as *const u8;
    assert!(!ptr.is_null());
    // SAFETY: Per-CPU struct is statically allocated. The caller provides a
    // valid offset (computed via offset_of!).
    unsafe {
        let field_ptr = ptr.add(offset).cast::<T>();
        field_ptr.read_volatile()
    }
}

/// Write a field in the per-CPU struct using a volatile write.
/// `offset` is the byte offset of the field from the struct start.
pub fn per_cpu_volatile_write<T>(offset: usize, value: T) {
    let ptr = arch::cpu::read_sscratch() as *mut u8;
    assert!(!ptr.is_null());
    // SAFETY: Per-CPU struct is statically allocated. The caller provides a
    // valid offset (computed via offset_of!).
    unsafe {
        let field_ptr = ptr.add(offset).cast::<T>();
        field_ptr.write_volatile(value);
    }
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
