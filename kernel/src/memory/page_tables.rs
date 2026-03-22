use core::{
    fmt::{Debug, Display},
    ops::Range,
};

use crate::klibc::util::align_up;
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use common::{pointer::Pointer, unwrap_or_return};

use crate::{
    debug, debugging,
    klibc::{
        sizes::{GiB, MiB},
        util::is_aligned,
    },
    memory::PAGE_SIZE,
};

use super::{PhysAddr, VirtAddr, heap_size, linker_information::LinkerInformation};
pub use sys::memory::page_table::XWRMode;
use sys::memory::page_table::{OwnedPageTable, PageTable, PageTableEntry, activate_page_table};

#[derive(Clone)]
pub struct MappingDescription {
    pub virtual_address_start: VirtAddr,
    pub size: usize,
    pub privileges: XWRMode,
    pub name: &'static str,
}

struct MappingEntry {
    virtual_range: core::ops::Range<VirtAddr>,
    name: String,
    privileges: XWRMode,
}

impl MappingEntry {
    fn new(virtual_range: Range<VirtAddr>, name: String, privileges: XWRMode) -> Self {
        Self {
            virtual_range,
            name,
            privileges,
        }
    }

    fn contains(&self, range: &Range<VirtAddr>) -> bool {
        self.virtual_range.start <= range.end && range.start <= self.virtual_range.end
    }
}

impl Display for MappingEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}-{} (Size: {:#010x}) ({:?})\t({})",
            self.virtual_range.start,
            self.virtual_range.end,
            self.virtual_range.end - self.virtual_range.start,
            self.privileges,
            self.name
        )
    }
}

pub struct RootPageTableHolder {
    root_table: OwnedPageTable,
    already_mapped: Vec<MappingEntry>,
}

impl Debug for RootPageTableHolder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RootPageTableHolder({})", self.root_table)
    }
}

impl Display for RootPageTableHolder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "Pagetables at {:p}", self.root_table.as_raw())?;
        for mapping in &self.already_mapped {
            writeln!(f, "{mapping}")?;
        }
        Ok(())
    }
}

impl Drop for RootPageTableHolder {
    fn drop(&mut self) {
        assert!(!self.is_active(), "Page table is dropped while active");
        self.root_table.reclaim_children();
        // Root table is freed by OwnedPageTable's Drop
    }
}

impl RootPageTableHolder {
    fn empty() -> Self {
        Self {
            root_table: OwnedPageTable::new(),
            already_mapped: Vec::new(),
        }
    }

    fn is_active(&self) -> bool {
        let satp = arch::cpu::read_satp();
        let ppn = satp & 0xfffffffffff;
        let page_table_address = ppn << 12;

        let current_physical_address = self.root_table.get_physical_address();

        debug!(
            "is_active: satp: {:x}; page_table_address: {}",
            satp, current_physical_address
        );
        page_table_address == current_physical_address.as_usize()
    }

    pub fn new_with_kernel_mapping(extra_mappings: &[MappingDescription]) -> Self {
        let mut root_page_table_holder = RootPageTableHolder::empty();

        for mapping in LinkerInformation::all_mappings() {
            root_page_table_holder.map_identity_kernel(
                mapping.virtual_address_start,
                mapping.size,
                mapping.privileges,
                mapping.name.to_string(),
            );
        }

        root_page_table_holder.map_identity_kernel(
            LinkerInformation::__start_symbols(),
            debugging::symbols::symbols_size(),
            XWRMode::ReadOnly,
            "SYMBOLS".to_string(),
        );

        root_page_table_holder.map_identity_kernel(
            LinkerInformation::__start_heap(),
            heap_size(),
            XWRMode::ReadWrite,
            "HEAP".to_string(),
        );

        for mapping in extra_mappings {
            root_page_table_holder.map_identity_kernel(
                mapping.virtual_address_start,
                mapping.size,
                mapping.privileges,
                mapping.name.to_string(),
            );
        }

        root_page_table_holder
    }

