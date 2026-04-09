use crate::{debug, klibc::array_vec::ArrayVec};

use super::eh_frame_parser::{Instruction, ParsedFDE};

pub struct Unwinder<'a> {
    fde: &'a ParsedFDE<'a>,
    rows: ArrayVec<Row, 128>, // 128 should be enough I guess - otherwise we can increase it
    row_stack: ArrayVec<Row, 16>,
}

impl<'a> Unwinder<'a> {
    pub fn new(fde: &'a ParsedFDE<'a>) -> Self {
        let cde_instructions = &fde.cie.initial_instructions;
        let fde_instructions = &fde.instructions;
        let mut self_ = Self {
            fde,
            rows: ArrayVec::default(),
            row_stack: ArrayVec::default(),
        };

        self_
            .rows
            .push(Row::new(fde.pc_begin))
            .expect("No space left in the ArrayVec");

        debug!("Evaluate cde instructions");
        self_.evaluate_instructions(cde_instructions);
        debug!("Evaluate fde instructions");
        self_.evaluate_instructions(fde_instructions);

        for row in &self_.rows {
            debug!("{row}");
        }

        let mut last_row = self_.last_row();
        last_row.end_address = fde.pc_begin + fde.address_range as usize;
        self_.update_last_row(&last_row);

        debug!("{} rows", self_.rows.len());

        self_
    }

    pub fn find_row_for_address(&self, address: usize) -> &Row {
        self.rows
            .iter()
            .find(|row| address >= row.start_address && address <= row.end_address)
            .expect("There must be an unwind rule.")
    }

    #[cfg(test)]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    fn last_row(&self) -> Row {
        self.rows
            .last()
            .cloned()
            .expect("There must be always at least one row.")
    }

    fn update_last_row(&mut self, row: &Row) {
        *self
            .rows
            .last_mut()
            .expect("There must be always at least one row.") = row.clone();
    }

    fn evaluate_instructions(&mut self, instructions: &[Instruction]) {
        let mut current_address = self.fde.pc_begin;
        debug!("current address: {current_address}");
        let mut current_row = self.last_row();
        for instruction in instructions {
            match instruction {
                Instruction::AdvanceLoc { delta } => {
                    debug!("AdvanceLoc(delta={})", *delta);
                    current_address += *delta as usize;
                    current_row.end_address = current_address;
                    self.update_last_row(&current_row);
                    current_row.start_address = current_address;
                    current_row.end_address = 0;
                    self.rows
                        .push(current_row.clone())
                        .expect("Not enough space in the array vec.");
                    debug!("pushing new row with address {current_address}");
                }
                Instruction::Offset { register, offset } => {
                    debug!("Offset(register={}, offset={})", *register, *offset);
                    let real_offset = i64::try_from(*offset)
                        .expect("DWARF offset fits in i64")
                        .wrapping_mul(self.fde.cie.data_alignment_factor);
                    current_row.register_rules[*register as usize] =
                        RegisterRule::Offset(real_offset);
                }
                Instruction::Restore { register } => {
                    debug!("Restre(register={})", *register);
                    let first_rule = self
                        .rows
                        .first()
                        .expect("Rows must not be empty during Restore");
                    current_row.register_rules[*register as usize] =
                        first_rule.register_rules[*register as usize];
                }
                Instruction::DefCfa { register, offset } => {
                    debug!("DefCfa(register={}, offset={})", *register, *offset);
                    current_row.cfa_register = u64::from(*register);
                    current_row.cfa_offset =
                        i64::try_from(*offset).expect("DWARF CFA offset fits in i64");
                }
                Instruction::DefCfaRegister { register } => {
                    debug!("DefCfaRegister(register={})", *register);
                    current_row.cfa_register = u64::from(*register);
                }
                Instruction::DefCfaOffset { offset } => {
                    debug!("DefCfaOffset(offset={})", *offset);
                    current_row.cfa_offset =
                        i64::try_from(*offset).expect("DWARF CFA offset fits in i64");
                }
                Instruction::RemeberState => {
                    self.row_stack
                        .push(current_row.clone())
                        .expect("There must be enough space on the row stack.");
                }
                Instruction::RestoreState => {
                    let row = self
                        .row_stack
                        .pop()
                        .expect("There must be a row on the stack.");
                    let start_address = row.start_address;
                    current_row = row;
                    current_row.start_address = start_address;
                }
                Instruction::Nop => {}
            }
        }
        self.update_last_row(&current_row);
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Row {
    pub start_address: usize,
    pub end_address: usize,
    pub cfa_register: u64,
    pub cfa_offset: i64,
    pub register_rules: [RegisterRule; 32], // Not sure how many registers are defined; we will check that later
}

impl core::fmt::Display for Row {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "Row[")?;
        writeln!(f, "address: {:#x}", self.start_address)?;
        writeln!(f, "cfa register: {:?}", self.cfa_register)?;
        writeln!(f, "cfa offset: {:?}", self.cfa_offset)?;
        writeln!(f, "register rules:")?;
        for (index, rule) in self
            .register_rules
            .iter()
            .filter(|&&r| r != RegisterRule::None)
            .enumerate()
        {
            writeln!(f, "\t{index} {rule:?}")?;
        }

        writeln!(f, "]")
    }
}

impl Row {
    fn new(address: usize) -> Self {
        Self {
            start_address: address,
            end_address: 0,
            cfa_register: 0,
            cfa_offset: 0,
            register_rules: [RegisterRule::None; 32],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterRule {
    None,
    Offset(i64),
}

#[cfg(test)]
impl PartialEq<gimli::RegisterRule<usize>> for RegisterRule {
    fn eq(&self, other: &gimli::RegisterRule<usize>) -> bool {
        match self {
            RegisterRule::Offset(offset) => {
                matches!(other, gimli::RegisterRule::Offset(control_offset) if offset == control_offset)
            }
            _ => panic!("Register rule not implemented."),
        }
    }
}
