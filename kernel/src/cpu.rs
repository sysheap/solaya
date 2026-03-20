#![allow(unsafe_code)]
use alloc::{boxed::Box, sync::Arc};
use common::syscalls::trap_frame::TrapFrame;
use core::{mem::offset_of, ptr::addr_of};

pub use arch::CpuId;

use crate::{
    klibc::{Spinlock, SpinlockGuard, runtime_initialized::RuntimeInitializedData, sizes::KiB},
    memory::page_tables::RootPageTableHolder,
    processes::{
        process::Process,
        scheduler::CpuScheduler,
        thread::{ThreadRef, ThreadWeakRef},
    },
};
use arch::sbi::extensions::ipi_extension::sbi_send_ipi;

pub(crate) const KERNEL_STACK_SIZE: usize = KiB(512);

pub static STARTING_CPU_ID: RuntimeInitializedData<CpuId> = RuntimeInitializedData::new();

pub const TRAP_FRAME_OFFSET: usize = offset_of!(Cpu, trap_frame);

pub const KERNEL_PAGE_TABLES_SATP_OFFSET: usize = offset_of!(Cpu, kernel_page_tables_satp_value);

pub struct Cpu {
    kernel_page_tables_satp_value: usize,
    trap_frame: TrapFrame,
    scheduler: Spinlock<CpuScheduler>,
    cpu_id: CpuId,
    kernel_page_tables: RootPageTableHolder,
    number_cpus: usize,
}

impl Cpu {
    pub fn ipi_to_all_but_me(&self) {
        assert!(
            self.number_cpus <= 64,
            "If we have more cpu's we need to use hart_mask_base, that is not implemented yet."
        );
        let mut mask = 0;
        for id in (0..self.number_cpus).filter(|i| *i != self.cpu_id.as_usize()) {
            mask |= 1 << id;
        }
        sbi_send_ipi(mask, 0).assert_success();
    }

    pub fn init(cpu_id: CpuId, number_cpus: usize) -> *const Cpu {
        assert!(
            cpu_id.as_usize() < number_cpus,
            "cpu_id {cpu_id} must be less than number_cpus {number_cpus}"
        );
        let kernel_stack = Box::leak(vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice()).as_mut_ptr();
        let mut page_tables =
            RootPageTableHolder::new_with_kernel_mapping(&crate::memory::kernel_device_mappings());

        let stack_start_virtual = (0usize).wrapping_sub(KERNEL_STACK_SIZE);

        page_tables.map(
            crate::memory::VirtAddr::new(stack_start_virtual),
            crate::memory::PhysAddr::new(kernel_stack as usize),
            KERNEL_STACK_SIZE,
            crate::memory::page_tables::XWRMode::ReadWrite,
            false,
            format!("KERNEL_STACK CPU {cpu_id}"),
        );

        let satp_value = page_tables.get_satp_value_from_page_tables();

        let cpu = Box::new(Self {
            kernel_page_tables_satp_value: satp_value,
            trap_frame: TrapFrame::zero(),
            scheduler: Spinlock::new(CpuScheduler::new()),
            cpu_id,
            number_cpus,
            kernel_page_tables: page_tables,
        });

        Box::leak(cpu) as *const Cpu
    }

    fn cpu_ptr() -> *mut Cpu {
        let ptr = arch::cpu::read_sscratch() as *mut Self;
        assert!(!ptr.is_null() && ptr.is_aligned());
        ptr
    }

    pub fn current() -> &'static Cpu {
        // SAFETY: The pointer points to a static and is therefore always valid.
        unsafe { &*Self::cpu_ptr() }
    }

    pub fn read_trap_frame() -> TrapFrame {
        let cpu_ptr = Self::cpu_ptr();
        // SAFETY: Cpu is statically allocated and offset
        // is calculated by the actual field offset.
        unsafe {
            let trap_frame_ptr = cpu_ptr.byte_add(TRAP_FRAME_OFFSET).cast::<TrapFrame>();
            trap_frame_ptr.read_volatile()
        }
    }

    pub fn write_trap_frame(trap_frame: TrapFrame) {
        let cpu_ptr = Self::cpu_ptr();
        // SAFETY: Cpu is statically allocated and offset
        // is calculated by the actual field offset.
        unsafe {
            let trap_frame_ptr = cpu_ptr.byte_add(TRAP_FRAME_OFFSET).cast::<TrapFrame>();
            trap_frame_ptr.write_volatile(trap_frame);
        }
    }

    pub fn with_scheduler<R>(f: impl FnOnce(SpinlockGuard<'_, CpuScheduler>) -> R) -> R {
        let cpu = Self::current();
        let scheduler = cpu.scheduler().lock();
        f(scheduler)
    }

    pub fn current_thread() -> ThreadRef {
        Self::with_scheduler(|s| s.get_current_thread().clone())
    }

    pub fn current_thread_weak() -> ThreadWeakRef {
        Self::with_scheduler(|s| Arc::downgrade(s.get_current_thread()))
    }

    pub fn with_current_process<R>(f: impl FnOnce(SpinlockGuard<'_, Process>) -> R) -> R {
        Self::with_scheduler(|s| f(s.get_current_process().lock()))
    }

    pub fn maybe_kernel_page_tables() -> Option<&'static RootPageTableHolder> {
        let ptr = arch::cpu::read_sscratch() as *mut Self;
        if ptr.is_null() || !ptr.is_aligned() {
            return None;
        }
        // SAFETY: We validate above that the pointer is non-null and aligned.
        // The Cpu struct is statically allocated via Box::leak and never freed.
        unsafe { Some(&(*ptr).kernel_page_tables) }
    }

    pub fn cpu_id() -> CpuId {
        let ptr = arch::cpu::read_sscratch() as *mut Self;
        if ptr.is_null() {
            return *STARTING_CPU_ID;
        }
        // SAFETY: The Cpu struct is statically allocated via Box::leak; addr_of!
        // reads the field without creating an intermediate reference.
        unsafe { *addr_of!((*ptr).cpu_id) }
    }

    pub fn activate_kernel_page_table(&self) {
        self.kernel_page_tables.activate_page_table();
    }

    pub fn scheduler(&self) -> &Spinlock<CpuScheduler> {
        &self.scheduler
    }
}

impl Drop for Cpu {
    fn drop(&mut self) {
        panic!("Cpu struct is never allowed to be dropped!");
    }
}