    pub fn map_userspace(
        &mut self,
        virtual_address_start: VirtAddr,
        physical_address_start: PhysAddr,
        size: usize,
        privileges: XWRMode,
        name: String,
    ) {
        self.map(
            virtual_address_start,
            physical_address_start,
            size,
            privileges,
            true,
            name,
        );
    }

    fn walk_to_entry(&self, address: VirtAddr) -> Option<&PageTableEntry> {
        let first = self
            .root_table
            .get_entry_for_virtual_address(address.as_usize(), 2);
        if !first.get_validity() {
            return None;
        }
        let second = first
            .target_page_table()
            .get_entry_for_virtual_address(address.as_usize(), 1);
        if !second.get_validity() {
            return None;
        }
        let third = second
            .target_page_table()
            .get_entry_for_virtual_address(address.as_usize(), 0);
        if !third.get_validity() {
            return None;
        }
        Some(third)
    }

    fn walk_to_entry_mut(&mut self, address: VirtAddr) -> Option<&mut PageTableEntry> {
        let first = self
            .root_table
            .get_entry_for_virtual_address_mut(address.as_usize(), 2);
        if !first.get_validity() {
            return None;
        }
        let second = first
            .target_page_table_mut()
            .get_entry_for_virtual_address_mut(address.as_usize(), 1);
        if !second.get_validity() {
            return None;
        }
        let third = second
            .target_page_table_mut()
            .get_entry_for_virtual_address_mut(address.as_usize(), 0);
        if !third.get_validity() {
            return None;
        }
        Some(third)
    }

    pub fn mprotect(&mut self, addr: VirtAddr, size: usize, mode: XWRMode) {
        assert!(addr.is_page_aligned());
        assert!(size > 0 && size.is_multiple_of(PAGE_SIZE));

        let mut offset = 0;
        while offset < size {
            let page_addr = addr + offset;
            let pte = self
                .walk_to_entry_mut(page_addr)
                .expect("mprotect: page not mapped");
            pte.set_xwr_mode(mode);
            offset += PAGE_SIZE;
        }

        for entry in &mut self.already_mapped {
            if entry.virtual_range.start <= addr && addr < entry.virtual_range.end {
                entry.privileges = mode;
                break;
            }
        }
    }

    pub fn map(
        &mut self,
        virtual_address_start: VirtAddr,
        physical_address_start: PhysAddr,
        mut size: usize,
        privileges: XWRMode,
        is_user_mode_accessible: bool,
        name: String,
    ) {
        assert!(virtual_address_start.is_page_aligned());
        assert!(physical_address_start.is_page_aligned());
        assert!(
            virtual_address_start != VirtAddr::zero(),
            "It is dangerous to map the null pointer."
        );
        assert!(size > 0);

        size = align_up(size, PAGE_SIZE);

        let virtual_end = virtual_address_start + (size - 1);
        let physical_end = physical_address_start + (size - 1);

        debug!(
            "Map {}-{} -> {}-{} (Size: {:#010x}) ({:?})\t({})",
            virtual_address_start,
            virtual_end,
            physical_address_start,
            physical_end,
            size,
            privileges,
            name
        );

        let already_mapped = self
            .already_mapped
            .iter()
            .find(|m| m.contains(&(virtual_address_start..virtual_end)));

        if let Some(mapping) = already_mapped {
            panic!("Cannot map {}. Overlaps with {}", name, mapping.name);
        }

        self.already_mapped.push(MappingEntry::new(
            virtual_address_start..virtual_end,
            name,
            privileges,
        ));

        let root_page_table = &mut *self.root_table;

        let mut offset = 0;

        let virtual_address_with_offset = |offset| (virtual_address_start + offset).as_usize();
        let physical_address_with_offset = |offset| physical_address_start + offset;

        let can_be_mapped_with = |mapped_bytes, offset| {
            mapped_bytes <= (size - offset)
                && is_aligned(virtual_address_with_offset(offset), mapped_bytes)
                && is_aligned(
                    physical_address_with_offset(offset).as_usize(),
                    mapped_bytes,
                )
        };

        while offset < size {
            if can_be_mapped_with(GiB(1), offset) {
                let first_level_entry = root_page_table
                    .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 2);

                assert!(
                    !first_level_entry.get_validity()
                        && first_level_entry.get_physical_address() == PhysAddr::zero(),
                    "Entry must be an invalid value and physical address must be zero"
                );
                first_level_entry.set_xwr_mode(privileges);
                first_level_entry.set_validity(true);
                first_level_entry.set_leaf_address(physical_address_with_offset(offset));
                first_level_entry.set_user_mode_accessible(is_user_mode_accessible);
                offset += GiB(1);
                continue;
            }

            if can_be_mapped_with(MiB(2), offset) {
                let first_level_entry = root_page_table
                    .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 2);
                if first_level_entry.get_physical_address() == PhysAddr::zero() {
                    let page = Box::leak(Box::new(PageTable::zero()));
                    first_level_entry.set_physical_address(page);
                    first_level_entry.set_validity(true);
                }

                let second_level_entry = first_level_entry
                    .target_page_table_mut()
                    .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 1);
                assert!(
                    !second_level_entry.get_validity()
                        && second_level_entry.get_physical_address() == PhysAddr::zero(),
                    "Entry must be an invalid value and physical address must be zero"
                );

