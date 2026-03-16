use core::arch::asm;

#[repr(i64)]
#[derive(Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub enum SbiError {
    SBI_SUCCESS = 0,
    SBI_ERR_FAILED = -1,
    SBI_ERR_NOT_SUPPORTED = -2,
    SBI_ERR_INVALID_PARAM = -3,
    SBI_ERR_DENIED = -4,
    SBI_ERR_INVALID_ADDRESS = -5,
    SBI_ERR_ALREADY_AVAILABLE = -6,
    SBI_ERR_ALREADY_STARTED = -7,
    SBI_ERR_ALREADY_STOPPED = -8,
    SBI_ERR_NO_SHMEM = -9,
}

#[must_use]
#[derive(Debug)]
pub struct SbiRet {
    pub error: SbiError,
    pub value: i64,
}

impl SbiRet {
    fn new(error: i64, value: i64) -> Self {
        let error = match error {
            0 => SbiError::SBI_SUCCESS,
            -1 => SbiError::SBI_ERR_FAILED,
            -2 => SbiError::SBI_ERR_NOT_SUPPORTED,
            -3 => SbiError::SBI_ERR_INVALID_PARAM,
            -4 => SbiError::SBI_ERR_DENIED,
            -5 => SbiError::SBI_ERR_INVALID_ADDRESS,
            -6 => SbiError::SBI_ERR_ALREADY_AVAILABLE,
            -7 => SbiError::SBI_ERR_ALREADY_STARTED,
            -8 => SbiError::SBI_ERR_ALREADY_STOPPED,
            -9 => SbiError::SBI_ERR_NO_SHMEM,
            _ => panic!("Unknown SBI error code: {error}"),
        };
        Self { error, value }
    }

    pub fn assert_success(&self) {
        assert!(
            self.error == SbiError::SBI_SUCCESS,
            "SBI call failed: {self:?}"
        );
    }

    pub fn is_error(&self) -> bool {
        self.error != SbiError::SBI_SUCCESS
    }
}

impl Default for SbiRet {
    fn default() -> Self {
        Self {
            error: SbiError::SBI_SUCCESS,
            value: Default::default(),
        }
    }
}

pub fn sbi_call(eid: u64, fid: u64, arg0: u64, arg1: u64, arg2: u64) -> SbiRet {
    let mut error: i64;
    let mut value: i64;

    // SAFETY: ecall traps into M-mode firmware (SBI). The register convention
    // is defined by the SBI spec: a7=EID, a6=FID, a0-a2=args, a0/a1=return.
    unsafe {
        asm!(
            "ecall",
            in("a7") eid,
            in("a6") fid,
            in("a0") arg0,
            in("a1") arg1,
            in("a2") arg2,
            lateout("a0") error,
            lateout("a1") value,
        );
    }
    SbiRet::new(error, value)
}
