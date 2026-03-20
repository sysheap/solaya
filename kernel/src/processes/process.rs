#![allow(unsafe_code)]
use crate::{
    debug,
    klibc::Spinlock,
    memory::{
        PAGE_SIZE, PhysAddr, VirtAddr,
        page::{Pages, PinnedHeapPages},
        page_tables::{RootPageTableHolder, XWRMode},
    },
    processes::{brk::Brk, fd_table::FdTable, thread::ThreadWeakRef, userspace_ptr::UserspacePtr},
};
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use common::{pid::Tid, pointer::Pointer};
use core::{self, fmt::Debug, ptr::null_mut};
use headers::errno::Errno;

pub const POWERSAVE_TID: Tid = Tid::new(0);

pub struct ForkedAddressSpace {
    pub page_table: RootPageTableHolder,
    pub allocated_pages: BTreeMap<VirtAddr, PinnedHeapPages>,
    pub mmap_allocations: BTreeMap<VirtAddr, PinnedHeapPages>,
    pub brk: Brk,
    pub free_mmap_address: VirtAddr,
}

const FREE_MMAP_START_ADDRESS: usize = 0x2000000000;

pub type ProcessRef = Arc<Spinlock<Process>>;

pub struct Process {
    name: Arc<String>,
    page_table: RootPageTableHolder,
    allocated_pages: BTreeMap<VirtAddr, PinnedHeapPages>,
    mmap_allocations: BTreeMap<VirtAddr, PinnedHeapPages>,
    free_mmap_address: VirtAddr,
    fd_table: Arc<Spinlock<FdTable>>,
    threads: BTreeMap<Tid, ThreadWeakRef>,
    main_tid: Tid,
    pgid: Tid,
    sid: Tid,
    brk: Brk,
    umask: u32,
    cwd: String,
}

impl Debug for Process {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Process [
            Page Table: {:?},
            Number of allocated page groups: {},
            Threads: {:?}
        ]",
            self.page_table,
            self.allocated_pages.len(),
            self.threads,
        )
    }
}

impl Process {
    pub fn new(
        name: Arc<String>,
        page_table: RootPageTableHolder,
        allocated_pages: BTreeMap<VirtAddr, PinnedHeapPages>,
        brk: Brk,
        main_thread: Tid,
        pgid: Tid,
        sid: Tid,
    ) -> Self {
        Self {
            name,
            page_table,
            allocated_pages,
            mmap_allocations: BTreeMap::new(),
            free_mmap_address: VirtAddr::new(FREE_MMAP_START_ADDRESS),
            fd_table: Arc::new(Spinlock::new(FdTable::new())),
            threads: BTreeMap::new(),
            brk,
            main_tid: main_thread,
            pgid,
            sid,
            umask: 0o022,
            cwd: String::from("/"),
        }
    }

    pub fn brk(&mut self, brk: VirtAddr) -> VirtAddr {
        self.brk.brk(brk)
    }

    pub fn add_thread(&mut self, tid: Tid, thread: ThreadWeakRef) {
        assert!(
            self.threads.insert(tid, thread).is_none(),
            "Duplicate TID {tid} in process"
        );
    }

    pub fn read_userspace_slice<T: Clone>(
        &self,
        ptr: &UserspacePtr<*const T>,
        len: usize,
    ) -> Result<Vec<T>, Errno> {
        let kernel_ptr = self.get_kernel_space_fat_pointer(ptr, len)?;
        // SAFETY: We just validate the pointer
        let slice = unsafe { core::slice::from_raw_parts(kernel_ptr, len) };
        Ok(slice.to_vec())
    }

    pub fn write_userspace_slice<T: Copy>(
        &self,
        ptr: &UserspacePtr<*mut T>,
        data: &[T],
    ) -> Result<(), Errno> {
        let len = data.len();
        let kernel_ptr = self.get_kernel_space_fat_pointer(ptr, len)?;
        // SAFETY: We just validate the pointer
        let slice = unsafe { core::slice::from_raw_parts_mut(kernel_ptr, len) };
        slice.copy_from_slice(data);
        Ok(())
    }

