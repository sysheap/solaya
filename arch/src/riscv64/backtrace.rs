/// Callee-saved RISC-V registers captured for stack unwinding.
#[derive(Debug, Clone, Default)]
pub struct CalleeSavedRegs {
    x1: usize,
    x2: usize,
    x8: usize,
    x9: usize,
    x18: usize,
    x19: usize,
    x20: usize,
    x21: usize,
    x22: usize,
    x23: usize,
    x24: usize,
    x25: usize,
    x26: usize,
    x27: usize,
}

const _: [(); 0x70] = [(); core::mem::size_of::<CalleeSavedRegs>()];

impl core::fmt::Display for CalleeSavedRegs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        macro_rules! print_reg {
            ($reg:ident) => {
                writeln!(f, "{}: {:#x}", stringify!($reg), self.$reg)?
            };
        }
        print_reg!(x1);
        print_reg!(x2);
        print_reg!(x8);
        print_reg!(x9);
        print_reg!(x18);
        print_reg!(x19);
        print_reg!(x20);
        print_reg!(x21);
        print_reg!(x22);
        print_reg!(x23);
        print_reg!(x24);
        print_reg!(x25);
        print_reg!(x26);
        print_reg!(x27);
        Ok(())
    }
}

impl core::ops::Index<usize> for CalleeSavedRegs {
    type Output = usize;
    fn index(&self, index: usize) -> &Self::Output {
        match index {
            1 => &self.x1,
            2 => &self.x2,
            8 => &self.x8,
            9 => &self.x9,
            18 => &self.x18,
            19 => &self.x19,
            20 => &self.x20,
            21 => &self.x21,
            22 => &self.x22,
            23 => &self.x23,
            24 => &self.x24,
            25 => &self.x25,
            26 => &self.x26,
            27 => &self.x27,
            _ => panic!("Invalid register index"),
        }
    }
}

impl core::ops::IndexMut<usize> for CalleeSavedRegs {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            1 => &mut self.x1,
            2 => &mut self.x2,
            8 => &mut self.x8,
            9 => &mut self.x9,
            18 => &mut self.x18,
            19 => &mut self.x19,
            20 => &mut self.x20,
            21 => &mut self.x21,
            22 => &mut self.x22,
            23 => &mut self.x23,
            24 => &mut self.x24,
            25 => &mut self.x25,
            26 => &mut self.x26,
            27 => &mut self.x27,
            _ => panic!("Invalid register index"),
        }
    }
}

impl CalleeSavedRegs {
    pub fn ra(&self) -> usize {
        self.x1
    }

    pub fn set_ra(&mut self, value: usize) {
        self.x1 = value;
    }

    pub fn sp(&self) -> usize {
        self.x2
    }

    pub fn set_sp(&mut self, value: usize) {
        self.x2 = value;
    }

    pub fn with_context<F: FnMut(&mut CalleeSavedRegs)>(f: F) {
        #[repr(C)]
        struct ClosureWrapper<F: FnMut(&mut CalleeSavedRegs)>(F);

        let mut closure = ClosureWrapper(f);

        dispatch(
            &mut CalleeSavedRegs::default(),
            &mut closure,
            closure_to_fn_pointer,
        );

        extern "C" fn closure_to_fn_pointer<F: FnMut(&mut CalleeSavedRegs)>(
            regs: &mut CalleeSavedRegs,
            f_data: &mut ClosureWrapper<F>,
        ) {
            (f_data.0)(regs);
        }

        // SAFETY: Naked function that captures callee-saved registers (s0-s11,
        // ra, sp) into the CalleeSavedRegs struct, then calls the closure.
        #[unsafe(naked)]
        extern "C-unwind" fn dispatch<F: FnMut(&mut CalleeSavedRegs)>(
            regs: &mut CalleeSavedRegs,
            f_data: &mut ClosureWrapper<F>,
            f: extern "C" fn(&mut CalleeSavedRegs, &mut ClosureWrapper<F>),
        ) {
            core::arch::naked_asm!(
                "
                     sd x1, 0x00(a0)
                     sd x2, 0x08(a0)
                     sd x8, 0x10(a0)
                     sd x9, 0x18(a0)
                     sd x18, 0x20(a0)
                     sd x19, 0x28(a0)
                     sd x20, 0x30(a0)
                     sd x21, 0x38(a0)
                     sd x22, 0x40(a0)
                     sd x23, 0x48(a0)
                     sd x24, 0x50(a0)
                     sd x25, 0x58(a0)
                     sd x26, 0x60(a0)
                     sd x27, 0x68(a0)
                     addi sp, sp, -0x08
                     sd ra, 0x00(sp)
                     jalr a2
                     ld ra, 0x00(sp)
                     addi sp, sp, 0x08
                     ret
                    "
            )
        }
    }
}