                second_level_entry.set_xwr_mode(privileges);
                second_level_entry.set_validity(true);
                second_level_entry.set_leaf_address(physical_address_with_offset(offset));
                second_level_entry.set_user_mode_accessible(is_user_mode_accessible);
                offset += MiB(2);
                continue;
            }

            assert!(
                is_aligned(virtual_address_with_offset(offset), PAGE_SIZE),
                "Virtual address must be aligned with page size"
            );
            assert!(
                is_aligned(physical_address_with_offset(offset).as_usize(), PAGE_SIZE),
                "Physical address must be aligned with page size"
            );

            let first_level_entry = root_page_table
                .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 2);
            if first_level_entry.get_physical_address() == PhysAddr::zero() {
                let page = Box::leak(Box::new(PageTable::zero()));
                first_level_entry.set_physical_address(page);
                first_level_entry.set_validity(true);
            }

            let second_level_entry = first_level_entry
                .target_page_table_mut()
                .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 1);
            if second_level_entry.get_physical_address() == PhysAddr::zero() {
                let page = Box::leak(Box::new(PageTable::zero()));
                second_level_entry.set_physical_address(page);
                second_level_entry.set_validity(true);
            }

            let third_level_entry = second_level_entry
                .target_page_table_mut()
                .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 0);

            assert!(!third_level_entry.get_validity());

            third_level_entry.set_xwr_mode(privileges);
            third_level_entry.set_validity(true);
            third_level_entry.set_leaf_address(physical_address_with_offset(offset));
            third_level_entry.set_user_mode_accessible(is_user_mode_accessible);

            offset += PAGE_SIZE;
        }
    }

    pub fn map_identity_kernel(
        &mut self,
        virtual_address_start: VirtAddr,
        size: usize,
        privileges: XWRMode,
        name: String,
    ) {
        self.map_identity(virtual_address_start, size, privileges, false, name);
    }

    fn map_identity(
        &mut self,
        virtual_address_start: VirtAddr,
        size: usize,
        privileges: XWRMode,
        is_user_mode_accessible: bool,
        name: String,
    ) {
        self.map(
            virtual_address_start,
            PhysAddr::new(virtual_address_start.as_usize()),
            size,
            privileges,
            is_user_mode_accessible,
            name,
        );
    }

    pub fn is_userspace_address(&self, address: VirtAddr) -> bool {
        self.walk_to_entry(address)
            .is_some_and(|entry| entry.get_validity() && entry.get_user_mode_accessible())
    }

    pub fn is_valid_userspace_fat_ptr<PTR: Pointer>(
        &self,
        ptr: PTR,
        len: usize,
        writable: bool,
    ) -> bool {
        let start = ptr.as_raw();
        let Some(byte_len) = core::mem::size_of::<PTR::Pointee>().checked_mul(len) else {
            return false;
        };
        if byte_len == 0 {
            return true;
        }
        let Some(last) = start.checked_add(byte_len - 1) else {
            return false;
        };
        let first_page = start & !(PAGE_SIZE - 1);
        let last_page = last & !(PAGE_SIZE - 1);
        let mut addr = first_page;
        loop {
            let entry = unwrap_or_return!(self.walk_to_entry(VirtAddr::new(addr)), false);
            let xwr = entry.get_xwr_mode();
            if !entry.get_validity()
                || !entry.get_user_mode_accessible()
                || !matches!(
                    xwr,
                    XWRMode::ReadOnly | XWRMode::ReadWrite | XWRMode::ReadExecute
                )
            {
                return false;
            }
            if writable && !matches!(xwr, XWRMode::ReadWrite) {
                return false;
            }
            if addr == last_page {
                break;
            }
            addr += PAGE_SIZE;
        }
        true
    }

    pub fn get_userspace_permissions(&self, va: VirtAddr) -> Option<XWRMode> {
        self.walk_to_entry(va).map(|e| e.get_xwr_mode())
    }

    pub fn is_valid_userspace_ptr(&self, ptr: impl Pointer, writable: bool) -> bool {
        self.is_valid_userspace_fat_ptr(ptr, 1, writable)
    }

    pub fn translate_userspace_address_to_physical_address<PTR: Pointer>(
        &self,
        ptr: PTR,
    ) -> Option<PTR> {
        let address = ptr.as_raw();
        if !self.is_userspace_address(VirtAddr::new(address)) {
            return None;
        }

        let offset_from_page_start = address % PAGE_SIZE;
        self.walk_to_entry(VirtAddr::new(address)).map(|entry| {
            PTR::as_pointer((entry.get_physical_address() + offset_from_page_start).as_usize())
        })
    }

    pub fn get_satp_value_from_page_tables(&self) -> usize {
        let page_table_address = self.root_table.get_physical_address();

        let page_table_address_shifted = page_table_address.as_usize() >> 12;

        (8 << 60) | (page_table_address_shifted & 0xfffffffffff)
    }

    pub fn activate_page_table(&self) {
        let page_table_address = self.root_table.get_physical_address();

        debug!(
            "Activate new page mapping (Addr of page tables {})",
            page_table_address
        );

        let satp_val = self.get_satp_value_from_page_tables();
        activate_page_table(satp_val);
    }

    pub fn unmap_userspace(&mut self, virtual_address_start: VirtAddr, size: usize) {
        assert!(virtual_address_start.is_page_aligned());
        assert!(size > 0);

        let idx = self
            .already_mapped
            .iter()
            .position(|m| m.virtual_range.start == virtual_address_start);
        let idx = idx.expect("unmap_userspace: no mapping at this address");
        self.already_mapped.swap_remove(idx);

        let root_page_table = &mut *self.root_table;
        let mut offset = 0;

        while offset < size {
            let addr = (virtual_address_start + offset).as_usize();
            let first_level_entry = root_page_table.get_entry_for_virtual_address_mut(addr, 2);

            assert!(
                first_level_entry.get_validity(),
                "unmap_userspace: first-level PTE not valid at {addr:#x}"
            );

            if first_level_entry.is_leaf() {
                *first_level_entry = PageTableEntry(core::ptr::null_mut());
                offset += GiB(1);
                continue;
            }

            let second_level_entry = first_level_entry
                .target_page_table_mut()
                .get_entry_for_virtual_address_mut(addr, 1);

            assert!(
                second_level_entry.get_validity(),
                "unmap_userspace: second-level PTE not valid at {addr:#x}"
            );

            if second_level_entry.is_leaf() {
                *second_level_entry = PageTableEntry(core::ptr::null_mut());
                offset += MiB(2);
                continue;
            }

            let third_level_entry = second_level_entry
                .target_page_table_mut()
                .get_entry_for_virtual_address_mut(addr, 0);

            assert!(
                third_level_entry.get_validity(),
                "unmap_userspace: third-level PTE not valid at {addr:#x}"
            );

            *third_level_entry = PageTableEntry(core::ptr::null_mut());
            offset += PAGE_SIZE;
        }
    }

    pub fn is_mapped(&self, range: Range<VirtAddr>) -> bool {
        self.already_mapped.iter().any(|m| m.contains(&range))
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::{MappingEntry, RootPageTableHolder};
    use crate::memory::{PhysAddr, VirtAddr};
    use alloc::string::ToString;
    use sys::memory::page_table::XWRMode;

    #[test_case]
    fn check_drop_of_page_table_holder() {
        let mut page_table = RootPageTableHolder::empty();
        page_table.map_userspace(
            VirtAddr::new(0x1000),
            PhysAddr::new(0x2000),
            0x3000,
            XWRMode::ReadOnly,
            "Test".to_string(),
        );
    }

    #[test_case]
    fn mapping_entry_overlap_detection() {
        let m = MappingEntry::new(
            VirtAddr::new(100)..VirtAddr::new(200),
            "test".to_string(),
            XWRMode::ReadOnly,
        );
        assert!(m.contains(&(VirtAddr::new(120)..VirtAddr::new(180))));
        assert!(m.contains(&(VirtAddr::new(50)..VirtAddr::new(150))));
        assert!(m.contains(&(VirtAddr::new(150)..VirtAddr::new(250))));
        assert!(m.contains(&(VirtAddr::new(50)..VirtAddr::new(250))));
    }

    #[test_case]
    fn mapping_entry_no_overlap() {
        let m = MappingEntry::new(
            VirtAddr::new(100)..VirtAddr::new(200),
            "test".to_string(),
            XWRMode::ReadOnly,
        );
        assert!(!m.contains(&(VirtAddr::new(0)..VirtAddr::new(99))));
        assert!(!m.contains(&(VirtAddr::new(201)..VirtAddr::new(300))));
    }

    #[test_case]
    fn is_mapped_after_map() {
        let mut pt = RootPageTableHolder::empty();
        pt.map_userspace(
            VirtAddr::new(0x1000),
            PhysAddr::new(0x2000),
            0x1000,
            XWRMode::ReadWrite,
            "A".to_string(),
        );
        assert!(pt.is_mapped(VirtAddr::new(0x1000)..VirtAddr::new(0x1FFF)));
        assert!(!pt.is_mapped(VirtAddr::new(0x3000)..VirtAddr::new(0x3FFF)));
    }

    #[test_case]
    fn unmap_userspace_clears_mapping() {
        let mut pt = RootPageTableHolder::empty();
        pt.map_userspace(
            VirtAddr::new(0x1000),
            PhysAddr::new(0x2000),
            0x1000,
            XWRMode::ReadWrite,
            "A".to_string(),
        );
        assert!(pt.is_mapped(VirtAddr::new(0x1000)..VirtAddr::new(0x1FFF)));
        pt.unmap_userspace(VirtAddr::new(0x1000), 0x1000);
        assert!(!pt.is_mapped(VirtAddr::new(0x1000)..VirtAddr::new(0x1FFF)));
    }

    #[test_case]
    fn unmap_userspace_clears_pte_validity() {
        let mut pt = RootPageTableHolder::empty();
        pt.map_userspace(
            VirtAddr::new(0x1000),
            PhysAddr::new(0x2000),
            0x1000,
            XWRMode::ReadWrite,
            "A".to_string(),
        );
        assert!(pt.walk_to_entry(VirtAddr::new(0x1000)).is_some());
        pt.unmap_userspace(VirtAddr::new(0x1000), 0x1000);
        assert!(pt.walk_to_entry(VirtAddr::new(0x1000)).is_none());
    }
}
