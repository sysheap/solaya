/// Parsed single-letter extensions from a RISC-V ISA string.
/// Bit N corresponds to extension letter `'a' + N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsaExtensions {
    bits: u32,
}

impl IsaExtensions {
    /// Parse a `riscv,isa` device tree string like `"rv64imafdcbsux_zba_zbb"`.
    /// Returns `None` if the prefix is not `rv32` or `rv64`.
    pub fn parse(isa_str: &str) -> Option<Self> {
        let remainder = isa_str
            .strip_prefix("rv64")
            .or_else(|| isa_str.strip_prefix("rv32"))?;

        let bytes = remainder.as_bytes();
        let mut bits = 0u32;
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
                    bits |= 1 << (ext_byte - b'a');
                }
                b'_' => {
                    i += 1;
                    if i < bytes.len() && bytes[i].is_ascii_lowercase() {
                        bits |= 1 << (bytes[i] - b'a');
                    }
                    while i < bytes.len() && bytes[i] != b'_' {
                        i += 1;
                    }
                }
                _ => return None,
            }
        }

        Some(Self { bits })
    }

    pub fn has_extension(self, ext: char) -> bool {
        let ext = ext as u32;
        let a = 'a' as u32;
        if ext < a || ext > a + 25 {
            return false;
        }
        self.bits & (1 << (ext - a)) != 0
    }

    pub fn has_supervisor(self) -> bool {
        self.has_extension('s')
    }
}
