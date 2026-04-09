use crate::sbi::{self, sbi_call::SbiRet};

const EID: u64 = 0x53525354;
const FID_SYSTEM_RESET: u64 = 0x0;

pub fn sbi_system_reset(reset_type: u32, reset_reason: u32) -> SbiRet {
    sbi::sbi_call(
        EID,
        FID_SYSTEM_RESET,
        reset_type as u64,
        reset_reason as u64,
        0,
    )
}
