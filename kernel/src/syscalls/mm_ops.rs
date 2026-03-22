use core::ffi::c_uint;
use headers::{
    errno::Errno,
    syscall_types::{MAP_ANONYMOUS, MAP_FIXED, MAP_PRIVATE},
};

use crate::memory::{PAGE_SIZE, Pages, VirtAddr, page_tables::XWRMode};

use super::linux::LinuxSyscallHandler;

impl LinuxSyscallHandler {
    pub(super) fn do_mmap(
        &self,
        addr: usize,
        length: usize,
        prot: c_uint,
        flags: c_uint,
        fd: i32,
        offset: isize,
    ) -> Result<isize, Errno> {
        assert_eq!(
            flags & !(MAP_ANONYMOUS | MAP_PRIVATE | MAP_FIXED),
            0,
            "Only this flags are implemented so far."
        );
        assert_eq!(
            flags & (MAP_ANONYMOUS | MAP_PRIVATE),
            MAP_ANONYMOUS | MAP_PRIVATE,
            "File backed mappings and shared mappings are not supported yet."
        );
        assert_eq!(fd, -1, "fd must be -1 when working in MAP_ANONYMOUS");
        assert_eq!(
            offset, 0,
            "offset must be null when working with MAP_ANONYMOUS"
        );
        if length == 0 {
            return Err(Errno::EINVAL);
        }
        let length = length.next_multiple_of(PAGE_SIZE);
        if (flags & MAP_FIXED) > 0 && addr == 0 {
            return Err(Errno::EINVAL);
        }
        let permission = XWRMode::from_prot(prot)?;
        self.current_process.with_lock(|mut p| {
            if (flags & MAP_FIXED) > 0 {
                // Only handles exact overlap (full range already mapped). Partial overlap
                // is not handled -- the real MAP_FIXED semantic would unmap existing pages
                // first. Sufficient for musl, which only re-maps exact prior allocations.
                if p.get_page_table()
                    .is_mapped(VirtAddr::new(addr)..VirtAddr::new(addr + length))
                {
                    p.get_page_table_mut()
                        .mprotect(VirtAddr::new(addr), length, permission);
                    return Ok(addr as isize);
                }
                let ptr = p.mmap_pages_with_address(
                    Pages::new(length / PAGE_SIZE),
                    VirtAddr::new(addr),
                    permission,
                );
                return Ok(ptr as isize);
            }
            if addr == 0
                || p.get_page_table()
                    .is_mapped(VirtAddr::new(addr)..VirtAddr::new(addr + length))
            {
                return Ok(p.mmap_pages(Pages::new(length / PAGE_SIZE), permission) as isize);
            }
            Ok(p.mmap_pages_with_address(
                Pages::new(length / PAGE_SIZE),
                VirtAddr::new(addr),
                permission,
            ) as isize)
        })
    }

    pub(super) fn do_mprotect(&self, addr: usize, len: usize, prot: i32) -> Result<isize, Errno> {
        if !addr.is_multiple_of(PAGE_SIZE) || len == 0 {
            return Err(Errno::EINVAL);
        }
        let prot = c_uint::try_from(prot).map_err(|_| Errno::EINVAL)?;
        let size = len.next_multiple_of(PAGE_SIZE);
        let mode = XWRMode::from_prot(prot)?;
        self.current_process.with_lock(|mut p| {
            p.get_page_table_mut()
                .mprotect(VirtAddr::new(addr), size, mode);
        });
        Ok(0)
    }
}
