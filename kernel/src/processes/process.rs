use crate::{
    debug,
    klibc::Spinlock,
    memory::{
        PAGE_SIZE, Pages, PhysAddr, PinnedHeapPages, VirtAddr,
        page_tables::{RootPageTableHolder, XWRMode},
    },
    processes::{
        brk::Brk, credentials::Credentials, fd_table::FdTable, thread::ThreadWeakRef,
        userspace_ptr::UserspacePtr,
    },
};
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use common::pid::Tid;
use core::{self, fmt::Debug, ptr::null_mut};
use headers::errno::Errno;
use sys::klibc::validated_ptr::ValidatedPtr;

pub const POWERSAVE_TID: Tid = Tid::new(0);

pub struct CowPageInfo {
    pub phys_addr: PhysAddr,
    pub original_perm: XWRMode,
    _backing: Arc<PinnedHeapPages>,
}

pub enum Mapping {
    Allocated(PinnedHeapPages),
    Mmap(PinnedHeapPages),
    Cow(CowPageInfo),
}

pub struct ForkedAddressSpace {
    pub page_table: RootPageTableHolder,
    pub cow_pages: BTreeMap<VirtAddr, CowPageInfo>,
    pub brk: Brk,
    pub free_mmap_address: VirtAddr,
}

const FREE_MMAP_START_ADDRESS: usize = 0x2000000000;

pub type ProcessRef = Arc<Spinlock<Process>>;

pub struct Process {
    name: Arc<String>,
    binary_path: Option<Arc<String>>,
    page_table: RootPageTableHolder,
    mappings: BTreeMap<VirtAddr, Mapping>,
    free_mmap_address: VirtAddr,
    fd_table: Arc<Spinlock<FdTable>>,
    threads: BTreeMap<Tid, ThreadWeakRef>,
    main_tid: Tid,
    pgid: Tid,
    sid: Tid,
    brk: Brk,
    umask: u32,
    cwd: String,
    credentials: Credentials,
}

impl Debug for Process {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Process [
            Page Table: {:?},
            Number of mappings: {},
            Threads: {:?}
        ]",
            self.page_table,
            self.mappings.len(),
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
        let mappings = allocated_pages
            .into_iter()
            .map(|(va, pages)| (va, Mapping::Allocated(pages)))
            .collect();
        Self {
            name,
            binary_path: None,
            page_table,
            mappings,
            free_mmap_address: VirtAddr::new(FREE_MMAP_START_ADDRESS),
            fd_table: Arc::new(Spinlock::new(FdTable::new())),
            threads: BTreeMap::new(),
            brk,
            main_tid: main_thread,
            pgid,
            sid,
            umask: 0o022,
            cwd: String::from("/"),
            credentials: Credentials::root(),
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
        let validated = ValidatedPtr::<T>::from_userspace(ptr.get(), len, self.get_page_table())?;
        Ok(validated.read_slice(len))
    }

    pub fn write_userspace_slice<T: Copy>(
        &mut self,
        ptr: &UserspacePtr<*mut T>,
        data: &[T],
    ) -> Result<(), Errno> {
        self.ensure_cow_resolved_for_write(ptr.get() as usize, core::mem::size_of_val(data));
        let validated =
            ValidatedPtr::<T>::from_userspace(ptr.get(), data.len(), self.get_page_table())?;
        validated.write_slice(data);
        Ok(())
    }

    pub fn read_userspace_ptr<T: Copy>(&self, ptr: &UserspacePtr<*const T>) -> Result<T, Errno> {
        let validated = ValidatedPtr::<T>::from_userspace(ptr.get(), 1, self.get_page_table())?;
        Ok(validated.read())
    }

    pub fn write_userspace_ptr<T>(
        &mut self,
        ptr: &UserspacePtr<*mut T>,
        value: T,
    ) -> Result<(), Errno> {
        self.ensure_cow_resolved_for_write(ptr.get() as usize, core::mem::size_of::<T>());
        let validated = ValidatedPtr::<T>::from_userspace(ptr.get(), 1, self.get_page_table())?;
        validated.write(value);
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
        self.mappings.insert(addr, Mapping::Mmap(pages));
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
        self.mappings.insert(addr, Mapping::Mmap(pages));
        self.free_mmap_address += length;
        core::ptr::without_provenance_mut(addr.as_usize())
    }

