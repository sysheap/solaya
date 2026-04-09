use crate::sbi;

const EID: u64 = 0x10;

pub struct SbiSpecVersion {
    pub minor: u32,
    pub major: u32,
}

pub fn sbi_get_spec_version() -> SbiSpecVersion {
    let result = sbi::sbi_call(EID, 0x0, 0, 0, 0);
    SbiSpecVersion {
        minor: u32::try_from(result.value & 0xFF_FFFF).expect("SBI minor version fits in u32"),
        major: u32::try_from(result.value >> 24).expect("SBI major version fits in u32"),
    }
}
