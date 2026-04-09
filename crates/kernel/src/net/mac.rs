use core::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    pub const fn new(address: [u8; 6]) -> Self {
        Self(address)
    }

    pub fn as_bytes(&self) -> [u8; 6] {
        self.0
    }
}

impl Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}
