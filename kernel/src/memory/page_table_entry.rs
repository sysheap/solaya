#[cfg(any(kani, test))]
use sys::memory::page_table::PageTableEntry;
pub use sys::memory::page_table::XWRMode;

use crate::klibc::elf;

impl From<elf::ProgramHeaderFlags> for XWRMode {
    fn from(value: elf::ProgramHeaderFlags) -> Self {
        match value {
            elf::ProgramHeaderFlags::RW => Self::ReadWrite,
            elf::ProgramHeaderFlags::RWX => Self::ReadWriteExecute,
            elf::ProgramHeaderFlags::RX => Self::ReadExecute,
            elf::ProgramHeaderFlags::X => Self::ExecuteOnly,
            elf::ProgramHeaderFlags::W => panic!("Cannot map W flag"),
            elf::ProgramHeaderFlags::WX => panic!("Cannot map WX flag"),
            elf::ProgramHeaderFlags::R => Self::ReadOnly,
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use sys::memory::address::PhysAddr;

    fn entry_from_bits(bits: usize) -> PageTableEntry {
        PageTableEntry(core::ptr::without_provenance_mut(bits))
    }

    #[kani::proof]
    fn validity_bit_roundtrip() {
        let bits: usize = kani::any();
        let val: bool = kani::any();
        let mut entry = entry_from_bits(bits);
        entry.set_validity(val);
        assert!(entry.get_validity() == val);
    }

    #[kani::proof]
    fn xwr_mode_roundtrip() {
        let bits: usize = kani::any();
        let mode_idx: u8 = kani::any();
        kani::assume(mode_idx < 6);
        let modes = [
            XWRMode::PointerToNextLevel,
            XWRMode::ReadOnly,
            XWRMode::ReadWrite,
            XWRMode::ExecuteOnly,
            XWRMode::ReadExecute,
            XWRMode::ReadWriteExecute,
        ];
        let mode = modes[mode_idx as usize];
        let mut entry = entry_from_bits(bits);
        entry.set_xwr_mode(mode);
        assert!(entry.get_xwr_mode() == mode);
    }

    #[kani::proof]
    fn user_mode_bit_roundtrip() {
        let bits: usize = kani::any();
        let val: bool = kani::any();
        let mut entry = entry_from_bits(bits);
        entry.set_user_mode_accessible(val);
        assert!(entry.get_user_mode_accessible() == val);
    }

    #[kani::proof]
    fn leaf_address_roundtrip() {
        let ppn: usize = kani::any();
        kani::assume(ppn <= 0x3FFFFFFFFF);
        let addr = PhysAddr::new(ppn << 12);
        let mut entry = entry_from_bits(0);
        entry.set_leaf_address(addr);
        assert!(entry.get_physical_address() == addr);
    }

    #[kani::proof]
    fn set_leaf_address_preserves_low_bits() {
        let ppn: usize = kani::any();
        kani::assume(ppn <= 0x3FFFFFFFFF);
        let addr = PhysAddr::new(ppn << 12);
        let mut entry = entry_from_bits(0);
        entry.set_validity(true);
        entry.set_xwr_mode(XWRMode::ReadWrite);
        entry.set_user_mode_accessible(true);
        entry.set_leaf_address(addr);
        assert!(entry.get_validity());
        assert!(entry.get_xwr_mode() == XWRMode::ReadWrite);
        assert!(entry.get_user_mode_accessible());
    }

    #[kani::proof]
    fn is_leaf_matches_xwr_mode() {
        let bits: usize = kani::any();
        let mode_idx: u8 = kani::any();
        kani::assume(mode_idx < 6);
        let modes = [
            XWRMode::PointerToNextLevel,
            XWRMode::ReadOnly,
            XWRMode::ReadWrite,
            XWRMode::ExecuteOnly,
            XWRMode::ReadExecute,
            XWRMode::ReadWriteExecute,
        ];
        let mode = modes[mode_idx as usize];
        let mut entry = entry_from_bits(bits);
        entry.set_xwr_mode(mode);
        assert!(entry.is_leaf() == (mode != XWRMode::PointerToNextLevel));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::klibc::elf::ProgramHeaderFlags;
    use core::ptr::null_mut;
    use sys::memory::address::PhysAddr;

    #[test_case]
    fn page_table_entry_validity_bit() {
        let mut entry = PageTableEntry(null_mut());
        assert!(!entry.get_validity());
        entry.set_validity(true);
        assert!(entry.get_validity());
        entry.set_validity(false);
        assert!(!entry.get_validity());
    }

    #[test_case]
    fn page_table_entry_xwr_modes() {
        let modes = [
            XWRMode::PointerToNextLevel,
            XWRMode::ReadOnly,
            XWRMode::ReadWrite,
            XWRMode::ExecuteOnly,
            XWRMode::ReadExecute,
            XWRMode::ReadWriteExecute,
        ];
        for mode in modes {
            let mut entry = PageTableEntry(null_mut());
            entry.set_xwr_mode(mode);
            assert_eq!(entry.get_xwr_mode(), mode);
        }
    }

    #[test_case]
    fn page_table_entry_user_mode_bit() {
        let mut entry = PageTableEntry(null_mut());
        assert!(!entry.get_user_mode_accessible());
        entry.set_user_mode_accessible(true);
        assert!(entry.get_user_mode_accessible());
        entry.set_user_mode_accessible(false);
        assert!(!entry.get_user_mode_accessible());
    }

    #[test_case]
    fn page_table_entry_bits_are_independent() {
        let mut entry = PageTableEntry(null_mut());
        entry.set_validity(true);
        entry.set_xwr_mode(XWRMode::ReadWrite);
        entry.set_user_mode_accessible(true);
        assert!(entry.get_validity());
        assert_eq!(entry.get_xwr_mode(), XWRMode::ReadWrite);
        assert!(entry.get_user_mode_accessible());
    }

    #[test_case]
    fn page_table_entry_is_leaf() {
        let mut entry = PageTableEntry(null_mut());
        entry.set_xwr_mode(XWRMode::PointerToNextLevel);
        assert!(!entry.is_leaf());
        for mode in [
            XWRMode::ReadOnly,
            XWRMode::ReadWrite,
            XWRMode::ExecuteOnly,
            XWRMode::ReadExecute,
            XWRMode::ReadWriteExecute,
        ] {
            entry.set_xwr_mode(mode);
            assert!(entry.is_leaf());
        }
    }

    #[test_case]
    fn page_table_entry_leaf_address_roundtrip() {
        let mut entry = PageTableEntry(null_mut());
        let addr = PhysAddr::new(0x8020_0000);
        entry.set_leaf_address(addr);
        let got = entry.get_physical_address();
        assert_eq!(got, addr);
    }

    #[test_case]
    fn page_table_entry_leaf_address_preserves_low_bits() {
        let mut entry = PageTableEntry(null_mut());
        entry.set_validity(true);
        entry.set_xwr_mode(XWRMode::ReadWrite);
        entry.set_user_mode_accessible(true);
        entry.set_leaf_address(PhysAddr::new(0x8020_0000));
        assert!(entry.get_validity());
        assert_eq!(entry.get_xwr_mode(), XWRMode::ReadWrite);
        assert!(entry.get_user_mode_accessible());
    }

    #[test_case]
    fn xwr_mode_from_program_header_flags() {
        assert_eq!(XWRMode::from(ProgramHeaderFlags::R), XWRMode::ReadOnly);
        assert_eq!(XWRMode::from(ProgramHeaderFlags::RW), XWRMode::ReadWrite);
        assert_eq!(XWRMode::from(ProgramHeaderFlags::RX), XWRMode::ReadExecute);
        assert_eq!(XWRMode::from(ProgramHeaderFlags::X), XWRMode::ExecuteOnly);
        assert_eq!(
            XWRMode::from(ProgramHeaderFlags::RWX),
            XWRMode::ReadWriteExecute
        );
    }
}
