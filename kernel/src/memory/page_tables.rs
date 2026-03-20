#![allow(unsafe_code)]
use core::{
    fmt::{Debug, Display},
    ops::Range,
    ptr::null_mut,
};

use crate::klibc::util::align_up;
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use common::{pointer::Pointer, unwrap_or_return};

use crate::{
    assert::static_assert_size,
    debug, debugging,
    klibc::{
        sizes::{GiB, MiB},
        util::is_aligned,
    },
    memory::page::PAGE_SIZE,
};

pub use super::page_table_entry::XWRMode;
use super::{
    address::{PhysAddr, VirtAddr},
    heap_size,
    linker_information::LinkerInformation,
    page::Page,
    page_table_entry::PageTableEntry,
};

#[derive(Clone)]
pub struct MappingDescription {
    pub virtual_address_start: VirtAddr,
    pub size: usize,
    pub privileges: XWRMode,
    pub name: &'static str,
}

/// Keeps track of already mapped virtual address ranges
/// We use that to prevent of overlapping mapping
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
    root_table: *mut PageTable,
    already_mapped: Vec<MappingEntry>,
}

// SAFETY: RootPageTableHolder owns its page table tree. The raw pointer is
// not shared; only one owner exists per process/kernel instance.
unsafe impl Send for RootPageTableHolder {}

impl Debug for RootPageTableHolder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let page_table = self.table();
        write!(f, "RootPageTableHolder({page_table:p})")
    }
}

impl Display for RootPageTableHolder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "Pagetables at {:p}", self.root_table)?;
        for mapping in &self.already_mapped {
            writeln!(f, "{mapping}")?;
        }
        Ok(())
    }
}

impl Drop for RootPageTableHolder {
    fn drop(&mut self) {
        assert!(!self.is_active(), "Page table is dropped while active");
        let table = self.table();
        for first_level_entry in table.0.iter() {
            if !first_level_entry.get_validity() || first_level_entry.is_leaf() {
                continue;
            }
            let second_level_ptr = first_level_entry.get_target_page_table();
            // SAFETY: The pointer is valid because we checked validity above, and we
            // own these page tables (Drop guarantees exclusive access).
            let second_level_table = unsafe { &*second_level_ptr };
            for second_level_entry in second_level_table.0.iter() {
                if !second_level_entry.get_validity() || second_level_entry.is_leaf() {
                    continue;
                }
                // SAFETY: Each third-level table was allocated with Box::new in map().
                let _ = unsafe { Box::from_raw(second_level_entry.get_target_page_table()) };
            }
            // SAFETY: Each second-level table was allocated with Box::new in map().
            let _ = unsafe { Box::from_raw(second_level_ptr) };
        }
        // SAFETY: The root table was allocated with Box::new in empty().
        let _ = unsafe { Box::from_raw(self.root_table) };
        self.root_table = null_mut();
    }
}

impl RootPageTableHolder {
    fn empty() -> Self {
        let root_table = Box::leak(Box::new(PageTable::zero()));
        Self {
            root_table,
            already_mapped: Vec::new(),
        }
    }

    fn table(&self) -> &PageTable {
        // SAFETY: It is always allocated
        unsafe { &*self.root_table }
    }

    fn table_mut(&mut self) -> &mut PageTable {
        // SAFETY: It is always allocated
        unsafe { &mut *self.root_table }
    }