    fn get_kernel_space_pointer<PTR: Pointer>(
        &self,
        ptr: &UserspacePtr<PTR>,
    ) -> Result<PTR, Errno> {
        let pt = self.get_page_table();
        // SAFETY: We know it is a userspace pointer and we gonna translate it later
        let ptr = unsafe { ptr.get() };
        if !pt.is_valid_userspace_ptr(ptr, PTR::WRITABLE) {
            return Err(Errno::EFAULT);
        }
        pt.translate_userspace_address_to_physical_address(ptr)
            .ok_or(Errno::EFAULT)
    }

    fn get_kernel_space_fat_pointer<PTR: Pointer>(
        &self,
        ptr: &UserspacePtr<PTR>,
        len: usize,
    ) -> Result<PTR, Errno> {
        let pt = self.get_page_table();
        // SAFETY: We know it is a userspace pointer and we gonna translate it later
        let ptr = unsafe { ptr.get() };
        if !pt.is_valid_userspace_fat_ptr(ptr, len, PTR::WRITABLE) {
            return Err(Errno::EFAULT);
        }
        pt.translate_userspace_address_to_physical_address(ptr)
            .ok_or(Errno::EFAULT)
    }

    pub fn read_userspace_ptr<T>(&self, ptr: &UserspacePtr<*const T>) -> Result<T, Errno> {
        let kernel_ptr = self.get_kernel_space_pointer(ptr)?;
        // SAFETY: We just validate the pointer
        unsafe { Ok(kernel_ptr.read()) }
    }

    pub fn write_userspace_ptr<T>(
        &self,
        ptr: &UserspacePtr<*mut T>,
        value: T,
    ) -> Result<(), Errno> {
        let kernel_ptr = self.get_kernel_space_pointer(ptr)?;
        // SAFETY: We just validate the pointer
        unsafe {
            kernel_ptr.write(value);
        }
        Ok(())
    }

    pub fn mmap_pages_with_address(
        &mut self,
        num_pages: Pages,
        addr: VirtAddr,
        permission: XWRMode,
    ) -> *mut u8 {
        let length = num_pages.as_bytes();
        if self.page_table.is_mapped(addr..addr + length) {
            return null_mut();
        }
        let pages = PinnedHeapPages::new_pages(num_pages);
        self.page_table.map_userspace(
            addr,
            PhysAddr::new(pages.addr()),
            length,
            permission,
            "mmap".into(),
        );
        self.mmap_allocations.insert(addr, pages);
        core::ptr::without_provenance_mut(addr.as_usize())
    }

    pub fn mmap_pages(&mut self, num_pages: Pages, permission: XWRMode) -> *mut u8 {
        let length = num_pages.as_bytes();
        let pages = PinnedHeapPages::new_pages(num_pages);
        let addr = self.free_mmap_address;
        self.page_table.map_userspace(
            addr,
            PhysAddr::new(pages.as_ptr() as usize),
            length,
            permission,
            "mmap".to_string(),
        );
        self.mmap_allocations.insert(addr, pages);
        self.free_mmap_address += length;
        core::ptr::without_provenance_mut(addr.as_usize())
    }

    pub fn munmap_pages(&mut self, addr: VirtAddr, length: usize) -> Result<(), Errno> {
        let pages = self.mmap_allocations.remove(&addr).ok_or(Errno::EINVAL)?;
        if pages.size() != length {
            self.mmap_allocations.insert(addr, pages);
            return Err(Errno::EINVAL);
        }
        self.page_table.unmap_userspace(addr, length);
        Ok(())
    }

    pub fn get_page_table(&self) -> &RootPageTableHolder {
        &self.page_table
    }

