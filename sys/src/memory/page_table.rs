use alloc::boxed::Box;
use core::ptr::null_mut;

use super::address::PhysAddr;
use crate::klibc::util::{get_bit, get_multiple_bits, set_multiple_bits, set_or_clear_bit};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XWRMode {
    PointerToNextLevel = 0b000,
    ReadOnly = 0b001,
    ReadWrite = 0b011,
    ExecuteOnly = 0b100,
    ReadExecute = 0b101,
    ReadWriteExecute = 0b111,
}

impl From<u8> for XWRMode {
    fn from(value: u8) -> Self {
        match value {
            0b000 => Self::PointerToNextLevel,
            0b001 => Self::ReadOnly,
            0b011 => Self::ReadWrite,
            0b100 => Self::ExecuteOnly,
            0b101 => Self::ReadExecute,
            0b111 => Self::ReadWriteExecute,
            _ => panic!("Invalid XWR mode: {value:#05b}"),
        }
    }
}

impl XWRMode {
    pub fn is_writable(self) -> bool {
        matches!(self, Self::ReadWrite | Self::ReadWriteExecute)
    }

    pub fn as_readonly(self) -> Self {
        match self {
            Self::ReadWrite => Self::ReadOnly,
            Self::ReadWriteExecute => Self::ReadExecute,
            other => other,
        }
    }

