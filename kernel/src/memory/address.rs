pub use sys::memory::address::{PhysAddr, VirtAddr};

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    fn phys_addr_page_aligned_iff_low_bits_zero() {
        let val: usize = kani::any();
        let addr = PhysAddr::new(val);
        assert!(addr.is_page_aligned() == (val & 0xFFF == 0));
    }

    #[kani::proof]
    fn virt_addr_page_aligned_iff_low_bits_zero() {
        let val: usize = kani::any();
        let addr = VirtAddr::new(val);
        assert!(addr.is_page_aligned() == (val & 0xFFF == 0));
    }

    #[kani::proof]
    fn phys_addr_add_sub_inverse() {
        let base: usize = kani::any();
        let n: usize = kani::any();
        kani::assume(base <= usize::MAX - n);
        let addr = PhysAddr::new(base);
        let result = (addr + n) - n;
        assert!(result == addr);
    }

    #[kani::proof]
    fn phys_addr_sub_gives_distance() {
        let a: usize = kani::any();
        let b: usize = kani::any();
        kani::assume(a >= b);
        let addr_a = PhysAddr::new(a);
        let addr_b = PhysAddr::new(b);
        assert!(addr_a - addr_b == a - b);
    }

    #[kani::proof]
    fn virt_addr_roundtrip() {
        let val: usize = kani::any();
        assert!(VirtAddr::new(val).as_usize() == val);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phys_from_page_number(ppn: usize) -> PhysAddr {
        PhysAddr::new(ppn << 12)
    }

    fn phys_page_number(addr: PhysAddr) -> usize {
        addr.as_usize() >> 12
    }

    fn phys_align_down(addr: PhysAddr) -> PhysAddr {
        PhysAddr::new(addr.as_usize() & !0xFFF)
    }

    fn phys_align_up(addr: PhysAddr) -> PhysAddr {
        PhysAddr::new((addr.as_usize() + 0xFFF) & !0xFFF)
    }

    fn virt_from_page_number(vpn: usize) -> VirtAddr {
        VirtAddr::new(vpn << 12)
    }

    fn virt_page_number(addr: VirtAddr) -> usize {
        addr.as_usize() >> 12
    }

    fn virt_align_down(addr: VirtAddr) -> VirtAddr {
        VirtAddr::new(addr.as_usize() & !0xFFF)
    }

    fn virt_align_up(addr: VirtAddr) -> VirtAddr {
        VirtAddr::new((addr.as_usize() + 0xFFF) & !0xFFF)
    }

    fn virt_vpn_level(addr: VirtAddr, level: u8) -> usize {
        assert!(level < 3);
        (addr.as_usize() >> (12 + level as usize * 9)) & 0x1FF
    }

    fn virt_page_offset(addr: VirtAddr) -> usize {
        addr.as_usize() & 0xFFF
    }

    #[test_case]
    fn test_phys_addr_basic() {
        let addr = PhysAddr::new(0x8000_0000);
        assert_eq!(addr.as_usize(), 0x8000_0000);
        assert_eq!(PhysAddr::zero().as_usize(), 0);
    }

    #[test_case]
    fn test_virt_addr_basic() {
        let addr = VirtAddr::new(0x1000);
        assert_eq!(addr.as_usize(), 0x1000);
        assert_eq!(VirtAddr::zero().as_usize(), 0);
    }

    #[test_case]
    fn test_page_number_conversion() {
        let addr = phys_from_page_number(0x8000);
        assert_eq!(addr.as_usize(), 0x8000 << 12);
        assert_eq!(phys_page_number(addr), 0x8000);

        let vaddr = virt_from_page_number(0x1000);
        assert_eq!(vaddr.as_usize(), 0x1000 << 12);
        assert_eq!(virt_page_number(vaddr), 0x1000);
    }

    #[test_case]
    fn test_alignment() {
        let addr = PhysAddr::new(0x8000_1234);
        assert!(!addr.is_page_aligned());
        assert_eq!(phys_align_down(addr).as_usize(), 0x8000_1000);
        assert_eq!(phys_align_up(addr).as_usize(), 0x8000_2000);

        let aligned = PhysAddr::new(0x8000_0000);
        assert!(aligned.is_page_aligned());
        assert_eq!(phys_align_down(aligned).as_usize(), 0x8000_0000);
        assert_eq!(phys_align_up(aligned).as_usize(), 0x8000_0000);
    }

    #[test_case]
    fn test_arithmetic() {
        let addr = PhysAddr::new(0x8000_0000);
        assert_eq!((addr + 0x1000).as_usize(), 0x8000_1000);
        assert_eq!((addr - 0x1000).as_usize(), 0x7FFF_F000);

        let other = PhysAddr::new(0x8000_2000);
        assert_eq!(other - addr, 0x2000);
    }

    #[test_case]
    fn test_vpn_level() {
        let addr = VirtAddr::new(0x0000_007F_FFFF_FFFF);
        assert_eq!(virt_vpn_level(addr, 2), 0x1FF);
        assert_eq!(virt_vpn_level(addr, 1), 0x1FF);
        assert_eq!(virt_vpn_level(addr, 0), 0x1FF);

        let addr2 = VirtAddr::new(0x0000_0000_0040_1000);
        assert_eq!(virt_vpn_level(addr2, 2), 0);
        assert_eq!(virt_vpn_level(addr2, 1), 2);
        assert_eq!(virt_vpn_level(addr2, 0), 1);
    }

    #[test_case]
    fn test_page_offset() {
        let addr = VirtAddr::new(0x1234);
        assert_eq!(virt_page_offset(addr), 0x234);

        let aligned = VirtAddr::new(0x1000);
        assert_eq!(virt_page_offset(aligned), 0);
    }

    #[test_case]
    fn test_ordering() {
        let a1 = PhysAddr::new(0x1000);
        let a2 = PhysAddr::new(0x2000);
        let a3 = PhysAddr::new(0x1000);
        assert!(a1 < a2);
        assert_eq!(a1, a3);

        let v1 = VirtAddr::new(0x1000);
        let v2 = VirtAddr::new(0x2000);
        let v3 = VirtAddr::new(0x1000);
        assert!(v1 < v2);
        assert_eq!(v1, v3);
    }
}
