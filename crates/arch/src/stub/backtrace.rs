/// Stub CalleeSavedRegs for non-RISC-V targets (unit tests).
#[derive(Debug, Clone, Default)]
pub struct CalleeSavedRegs {
    regs: [usize; 32],
}

impl core::fmt::Display for CalleeSavedRegs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "CalleeSavedRegs(stub)")
    }
}

impl core::ops::Index<usize> for CalleeSavedRegs {
    type Output = usize;
    fn index(&self, index: usize) -> &Self::Output {
        &self.regs[index]
    }
}

impl core::ops::IndexMut<usize> for CalleeSavedRegs {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.regs[index]
    }
}

impl CalleeSavedRegs {
    pub fn ra(&self) -> usize {
        self.regs[1]
    }

    pub fn set_ra(&mut self, value: usize) {
        self.regs[1] = value;
    }

    pub fn sp(&self) -> usize {
        self.regs[2]
    }

    pub fn set_sp(&mut self, value: usize) {
        self.regs[2] = value;
    }

    pub fn with_context<F: FnMut(&mut CalleeSavedRegs)>(_f: F) {
        panic!("with_context is not available on this target");
    }
}
