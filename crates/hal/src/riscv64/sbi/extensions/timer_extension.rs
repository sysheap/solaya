use crate::sbi::{self, sbi_call::SbiRet};

const EID: u64 = 0x54494D45;
const FID_SET_TIMER: u64 = 0x0;

pub fn sbi_set_timer(stime_value: u64) -> SbiRet {
    sbi::sbi_call(EID, FID_SET_TIMER, stime_value, 0, 0)
}
