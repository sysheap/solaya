use crate::sbi::{self, sbi_call::SbiRet};

const EID: u64 = 0x735049;
const FID_SEND_IPI: u64 = 0x0;

#[allow(clippy::cast_sign_loss)]
pub fn sbi_send_ipi(hart_mask: u64, hart_mask_base: i64) -> SbiRet {
    sbi::sbi_call(EID, FID_SEND_IPI, hart_mask, hart_mask_base as u64, 0)
}
