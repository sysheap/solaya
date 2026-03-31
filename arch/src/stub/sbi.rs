pub mod sbi_call {
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
        pub fn assert_success(&self) {
            assert!(
                self.error == SbiError::SBI_SUCCESS,
                "SBI call failed: {self:?}"
            );
        }

        #[allow(dead_code)]
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
}

pub mod extensions {
    pub mod base_extension {
        pub struct SbiSpecVersion {
            pub minor: u32,
            pub major: u32,
        }

        #[allow(dead_code)]
        pub fn sbi_get_spec_version() -> SbiSpecVersion {
            SbiSpecVersion { minor: 0, major: 0 }
        }
    }

    pub mod hart_state_extension {
        use crate::sbi::sbi_call::SbiRet;

        #[allow(dead_code)]
        pub fn get_number_of_harts() -> usize {
            1
        }

        #[allow(dead_code)]
        pub fn is_hart_stopped(_hart_id: usize) -> bool {
            true
        }

        #[allow(dead_code)]
        pub fn start_hart(_hart_id: usize, _start_addr: usize, _opaque: usize) -> SbiRet {
            SbiRet::default()
        }
    }

    pub mod ipi_extension {
        use crate::sbi::sbi_call::SbiRet;

        #[allow(dead_code)]
        pub fn sbi_send_ipi(_hart_mask: u64, _hart_mask_base: i64) -> SbiRet {
            SbiRet::default()
        }
    }

    pub mod timer_extension {
        use crate::sbi::sbi_call::SbiRet;

        #[allow(dead_code)]
        pub fn sbi_set_timer(_stime_value: u64) -> SbiRet {
            SbiRet::default()
        }
    }
}