    pub fn munmap_pages(&mut self, addr: VirtAddr, length: usize) -> Result<(), Errno> {
        let entry = self.mappings.remove(&addr).ok_or(Errno::EINVAL)?;
        let Mapping::Mmap(pages) = entry else {
            self.mappings.insert(addr, entry);
            return Err(Errno::EINVAL);
        };
        if pages.size() != length {
            self.mappings.insert(addr, Mapping::Mmap(pages));
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

    pub fn binary_path(&self) -> Option<&Arc<String>> {
        self.binary_path.as_ref()
    }

    pub fn set_binary_path(&mut self, path: Arc<String>) {
        self.binary_path = Some(path);
    }

    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    pub fn credentials_mut(&mut self) -> &mut Credentials {
        &mut self.credentials
    }

    pub fn set_credentials(&mut self, creds: Credentials) {
        self.credentials = creds;
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

    pub fn fork_address_space(&mut self) -> ForkedAddressSpace {
        use super::signal;

        let mut child_pt = RootPageTableHolder::new_with_kernel_mapping(&[]);

        child_pt.map_userspace(
            signal::TRAMPOLINE_VADDR,
            signal::trampoline_phys_addr(),
            PAGE_SIZE,
            XWRMode::ReadExecute,
            "Signal trampoline".into(),
        );

        let mut parent_cow = BTreeMap::new();
        let mut child_cow = BTreeMap::new();

        for (va, mapping) in core::mem::take(&mut self.mappings) {
            match mapping {
                Mapping::Allocated(pages) | Mapping::Mmap(pages) => {
                    let backing = Arc::new(pages);
                    for i in 0..backing.len() {
                        let page_va = va + i * PAGE_SIZE;
                        let Some(perm) = self.page_table.get_userspace_permissions(page_va) else {
                            continue;
                        };
                        let phys_addr = PhysAddr::new(backing.addr() + i * PAGE_SIZE);
                        let child_perm = if perm.is_writable() {
                            self.page_table
                                .remap_page(page_va, phys_addr, perm.as_readonly());
                            perm.as_readonly()
                        } else {
                            perm
                        };
                        child_pt.map_userspace(
                            page_va,
                            phys_addr,
                            PAGE_SIZE,
                            child_perm,
                            "fork-cow".into(),
                        );
                        let cow_info = CowPageInfo {
                            phys_addr,
                            original_perm: perm,
                            _backing: Arc::clone(&backing),
                        };
                        child_cow.insert(
                            page_va,
                            CowPageInfo {
                                phys_addr,
                                original_perm: perm,
                                _backing: Arc::clone(&backing),
                            },
                        );
                        parent_cow.insert(page_va, cow_info);
                    }
                }
                Mapping::Cow(cow_info) => {
                    let current_perm = self
                        .page_table
                        .get_userspace_permissions(va)
                        .expect("cow_pages entry must be mapped");
                    child_pt.map_userspace(
                        va,
                        cow_info.phys_addr,
                        PAGE_SIZE,
                        current_perm,
                        "fork-cow".into(),
                    );
                    child_cow.insert(
                        va,
                        CowPageInfo {
                            phys_addr: cow_info.phys_addr,
                            original_perm: cow_info.original_perm,
                            _backing: Arc::clone(&cow_info._backing),
                        },
                    );
                    parent_cow.insert(va, cow_info);
                }
            }
        }

        self.mappings = parent_cow
            .into_iter()
            .map(|(va, info)| (va, Mapping::Cow(info)))
            .collect();

        ForkedAddressSpace {
            page_table: child_pt,
            cow_pages: child_cow,
            brk: self.brk.clone(),
            free_mmap_address: self.free_mmap_address,
        }
    }

    pub fn set_fork_state(
        &mut self,
        cow_pages: BTreeMap<VirtAddr, CowPageInfo>,
        free_mmap_address: VirtAddr,
    ) {
        self.mappings = cow_pages
            .into_iter()
            .map(|(va, info)| (va, Mapping::Cow(info)))
            .collect();
        self.free_mmap_address = free_mmap_address;
    }

    pub fn resolve_cow_page(&mut self, faulting_va: VirtAddr) -> bool {
        use crate::memory::page_slice_at_phys;

        let page_va = VirtAddr::new(faulting_va.as_usize() & !(PAGE_SIZE - 1));
        let Some(mapping) = self.mappings.remove(&page_va) else {
            return false;
        };
        let Mapping::Cow(cow_info) = mapping else {
            self.mappings.insert(page_va, mapping);
            return false;
        };

        let mut new_page = PinnedHeapPages::new(1);
        let src = page_slice_at_phys(cow_info.phys_addr);
        new_page[0].copy_from_slice(src);
        let new_phys = PhysAddr::new(new_page.addr());
        self.page_table
            .remap_page(page_va, new_phys, cow_info.original_perm);
        self.mappings.insert(page_va, Mapping::Allocated(new_page));
        true
    }

    fn ensure_cow_resolved_for_write(&mut self, addr: usize, len: usize) {
        if len == 0 {
            return;
        }
        let start_page = addr & !(PAGE_SIZE - 1);
        let end_page = (addr + len - 1) & !(PAGE_SIZE - 1);
        let num_pages = (end_page - start_page) / PAGE_SIZE + 1;
        for i in 0..num_pages {
            let va = VirtAddr::new(start_page + i * PAGE_SIZE);
            let is_writable_cow = matches!(
                self.mappings.get(&va),
                Some(Mapping::Cow(c)) if c.original_perm.is_writable()
            );
            if is_writable_cow {
                self.resolve_cow_page(va);
            }
        }
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
            "Drop process (MAIN_TID: {}) (Mappings: {})",
            self.main_tid,
            self.mappings.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use common::{pid::Tid, syscalls::trap_frame::Register};

    use crate::{
        autogenerated::userspace_programs::PROG1,
        klibc::{consumable_buffer::ConsumableBuffer, elf::ElfFile},
        memory::{PAGE_SIZE, Pages, page_tables::XWRMode},
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