    fn is_active(&self) -> bool {
        let satp = arch::cpu::read_satp();
        let ppn = satp & 0xfffffffffff;
        let page_table_address = ppn << 12;

        let current_physical_address = self.table().get_physical_address();

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

    fn get_page_table_entry_for_address(&self, address: VirtAddr) -> Option<&PageTableEntry> {
        let root_page_table = self.table();

        let first_level_entry =
            root_page_table.get_entry_for_virtual_address(address.as_usize(), 2);
        if !first_level_entry.get_validity() {
            return None;
        }

        // SAFETY: The pointer is valid because we checked the entry's validity bit above,
        // and &self guarantees no concurrent mutation.
        let second_level_entry = unsafe { &*first_level_entry.get_target_page_table() }
            .get_entry_for_virtual_address(address.as_usize(), 1);
        if !second_level_entry.get_validity() {
            return None;
        }

        // SAFETY: Same as above — validity checked, &self guarantees no concurrent mutation.
        let third_level_entry = unsafe { &*second_level_entry.get_target_page_table() }
            .get_entry_for_virtual_address(address.as_usize(), 0);
        if !third_level_entry.get_validity() {
            return None;
        }

        Some(third_level_entry)
    }

    fn get_page_table_entry_for_address_mut(
        &mut self,
        address: VirtAddr,
    ) -> Option<&mut PageTableEntry> {
        let root_page_table = self.table_mut();

        let first_level_entry =
            root_page_table.get_entry_for_virtual_address_mut(address.as_usize(), 2);
        if !first_level_entry.get_validity() {
            return None;
        }

        // SAFETY: Entry is valid and non-leaf; &mut self guarantees exclusive access.
        let second_level_entry = unsafe { &mut *first_level_entry.get_target_page_table() }
            .get_entry_for_virtual_address_mut(address.as_usize(), 1);
        if !second_level_entry.get_validity() {
            return None;
        }

        // SAFETY: Same as above.
        let third_level_entry = unsafe { &mut *second_level_entry.get_target_page_table() }
            .get_entry_for_virtual_address_mut(address.as_usize(), 0);
        if !third_level_entry.get_validity() {
            return None;
        }

        Some(third_level_entry)
    }

    pub fn mprotect(&mut self, addr: VirtAddr, size: usize, mode: XWRMode) {
        assert!(addr.is_page_aligned());
        assert!(size > 0 && size.is_multiple_of(PAGE_SIZE));

        let mut offset = 0;
        while offset < size {
            let page_addr = addr + offset;
            let pte = self
                .get_page_table_entry_for_address_mut(page_addr)
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

        // Check if we have an overlapping mapping
        let already_mapped = self
            .already_mapped
            .iter()
            .find(|m| m.contains(&(virtual_address_start..virtual_end)));

        if let Some(mapping) = already_mapped {
            panic!("Cannot map {}. Overlaps with {}", name, mapping.name);
        }

        // Add mapping
        self.already_mapped.push(MappingEntry::new(
            virtual_address_start..virtual_end,
            name,
            privileges,
        ));

        let root_page_table = self.table_mut();

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

        // Any level of PTE can be a leaf PTE
        // So we can have 4KiB pages, 2MiB pages, and 1GiB pages in the same page table
        // They have to be aligned on 4KiB, 2MiB, and 1GiB boundaries respectively
        // We try to be smart and save memory by mapping as least as possible

        while offset < size {
            // Check if we can map a 1GiB page
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

            // Check if we can map a 2MiB page
            if can_be_mapped_with(MiB(2), offset) {
                let first_level_entry = root_page_table
                    .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 2);
                if first_level_entry.get_physical_address() == PhysAddr::zero() {
                    let page = Box::leak(Box::new(PageTable::zero()));
                    first_level_entry.set_physical_address(page);
                    first_level_entry.set_validity(true);
                }

                // SAFETY: We just ensured the entry is valid and points to an allocated table.
                // &mut self guarantees exclusive access to the page table hierarchy.
                let second_level_entry = unsafe { &mut *first_level_entry.get_target_page_table() }
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

            // Map single page
            let first_level_entry = root_page_table
                .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 2);
            if first_level_entry.get_physical_address() == PhysAddr::zero() {
                let page = Box::leak(Box::new(PageTable::zero()));
                first_level_entry.set_physical_address(page);
                first_level_entry.set_validity(true);
            }

            // SAFETY: We just ensured the entry is valid and points to an allocated table.
            // &mut self guarantees exclusive access to the page table hierarchy.
            let second_level_entry = unsafe { &mut *first_level_entry.get_target_page_table() }
                .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 1);
            if second_level_entry.get_physical_address() == PhysAddr::zero() {
                let page = Box::leak(Box::new(PageTable::zero()));
                second_level_entry.set_physical_address(page);
                second_level_entry.set_validity(true);
            }

            // SAFETY: Same as above — entry is valid and allocated.
            let third_level_entry = unsafe { &mut *second_level_entry.get_target_page_table() }
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
        self.get_page_table_entry_for_address(address)
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
        // We only need to check for each PAGE_SIZE step if it is mapped
        let mut addr = first_page;
        loop {
            let entry = unwrap_or_return!(
                self.get_page_table_entry_for_address(VirtAddr::new(addr)),
                false
            );
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
        self.get_page_table_entry_for_address(va)
            .map(|e| e.get_xwr_mode())
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
        self.get_page_table_entry_for_address(VirtAddr::new(address))
            .map(|entry| {
                PTR::as_pointer((entry.get_physical_address() + offset_from_page_start).as_usize())
            })
    }

    pub fn get_satp_value_from_page_tables(&self) -> usize {
        let page_table_address = self.table().get_physical_address();

        let page_table_address_shifted = page_table_address.as_usize() >> 12;

        (8 << 60) | (page_table_address_shifted & 0xfffffffffff)
    }

    pub fn activate_page_table(&self) {
        let page_table_address = self.table().get_physical_address();

        debug!(
            "Activate new page mapping (Addr of page tables {})",
            page_table_address
        );

        let satp_val = self.get_satp_value_from_page_tables();

        // SAFETY: satp_val encodes a valid page table that identity-maps all
        // kernel memory, so execution can continue after the switch.
        unsafe {
            arch::cpu::write_satp_and_fence(satp_val);
        };
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

        let root_page_table = self.table_mut();
        let mut offset = 0;

        while offset < size {
            let addr = (virtual_address_start + offset).as_usize();
            let first_level_entry = root_page_table.get_entry_for_virtual_address_mut(addr, 2);

            assert!(
                first_level_entry.get_validity(),
                "unmap_userspace: first-level PTE not valid at {addr:#x}"
            );

            if first_level_entry.is_leaf() {
                *first_level_entry = PageTableEntry(null_mut());
                offset += GiB(1);
                continue;
            }

            // SAFETY: Entry is valid and non-leaf, so it points to an allocated second-level table.
            let second_level_entry = unsafe { &mut *first_level_entry.get_target_page_table() }
                .get_entry_for_virtual_address_mut(addr, 1);

            assert!(
                second_level_entry.get_validity(),
                "unmap_userspace: second-level PTE not valid at {addr:#x}"
            );

            if second_level_entry.is_leaf() {
                *second_level_entry = PageTableEntry(null_mut());
                offset += MiB(2);
                continue;
            }

            // SAFETY: Entry is valid and non-leaf, so it points to an allocated third-level table.
            let third_level_entry = unsafe { &mut *second_level_entry.get_target_page_table() }
                .get_entry_for_virtual_address_mut(addr, 0);

            assert!(
                third_level_entry.get_validity(),
                "unmap_userspace: third-level PTE not valid at {addr:#x}"
            );

            *third_level_entry = PageTableEntry(null_mut());
            offset += PAGE_SIZE;
        }
    }

    pub fn is_mapped(&self, range: Range<VirtAddr>) -> bool {
        self.already_mapped.iter().any(|m| m.contains(&range))
    }
}

#[repr(C, align(4096))]
#[derive(Debug)]
pub(super) struct PageTable(pub(super) [PageTableEntry; 512]);

static_assert_size!(PageTable, core::mem::size_of::<Page>());

impl PageTable {
    pub(super) fn zero() -> Self {
        Self([PageTableEntry(null_mut()); 512])
    }

    pub(super) fn get_entry_for_virtual_address_mut(
        &mut self,
        virtual_address: usize,
        level: u8,
    ) -> &mut PageTableEntry {
        assert!(level <= 2);
        let shifted_address = virtual_address >> (12 + 9 * level);
        let index = shifted_address & 0x1ff;
        &mut self.0[index]
    }

    pub(super) fn get_entry_for_virtual_address(
        &self,
        virtual_address: usize,
        level: u8,
    ) -> &PageTableEntry {
        assert!(level <= 2);
        let shifted_address = virtual_address >> (12 + 9 * level);
        let index = shifted_address & 0x1ff;
        &self.0[index]
    }

    fn get_physical_address(&self) -> PhysAddr {
        PhysAddr::new((self as *const Self).addr())
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::{MappingEntry, RootPageTableHolder};
    use crate::memory::{PhysAddr, VirtAddr, page_table_entry::XWRMode};
    use alloc::string::ToString;

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
        // Contained
        assert!(m.contains(&(VirtAddr::new(120)..VirtAddr::new(180))));
        // Left overlap
        assert!(m.contains(&(VirtAddr::new(50)..VirtAddr::new(150))));
        // Right overlap
        assert!(m.contains(&(VirtAddr::new(150)..VirtAddr::new(250))));
        // Enclosing
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
        assert!(
            pt.get_page_table_entry_for_address(VirtAddr::new(0x1000))
                .is_some()
        );
        pt.unmap_userspace(VirtAddr::new(0x1000), 0x1000);
        assert!(
            pt.get_page_table_entry_for_address(VirtAddr::new(0x1000))
                .is_none()
        );
    }
}