    pub fn get_page_table_mut(&mut self) -> &mut RootPageTableHolder {
        &mut self.page_table
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn main_tid(&self) -> Tid {
        self.main_tid
    }

    pub fn pgid(&self) -> Tid {
        self.pgid
    }

    pub fn sid(&self) -> Tid {
        self.sid
    }

    pub fn set_pgid(&mut self, pgid: Tid) {
        self.pgid = pgid;
    }

    pub fn set_sid(&mut self, sid: Tid) {
        self.sid = sid;
    }

    pub fn umask(&self) -> u32 {
        self.umask
    }

    pub fn set_umask(&mut self, mask: u32) {
        self.umask = mask;
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn set_cwd(&mut self, cwd: String) {
        self.cwd = cwd;
    }

    pub fn fd_table(&self) -> crate::klibc::SpinlockGuard<'_, FdTable> {
        self.fd_table.lock()
    }

    pub fn set_fd_table(&mut self, fd_table: FdTable) {
        self.fd_table = Arc::new(Spinlock::new(fd_table));
    }

    pub fn get_satp_value(&self) -> usize {
        self.page_table.get_satp_value_from_page_tables()
    }

    pub fn remove_thread(&mut self, tid: Tid) {
        self.threads.remove(&tid);
    }

    pub fn has_no_threads(&self) -> bool {
        self.threads.is_empty()
    }

    pub fn close_all_fds(&self) {
        self.fd_table.lock().close_all();
    }

    pub fn thread_tids(&self) -> Vec<Tid> {
        self.threads.keys().copied().collect()
    }

    pub fn fork_address_space(&self) -> ForkedAddressSpace {
        use super::signal;

        let mut child_pt = RootPageTableHolder::new_with_kernel_mapping(&[]);

        child_pt.map_userspace(
            signal::TRAMPOLINE_VADDR,
            signal::trampoline_phys_addr(),
            PAGE_SIZE,
            XWRMode::ReadExecute,
            "Signal trampoline".into(),
        );

        let copy_pages = |pages_map: &BTreeMap<VirtAddr, PinnedHeapPages>,
                          pt: &mut RootPageTableHolder,
                          parent_pt: &RootPageTableHolder|
         -> BTreeMap<VirtAddr, PinnedHeapPages> {
            let mut child_map = BTreeMap::new();
            for (&va, parent_pages) in pages_map {
                let mut child_pages = PinnedHeapPages::new(parent_pages.len());
                for (dst, src) in child_pages.iter_mut().zip(parent_pages.iter()) {
                    let dst_slice: &mut [u8] = &mut **dst;
                    let src_slice: &[u8] = &**src;
                    dst_slice.copy_from_slice(src_slice);
                }
                for i in 0..parent_pages.len() {
                    let page_va = va + i * PAGE_SIZE;
                    let Some(perm) = parent_pt.get_userspace_permissions(page_va) else {
                        continue;
                    };
                    pt.map_userspace(
                        page_va,
                        PhysAddr::new(child_pages.addr() + i * PAGE_SIZE),
                        PAGE_SIZE,
                        perm,
                        "fork".into(),
                    );
                }
                child_map.insert(va, child_pages);
            }
            child_map
        };

        let allocated_pages = copy_pages(&self.allocated_pages, &mut child_pt, &self.page_table);
        let mmap_allocations = copy_pages(&self.mmap_allocations, &mut child_pt, &self.page_table);

        ForkedAddressSpace {
            page_table: child_pt,
            allocated_pages,
            mmap_allocations,
            brk: self.brk.clone(),
            free_mmap_address: self.free_mmap_address,
        }
    }

    pub fn set_mmap_state(
        &mut self,
        mmap_allocations: BTreeMap<VirtAddr, PinnedHeapPages>,
        free_mmap_address: VirtAddr,
    ) {
        self.mmap_allocations = mmap_allocations;
        self.free_mmap_address = free_mmap_address;
    }
}

impl core::fmt::Display for Process {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "main_tid={} name={}", self.main_tid, self.name)?;
        for thread in self.threads.values().filter_map(ThreadWeakRef::upgrade) {
            writeln!(f, "\t{}", *thread.lock())?;
        }
        Ok(())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        debug!(
            "Drop process (MAIN_TID: {}) (Allocated pages: {:?})",
            self.main_tid, self.allocated_pages
        );
    }
}

#[cfg(test)]
mod tests {
    use common::{pid::Tid, syscalls::trap_frame::Register};

    use crate::{
        autogenerated::userspace_programs::PROG1,
        klibc::{consumable_buffer::ConsumableBuffer, elf::ElfFile},
        memory::{PAGE_SIZE, page::Pages, page_tables::XWRMode},
        processes::{
            loader::{STACK_END, STACK_START},
            process::FREE_MMAP_START_ADDRESS,
            thread::Thread,
        },
    };
    use alloc::sync::Arc;

    use super::Process;

