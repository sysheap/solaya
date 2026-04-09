/// A validated single-letter RISC-V ISA extension (one of `'a'..='z'`).
///
/// Making the element type non-constructible from arbitrary characters means
/// [`IsaExtensions::contains`] cannot be called with an invalid letter — the
/// compiler rejects it instead of returning `false` at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extension(u8);

impl Extension {
    pub const A: Self = Self(b'a');
    pub const B: Self = Self(b'b');
    pub const C: Self = Self(b'c');
    pub const D: Self = Self(b'd');
    pub const E: Self = Self(b'e');
    pub const F: Self = Self(b'f');
    pub const G: Self = Self(b'g');
    pub const H: Self = Self(b'h');
    pub const I: Self = Self(b'i');
    pub const J: Self = Self(b'j');
    pub const K: Self = Self(b'k');
    pub const L: Self = Self(b'l');
    pub const M: Self = Self(b'm');
    pub const N: Self = Self(b'n');
    pub const O: Self = Self(b'o');
    pub const P: Self = Self(b'p');
    pub const Q: Self = Self(b'q');
    pub const R: Self = Self(b'r');
    pub const S: Self = Self(b's');
    pub const T: Self = Self(b't');
    pub const U: Self = Self(b'u');
    pub const V: Self = Self(b'v');
    pub const W: Self = Self(b'w');
    pub const X: Self = Self(b'x');
    pub const Y: Self = Self(b'y');
    pub const Z: Self = Self(b'z');

    /// Return `Some` if `letter` is ASCII lowercase `'a'..='z'`.
    pub const fn from_letter(letter: u8) -> Option<Self> {
        if letter.is_ascii_lowercase() {
            Some(Self(letter))
        } else {
            None
        }
    }

    const fn bit(self) -> u32 {
        1 << (self.0 - b'a')
    }
}

/// Set of single-letter extensions parsed from a RISC-V ISA string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IsaExtensions(u32);

impl IsaExtensions {
    /// Parse a `riscv,isa` device tree string like `"rv64imafdcbsux_zba_zbb"`.
    /// Returns `None` if the prefix is not `rv32` or `rv64`.
    pub fn parse(isa_str: &str) -> Option<Self> {
        let remainder = isa_str
            .strip_prefix("rv64")
            .or_else(|| isa_str.strip_prefix("rv32"))?;

        let bytes = remainder.as_bytes();
        let mut set = Self::default();
        let mut i = 0;

        while i < bytes.len() {
            match bytes[i] {
                b'a'..=b'z' => {
                    let ext_byte = bytes[i];
                    // Check if this letter is followed by digits (version number).
                    // If so, it might be a version like "2p1" — skip digits,
                    // then if 'p' follows with more digits, skip those too
                    // (don't record 'p' as the packed-SIMD extension).
                    let had_digits = i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if had_digits
                        && i < bytes.len()
                        && bytes[i] == b'p'
                        && i + 1 < bytes.len()
                        && bytes[i + 1].is_ascii_digit()
                    {
                        // Version patch part (e.g., "p1" in "i2p1") — skip 'p' + digits
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    if let Some(ext) = Extension::from_letter(ext_byte) {
                        set.insert(ext);
                    }
                }
                b'_' => {
                    i += 1;
                    if i < bytes.len()
                        && let Some(ext) = Extension::from_letter(bytes[i])
                    {
                        set.insert(ext);
                    }
                    while i < bytes.len() && bytes[i] != b'_' {
                        i += 1;
                    }
                }
                _ => return None,
            }
        }

        Some(set)
    }

    pub fn insert(&mut self, ext: Extension) {
        self.0 |= ext.bit();
    }

    pub fn contains(self, ext: Extension) -> bool {
        self.0 & ext.bit() != 0
    }

    pub fn has_supervisor(self) -> bool {
        self.contains(Extension::S)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visionfive2_s7_no_supervisor() {
        let isa = IsaExtensions::parse("rv64imacu_zba_zbb").expect("parse failed");
        assert!(!isa.has_supervisor());
        assert!(isa.contains(Extension::I));
        assert!(isa.contains(Extension::M));
        assert!(isa.contains(Extension::A));
        assert!(isa.contains(Extension::C));
        assert!(isa.contains(Extension::U));
        assert!(!isa.contains(Extension::F));
        assert!(!isa.contains(Extension::D));
    }

    #[test]
    fn visionfive2_u74_has_supervisor() {
        let isa = IsaExtensions::parse("rv64imafdcbsux_zba_zbb").expect("parse failed");
        assert!(isa.has_supervisor());
        assert!(isa.contains(Extension::F));
        assert!(isa.contains(Extension::D));
        assert!(isa.contains(Extension::B));
    }

    #[test]
    fn qemu_full_isa() {
        let isa = IsaExtensions::parse(
            "rv64imafdch_zicbom_zicboz_zicntr_zicsr_zifencei_zihintntl_zihintpause_zihpm_zawrs_zfa_zca_zcd_zba_zbb_zbc_zbs_sstc_svadu",
        ).expect("parse failed");
        assert!(isa.has_supervisor());
        assert!(isa.contains(Extension::H));
        assert!(isa.contains(Extension::F));
        assert!(isa.contains(Extension::D));
        assert!(isa.contains(Extension::C));
    }

    #[test]
    fn supervisor_only_in_multi_letter_extensions() {
        let isa =
            IsaExtensions::parse("rv64imafdc_zicsr_zifencei_sstc_svadu").expect("parse failed");
        assert!(isa.has_supervisor());
        assert!(!isa.contains(Extension::B));
    }

    #[test]
    fn z_extensions_do_not_imply_supervisor() {
        let isa = IsaExtensions::parse("rv64imac_zba_zbb_zbc_zbs").expect("parse failed");
        assert!(!isa.has_supervisor());
    }

    #[test]
    fn rv32_base() {
        let isa = IsaExtensions::parse("rv32imac").expect("parse failed");
        assert!(isa.contains(Extension::C));
        assert!(!isa.contains(Extension::F));
    }

    #[test]
    fn version_numbers_skipped() {
        let isa = IsaExtensions::parse("rv64i2p1m2a2f2d2c").expect("parse failed");
        assert!(isa.contains(Extension::I));
        assert!(isa.contains(Extension::F));
        assert!(isa.contains(Extension::D));
        assert!(isa.contains(Extension::C));
        // 'p' should NOT be recorded — it's a version separator, not packed-SIMD
        assert!(!isa.contains(Extension::P));
    }

    #[test]
    fn invalid_prefix() {
        assert!(IsaExtensions::parse("rv128i").is_none());
        assert!(IsaExtensions::parse("arm64").is_none());
        assert!(IsaExtensions::parse("").is_none());
    }

    #[test]
    fn missing_extension() {
        let isa = IsaExtensions::parse("rv64imac").expect("parse failed");
        assert!(!isa.contains(Extension::S));
        assert!(!isa.contains(Extension::F));
        assert!(!isa.contains(Extension::D));
    }

    #[test]
    fn extension_rejects_non_letters() {
        assert!(Extension::from_letter(b'1').is_none());
        assert!(Extension::from_letter(b'A').is_none());
        assert!(Extension::from_letter(b'_').is_none());
        assert!(Extension::from_letter(b'a').is_some());
        assert!(Extension::from_letter(b'z').is_some());
    }
}
