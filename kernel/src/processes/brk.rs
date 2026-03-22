use crate::{
    klibc::util::align_up_page_size,
    memory::{
        Pages, PhysAddr, PinnedHeapPages, VirtAddr,
        page_tables::{RootPageTableHolder, XWRMode},
    },
};

const BRK_SIZE: Pages = Pages::new(4);

#[derive(Debug, Clone)]
pub struct Brk {
    brk_start: VirtAddr,
    brk_current: VirtAddr,
    /// One past the end of the allocated area
    brk_end: VirtAddr,
}

impl Brk {
    pub fn new(
        bss_end: VirtAddr,
        page_tables: &mut RootPageTableHolder,
    ) -> (PinnedHeapPages, Self) {
        let brk_start = VirtAddr::new(align_up_page_size(bss_end.as_usize()));
        let pages = PinnedHeapPages::new_pages(BRK_SIZE);
        page_tables.map_userspace(
            brk_start,
            PhysAddr::new(pages.addr()),
            pages.size(),
            XWRMode::ReadWrite,
            "BRK".into(),
        );
        let brk_end = brk_start + BRK_SIZE.as_bytes();
        (
            pages,
            Self {
                brk_start,
                brk_current: brk_start,
                brk_end,
            },
        )
    }

    pub fn empty() -> Self {
        Self {
            brk_start: VirtAddr::zero(),
            brk_current: VirtAddr::zero(),
            brk_end: VirtAddr::new(1),
        }
    }

    pub fn start(&self) -> VirtAddr {
        self.brk_start
    }

    pub fn brk(&mut self, brk: VirtAddr) -> VirtAddr {
        if brk >= self.brk_start && brk < self.brk_end {
            self.brk_current = brk;
        }

        self.brk_current
    }
}

#[cfg(test)]
mod tests {
    use super::Brk;

    #[test_case]
    fn brk_within_range() {
        use super::VirtAddr;
        let mut brk = Brk {
            brk_start: VirtAddr::new(0x1000),
            brk_current: VirtAddr::new(0x1000),
            brk_end: VirtAddr::new(0x5000),
        };
        assert_eq!(brk.brk(VirtAddr::new(0x2000)), VirtAddr::new(0x2000));
        assert_eq!(brk.brk(VirtAddr::new(0x4FFF)), VirtAddr::new(0x4FFF));
    }

    #[test_case]
    fn brk_out_of_range_returns_current() {
        use super::VirtAddr;
        let mut brk = Brk {
            brk_start: VirtAddr::new(0x1000),
            brk_current: VirtAddr::new(0x2000),
            brk_end: VirtAddr::new(0x5000),
        };
        // Below start
        assert_eq!(brk.brk(VirtAddr::new(0x0500)), VirtAddr::new(0x2000));
        // At end (exclusive boundary)
        assert_eq!(brk.brk(VirtAddr::new(0x5000)), VirtAddr::new(0x2000));
        // Above end
        assert_eq!(brk.brk(VirtAddr::new(0x9000)), VirtAddr::new(0x2000));
    }

    #[test_case]
    fn brk_empty() {
        use super::VirtAddr;
        let mut brk = Brk::empty();
        assert_eq!(brk.brk(VirtAddr::zero()), VirtAddr::zero());
        // brk_end is 1, so 0 is within [0, 1)
        assert_eq!(brk.brk(VirtAddr::new(0x1000)), VirtAddr::zero());
    }
}