    #[test_case]
    fn create_process_from_elf() {
        let elf = ElfFile::parse(PROG1).expect("Cannot parse elf file");
        let _process = Thread::from_elf(&elf, "prog1", &[], &[], Tid::new(0))
            .expect("ELF loading must succeed");
    }

    #[test_case]
    fn mmap_process() {
        let elf = ElfFile::parse(PROG1).expect("Cannot parse elf file");

        let process_ref = Thread::from_elf(&elf, "prog1", &[], &[], Tid::new(0))
            .expect("ELF loading must succeed");

        let thread = Arc::into_inner(process_ref)
            .expect("Must be sole owner")
            .into_inner();
        let process = thread.process();
        let mut process = process.lock();

        use crate::memory::VirtAddr;
        assert!(
            process.free_mmap_address == VirtAddr::new(FREE_MMAP_START_ADDRESS),
            "Free MMAP Address must set to correct start"
        );
        let ptr = process.mmap_pages(Pages::new(1), XWRMode::ReadWrite);
        assert!(
            ptr as usize == FREE_MMAP_START_ADDRESS,
            "Returned pointer must have the value of the initial free mmap start address."
        );
        assert!(
            process.free_mmap_address == VirtAddr::new(FREE_MMAP_START_ADDRESS + PAGE_SIZE),
            "Free mmap address must have the value of the next free value"
        );
        let ptr = process.mmap_pages(Pages::new(2), XWRMode::ReadWrite);
        assert!(
            ptr as usize == FREE_MMAP_START_ADDRESS + PAGE_SIZE,
            "Returned pointer must have the value of the initial free mmap start address."
        );
        assert!(
            process.free_mmap_address == VirtAddr::new(FREE_MMAP_START_ADDRESS + (3 * PAGE_SIZE)),
            "Free mmap address must have the value of the next free value"
        );
    }

    #[test_case]
    fn munmap_process() {
        let elf = ElfFile::parse(PROG1).expect("Cannot parse elf file");
        let process_ref = Thread::from_elf(&elf, "prog1", &[], &[], Tid::new(0))
            .expect("ELF loading must succeed");
        let thread = Arc::into_inner(process_ref)
            .expect("Must be sole owner")
            .into_inner();
        let process = thread.process();
        let mut process = process.lock();

        use crate::memory::VirtAddr;
        let ptr = process.mmap_pages(Pages::new(1), XWRMode::ReadWrite);
        let addr = VirtAddr::new(ptr as usize);
        assert!(process.get_page_table().is_mapped(addr..addr + PAGE_SIZE));

        process
            .munmap_pages(addr, PAGE_SIZE)
            .expect("munmap must succeed");
        assert!(!process.get_page_table().is_mapped(addr..addr + PAGE_SIZE));
    }

    #[test_case]
    fn munmap_unknown_address_returns_einval() {
        let elf = ElfFile::parse(PROG1).expect("Cannot parse elf file");
        let process_ref = Thread::from_elf(&elf, "prog1", &[], &[], Tid::new(0))
            .expect("ELF loading must succeed");
        let thread = Arc::into_inner(process_ref)
            .expect("Must be sole owner")
            .into_inner();
        let process = thread.process();
        let mut process = process.lock();

        use crate::memory::VirtAddr;
        let result = process.munmap_pages(VirtAddr::new(0xDEAD_0000), PAGE_SIZE);
        assert_eq!(result, Err(headers::errno::Errno::EINVAL));
    }

    #[test_case]
    fn munmap_wrong_length_returns_einval() {
        let elf = ElfFile::parse(PROG1).expect("Cannot parse elf file");
        let process_ref = Thread::from_elf(&elf, "prog1", &[], &[], Tid::new(0))
            .expect("ELF loading must succeed");
        let thread = Arc::into_inner(process_ref)
            .expect("Must be sole owner")
            .into_inner();
        let process = thread.process();
        let mut process = process.lock();

        use crate::memory::VirtAddr;
        let ptr = process.mmap_pages(Pages::new(1), XWRMode::ReadWrite);
        let addr = VirtAddr::new(ptr as usize);
        let result = process.munmap_pages(addr, PAGE_SIZE * 2);
        assert_eq!(result, Err(headers::errno::Errno::EINVAL));
        assert!(
            process.get_page_table().is_mapped(addr..addr + PAGE_SIZE),
            "mapping must still exist after failed munmap"
        );
    }
}
