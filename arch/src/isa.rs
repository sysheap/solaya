/// A single-letter RISC-V ISA extension as defined by the Unprivileged ISA
/// spec (§"ISA Extension Naming Conventions") plus the privileged-mode letters
/// `S` and `U` that appear in real `riscv,isa` strings.
///
/// `K, O, R, W, Y` are not assigned by the spec. `Z` is exclusively a
/// multi-letter prefix (`Zba`, `Zicsr`, …), never a single-letter extension.
/// `X` is for non-standard custom extensions, always written as `X<name>`.
/// None of those are representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Extension {
    I,
    E,
    M,
    A,
    F,
    D,
    G,
    Q,
    L,
    C,
    B,
    J,
    T,
    P,
    V,
    N,
    H,
    S,
    U,
}

impl Extension {
    /// Return `Some` if `letter` is a defined single-letter RISC-V extension.
    pub const fn from_letter(letter: u8) -> Option<Self> {
        Some(match letter {
            b'i' => Self::I,
            b'e' => Self::E,
            b'm' => Self::M,
            b'a' => Self::A,
            b'f' => Self::F,
            b'd' => Self::D,
            b'g' => Self::G,
            b'q' => Self::Q,
            b'l' => Self::L,
            b'c' => Self::C,
            b'b' => Self::B,
            b'j' => Self::J,
            b't' => Self::T,
            b'p' => Self::P,
            b'v' => Self::V,
            b'n' => Self::N,
            b'h' => Self::H,
            b's' => Self::S,
            b'u' => Self::U,
            _ => return None,
        })
    }

    const fn bit(self) -> u32 {
        1 << (self as u32)
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
    fn from_letter_only_accepts_defined_extensions() {
        assert!(Extension::from_letter(b'i').is_some());
        assert!(Extension::from_letter(b's').is_some());
        // Z is a multi-letter prefix only, never a single-letter extension.
        assert!(Extension::from_letter(b'z').is_none());
        // K, O, R, W, Y are not assigned by the spec.
        assert!(Extension::from_letter(b'k').is_none());
        assert!(Extension::from_letter(b'w').is_none());
        // X is for non-standard extensions, always written as X<name>.
        assert!(Extension::from_letter(b'x').is_none());
        // Non-letter / wrong-case input.
        assert!(Extension::from_letter(b'A').is_none());
        assert!(Extension::from_letter(b'1').is_none());
        assert!(Extension::from_letter(b'_').is_none());
    }
}
