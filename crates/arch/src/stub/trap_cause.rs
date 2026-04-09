pub mod interrupt {
    pub const SUPERVISOR_SOFTWARE_INTERRUPT: usize = 1;
    #[allow(dead_code)]
    pub const MACHINE_SOFTWARE_INTERRUPT: usize = 3;
    pub const SUPERVISOR_TIMER_INTERRUPT: usize = 5;
    #[allow(dead_code)]
    pub const MACHINE_TIMER_INTERRUPT: usize = 7;
    pub const SUPERVISOR_EXTERNAL_INTERRUPT: usize = 9;
    #[allow(dead_code)]
    pub const MACHINE_EXTERNAL_INTERRUPT: usize = 11;
}

pub mod exception {
    #[allow(dead_code)]
    pub const INSTRUCTION_ADDRESS_MISALIGNED: usize = 0;
    #[allow(dead_code)]
    pub const INSTRUCTION_ACCESS_FAULT: usize = 1;
    #[allow(dead_code)]
    pub const ILLEGAL_INSTRUCTION: usize = 2;
    #[allow(dead_code)]
    pub const BREAKPOINT: usize = 3;
    #[allow(dead_code)]
    pub const LOAD_ADDRESS_MISALIGNED: usize = 4;
    #[allow(dead_code)]
    pub const LOAD_ACCESS_FAULT: usize = 5;
    #[allow(dead_code)]
    pub const STORE_AMO_ADDRESS_MISALIGNED: usize = 6;
    #[allow(dead_code)]
    pub const STORE_AMO_ACCESS_FAULT: usize = 7;
    pub const ENVIRONMENT_CALL_FROM_U_MODE: usize = 8;
    #[allow(dead_code)]
    pub const ENVIRONMENT_CALL_FROM_S_MODE: usize = 9;
    #[allow(dead_code)]
    pub const ENVIRONMENT_CALL_FROM_M_MODE: usize = 11;
    pub const INSTRUCTION_PAGE_FAULT: usize = 12;
    pub const LOAD_PAGE_FAULT: usize = 13;
    pub const STORE_AMO_PAGE_FAULT: usize = 15;
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct InterruptCause(usize);

impl InterruptCause {
    #[allow(dead_code)]
    pub fn from_scause() -> Self {
        Self(super::cpu::read_scause())
    }

    pub fn is_interrupt(&self) -> bool {
        self.0 >> 63 == 1
    }

    pub fn get_exception_code(&self) -> usize {
        self.0 << 1 >> 1
    }

    #[allow(dead_code)]
    pub fn get_reason(&self) -> &'static str {
        use exception::*;
        use interrupt::*;

        let is_asynchronous = self.is_interrupt();

        if is_asynchronous {
            match self.get_exception_code() {
                SUPERVISOR_SOFTWARE_INTERRUPT => "Supervisor software interrupt",
                MACHINE_SOFTWARE_INTERRUPT => "Machine software interrupt",
                SUPERVISOR_TIMER_INTERRUPT => "Supervisor timer interrupt",
                MACHINE_TIMER_INTERRUPT => "Machine timer interrupt",
                SUPERVISOR_EXTERNAL_INTERRUPT => "Supervisor external interrupt",
                MACHINE_EXTERNAL_INTERRUPT => "Machine external interrupt",
                _ => "Reserved or designated for platform use",
            }
        } else {
            match self.get_exception_code() {
                INSTRUCTION_ADDRESS_MISALIGNED => "Instruction address misaligned",
                INSTRUCTION_ACCESS_FAULT => "Instruction access fault",
                ILLEGAL_INSTRUCTION => "Illegal instruction",
                BREAKPOINT => "Breakpoint",
                LOAD_ADDRESS_MISALIGNED => "Load address misaligned",
                LOAD_ACCESS_FAULT => "Load access fault",
                STORE_AMO_ADDRESS_MISALIGNED => "Store/AMO address misaligned",
                STORE_AMO_ACCESS_FAULT => "Store/AMO access fault",
                ENVIRONMENT_CALL_FROM_U_MODE => "Environment call from U-mode",
                ENVIRONMENT_CALL_FROM_S_MODE => "Environment call from S-mode",
                ENVIRONMENT_CALL_FROM_M_MODE => "Environment call from M-Mode",
                INSTRUCTION_PAGE_FAULT => "Instruction page fault",
                LOAD_PAGE_FAULT => "Load page fault",
                STORE_AMO_PAGE_FAULT => "Store/AMO page fault",
                _ => "Reserved or designated for platform use",
            }
        }
    }
}
