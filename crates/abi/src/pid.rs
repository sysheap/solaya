#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tid(u64);

impl Tid {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn as_isize(self) -> isize {
        isize::try_from(self.0).expect("tid fits in isize")
    }

    pub fn try_from_i32(value: i32) -> Option<Self> {
        u64::try_from(value).ok().map(Self)
    }
}

impl core::fmt::Display for Tid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}
