use crate::syscalls::trap_frame::TrapFrame;
use core::mem::offset_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuId(usize);

impl CpuId {
    pub fn from_hart_id(hart_id: usize) -> Self {
        Self(hart_id)
    }

    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl core::fmt::Display for CpuId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[repr(C)]
pub struct CpuBase {
    pub kernel_page_tables_satp_value: usize,
    pub trap_frame: TrapFrame,
    pub cpu_id: CpuId,
}

pub const TRAP_FRAME_OFFSET: usize = offset_of!(CpuBase, trap_frame);
pub const KERNEL_PAGE_TABLES_SATP_OFFSET: usize =
    offset_of!(CpuBase, kernel_page_tables_satp_value);
pub const CPU_ID_OFFSET: usize = offset_of!(CpuBase, cpu_id);
