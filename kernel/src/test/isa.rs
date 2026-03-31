#[cfg(test)]
mod tests {
    use arch::isa::IsaExtensions;

    #[test_case]
    fn visionfive2_s7_no_supervisor() {
        let isa = IsaExtensions::parse("rv64imacu_zba_zbb").expect("parse failed");
        assert!(!isa.has_supervisor());
        assert!(isa.has_extension('i'));
        assert!(isa.has_extension('m'));
        assert!(isa.has_extension('a'));
        assert!(isa.has_extension('c'));
        assert!(isa.has_extension('u'));
        assert!(!isa.has_extension('f'));
        assert!(!isa.has_extension('d'));
    }

    #[test_case]
    fn visionfive2_u74_has_supervisor() {
        let isa = IsaExtensions::parse("rv64imafdcbsux_zba_zbb").expect("parse failed");
        assert!(isa.has_supervisor());
        assert!(isa.has_extension('f'));
        assert!(isa.has_extension('d'));
        assert!(isa.has_extension('b'));
    }

    #[test_case]
    fn qemu_full_isa() {
        let isa = IsaExtensions::parse(
            "rv64imafdch_zicbom_zicboz_zicntr_zicsr_zifencei_zihintntl_zihintpause_zihpm_zawrs_zfa_zca_zcd_zba_zbb_zbc_zbs_sstc_svadu",
        ).expect("parse failed");
        assert!(isa.has_extension('h'));
        assert!(isa.has_extension('f'));
        assert!(isa.has_extension('d'));
        assert!(isa.has_extension('c'));
    }

    #[test_case]
    fn rv32_base() {
        let isa = IsaExtensions::parse("rv32imac").expect("parse failed");
        assert!(isa.has_extension('c'));
        assert!(!isa.has_extension('f'));
    }

    #[test_case]
    fn version_numbers_skipped() {
        let isa = IsaExtensions::parse("rv64i2p1m2a2f2d2c").expect("parse failed");
        assert!(isa.has_extension('i'));
        assert!(isa.has_extension('f'));
        assert!(isa.has_extension('d'));
        assert!(isa.has_extension('c'));
        // 'p' should NOT be recorded — it's a version separator, not packed-SIMD
        assert!(!isa.has_extension('p'));
    }

    #[test_case]
    fn invalid_prefix() {
        assert!(IsaExtensions::parse("rv128i").is_none());
        assert!(IsaExtensions::parse("arm64").is_none());
        assert!(IsaExtensions::parse("").is_none());
    }

    #[test_case]
    fn missing_extension() {
        let isa = IsaExtensions::parse("rv64imac").expect("parse failed");
        assert!(!isa.has_extension('s'));
        assert!(!isa.has_extension('f'));
        assert!(!isa.has_extension('d'));
    }

    #[test_case]
    fn has_extension_non_letter() {
        let isa = IsaExtensions::parse("rv64imac").expect("parse failed");
        assert!(!isa.has_extension('1'));
        assert!(!isa.has_extension('A'));
    }
}