    pub fn from_prot(prot: u32) -> Result<Self, headers::errno::Errno> {
        use headers::syscall_types::{PROT_EXEC, PROT_NONE, PROT_READ, PROT_WRITE};
        match prot {
            PROT_NONE | PROT_READ => Ok(Self::ReadOnly),
            PROT_EXEC => Ok(Self::ExecuteOnly),
            x if x == (PROT_READ | PROT_WRITE) => Ok(Self::ReadWrite),
            x if x == (PROT_READ | PROT_EXEC) => Ok(Self::ReadExecute),
            x if x == (PROT_READ | PROT_WRITE | PROT_EXEC) => Ok(Self::ReadWriteExecute),
            _ => Err(headers::errno::Errno::EINVAL),
        }
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct PageTableEntry(pub *mut PageTable);

impl PageTableEntry {
    const VALID_BIT_POS: usize = 0;
    const READ_BIT_POS: usize = 1;
    #[allow(dead_code)]
    const WRITE_BIT_POS: usize = 2;
    #[allow(dead_code)]
    const EXECUTE_BIT_POS: usize = 3;
    const USER_MODE_ACCESSIBLE_BIT_POS: usize = 4;
    const ACCESSED_BIT_POS: usize = 6;
    const DIRTY_BIT_POS: usize = 7;
    const PHYSICAL_PAGE_BIT_POS: usize = 10;
    const PHYSICAL_PAGE_BITS: usize = 0xfffffffffff;

    pub fn set_validity(&mut self, is_valid: bool) {
        self.0 = self
            .0
            .map_addr(|mut addr| set_or_clear_bit(&mut addr, is_valid, Self::VALID_BIT_POS));
    }

    pub fn get_validity(&self) -> bool {
        get_bit(self.0.addr(), Self::VALID_BIT_POS)
    }

    pub fn set_user_mode_accessible(&mut self, is_user_mode_accessible: bool) {
        self.0 = self.0.map_addr(|mut addr| {
            set_or_clear_bit(
                &mut addr,
                is_user_mode_accessible,
                Self::USER_MODE_ACCESSIBLE_BIT_POS,
            )
        });
    }

    pub fn get_user_mode_accessible(&self) -> bool {
        get_bit(self.0.addr(), Self::USER_MODE_ACCESSIBLE_BIT_POS)
    }

    pub fn set_accessed_and_dirty(&mut self) {
        self.0 = self
            .0
            .map_addr(|mut addr| set_or_clear_bit(&mut addr, true, Self::ACCESSED_BIT_POS));
        self.0 = self
            .0
            .map_addr(|mut addr| set_or_clear_bit(&mut addr, true, Self::DIRTY_BIT_POS));
    }

    pub fn set_xwr_mode(&mut self, mode: XWRMode) {
        self.0 = self
            .0
            .map_addr(|mut addr| set_multiple_bits(&mut addr, mode as u8, 3, Self::READ_BIT_POS));
    }

    pub fn get_xwr_mode(&self) -> XWRMode {
        let bits: u8 = u8::try_from(get_multiple_bits::<u64, u64>(
            self.0.addr() as u64,
            3,
            Self::READ_BIT_POS,
        ))
        .expect("3 bits fit in u8");
        bits.into()
    }

    pub fn is_leaf(&self) -> bool {
        self.get_xwr_mode() != XWRMode::PointerToNextLevel
    }

    pub fn set_physical_address(&mut self, address: *mut PageTable) {
        let mask: usize = !(Self::PHYSICAL_PAGE_BITS << Self::PHYSICAL_PAGE_BIT_POS);
        self.0 = address.map_addr(|new_address| {
            let mut original = self.0.addr();
            original &= mask;
            original |=
                ((new_address >> 12) & Self::PHYSICAL_PAGE_BITS) << Self::PHYSICAL_PAGE_BIT_POS;
            original
        });
    }

    pub fn set_leaf_address(&mut self, address: PhysAddr) {
        assert!(
            address.is_page_aligned(),
            "Leaf address {} is not page-aligned",
            address
        );
        let mask: usize = !(Self::PHYSICAL_PAGE_BITS << Self::PHYSICAL_PAGE_BIT_POS);
        let address_usize = address.as_usize();
        self.0 = self.0.map_addr(|mut original| {
            original &= mask;
            original |=
                ((address_usize >> 12) & Self::PHYSICAL_PAGE_BITS) << Self::PHYSICAL_PAGE_BIT_POS;
            original
        });
    }

    pub fn get_physical_address(&self) -> PhysAddr {
        let addr = self.0.addr();
        PhysAddr::new(((addr >> Self::PHYSICAL_PAGE_BIT_POS) & Self::PHYSICAL_PAGE_BITS) << 12)
    }

    /// Returns a reference to the page table this non-leaf entry points to.
    pub fn target_page_table(&self) -> &PageTable {
        assert!(!self.is_leaf());
        let addr = self.get_physical_address();
        assert!(addr != PhysAddr::zero());
        let ptr = self.0.map_addr(|_| addr.as_usize());
        // SAFETY: Non-leaf entries with non-zero physical address point to
        // valid, allocated page tables (created via Box::leak).
        unsafe { &*ptr }
    }

    /// Returns a mutable reference to the page table this non-leaf entry points to.
    pub fn target_page_table_mut(&mut self) -> &mut PageTable {
        assert!(!self.is_leaf());
        let addr = self.get_physical_address();
        assert!(addr != PhysAddr::zero());
        let ptr = self.0.map_addr(|_| addr.as_usize());
        // SAFETY: Non-leaf entries with non-zero physical address point to
        // valid, allocated page tables (created via Box::leak). Caller has
        // &mut access to the entry, ensuring exclusive access.
        unsafe { &mut *ptr }
    }

    /// Returns the raw pointer to the target page table (for Drop reclamation).
    pub fn get_target_page_table_raw(&self) -> *mut PageTable {
        assert!(!self.is_leaf());
        let addr = self.get_physical_address();
        assert!(addr != PhysAddr::zero());
        self.0.map_addr(|_| addr.as_usize())
    }
}

#[repr(C, align(4096))]
#[derive(Debug)]
pub struct PageTable(pub [PageTableEntry; 512]);

const _: [(); core::mem::size_of::<super::page::Page>()] = [(); core::mem::size_of::<PageTable>()];

impl PageTable {
    pub fn zero() -> Self {
        Self([PageTableEntry(null_mut()); 512])
    }

    pub fn get_entry_for_virtual_address_mut(
        &mut self,
        virtual_address: usize,
        level: u8,
    ) -> &mut PageTableEntry {
        assert!(level <= 2);
        let shifted_address = virtual_address >> (12 + 9 * level);
        let index = shifted_address & 0x1ff;
        &mut self.0[index]
    }

    pub fn get_entry_for_virtual_address(
        &self,
        virtual_address: usize,
        level: u8,
    ) -> &PageTableEntry {
        assert!(level <= 2);
        let shifted_address = virtual_address >> (12 + 9 * level);
        let index = shifted_address & 0x1ff;
        &self.0[index]
    }

    pub fn get_physical_address(&self) -> PhysAddr {
        PhysAddr::new((self as *const Self).addr())
    }
}

/// Owns a heap-allocated PageTable via Box::leak. Provides safe Deref/DerefMut
/// and reclaims via Box::from_raw on Drop.
pub struct OwnedPageTable {
    ptr: *mut PageTable,
}

// SAFETY: OwnedPageTable owns its page table exclusively.
unsafe impl Send for OwnedPageTable {}

impl OwnedPageTable {
    pub fn new() -> Self {
        Self {
            ptr: Box::leak(Box::new(PageTable::zero())),
        }
    }
}

impl Default for OwnedPageTable {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnedPageTable {
    pub fn as_raw(&self) -> *mut PageTable {
        self.ptr
    }

    /// Reclaim a child page table that was allocated with Box::leak.
    ///
    /// # Safety
    /// `ptr` must have been created by `Box::leak(Box::new(...))` and must
    /// not be used after this call.
    pub unsafe fn reclaim(ptr: *mut PageTable) {
        // SAFETY: Caller guarantees ptr was created by Box::leak.
        unsafe {
            let _ = Box::from_raw(ptr);
        }
    }

    /// Reclaim all second- and third-level child tables. Called before
    /// dropping the root table to free the entire page table tree.
    pub fn reclaim_children(&self) {
        // SAFETY: We own the root table and all children were allocated
        // via Box::leak in map().
        let table = unsafe { &*self.ptr };
        for first_level_entry in table.0.iter() {
            if !first_level_entry.get_validity() || first_level_entry.is_leaf() {
                continue;
            }
            let second_level_table = first_level_entry.target_page_table();
            for second_level_entry in second_level_table.0.iter() {
                if !second_level_entry.get_validity() || second_level_entry.is_leaf() {
                    continue;
                }
                unsafe { Self::reclaim(second_level_entry.get_target_page_table_raw()) };
            }
            unsafe { Self::reclaim(first_level_entry.get_target_page_table_raw()) };
        }
    }
}

impl core::ops::Deref for OwnedPageTable {
    type Target = PageTable;
    fn deref(&self) -> &PageTable {
        // SAFETY: ptr was created via Box::leak and is always valid.
        unsafe { &*self.ptr }
    }
}

impl core::ops::DerefMut for OwnedPageTable {
    fn deref_mut(&mut self) -> &mut PageTable {
        // SAFETY: ptr was created via Box::leak. &mut self guarantees exclusivity.
        unsafe { &mut *self.ptr }
    }
}

impl Drop for OwnedPageTable {
    fn drop(&mut self) {
        // Only reclaim the root table — child tables are managed by
        // RootPageTableHolder::drop in the kernel.
        // SAFETY: ptr was created via Box::leak in new().
        unsafe {
            let _ = Box::from_raw(self.ptr);
        }
        self.ptr = null_mut();
    }
}

impl core::fmt::Display for OwnedPageTable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "OwnedPageTable({:p})", self.ptr)
    }
}

/// Activate a page table by writing satp and fencing.
pub fn activate_page_table(satp_val: usize) {
    // SAFETY: satp_val encodes a valid page table that identity-maps all
    // kernel memory, so execution can continue after the switch.
    unsafe {
        arch::cpu::write_satp_and_fence(satp_val);
    }
}
