use crate::sbi::{self, sbi_call::SbiRet};

const EID: u64 = 0x48534D;
const FID_HART_START: u64 = 0x0;
const FID_GET_STATUS: u64 = 0x2;

const HART_STATUS_STOPPED: usize = 1;

pub fn get_number_of_harts() -> usize {
    let mut harts = 0;

    loop {
        if sbi::sbi_call(EID, FID_GET_STATUS, harts as u64, 0, 0).is_error() {
            break;
        }
        harts += 1;
    }

    harts
}

pub fn is_hart_stopped(hart_id: usize) -> bool {
    let ret = sbi::sbi_call(EID, FID_GET_STATUS, hart_id as u64, 0, 0);
    !ret.is_error() && ret.value == HART_STATUS_STOPPED as i64
}

pub fn start_hart(hart_id: usize, start_addr: usize, opaque: usize) -> SbiRet {
    sbi::sbi_call(
        EID,
        FID_HART_START,
        hart_id as u64,
        start_addr as u64,
        opaque as u64,
    )
}
