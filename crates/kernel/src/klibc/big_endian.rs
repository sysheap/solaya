use core::fmt::{Debug, Display};

use super::consumable_buffer::FromU8Buffer;
use common::numbers::Number;

#[derive(PartialEq, Eq, Clone, Copy, Default)]
#[repr(transparent)]
pub struct BigEndian<T: Number>(T);

impl<T: Number> BigEndian<T> {
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn from_big_endian(value: T) -> Self {
        Self(value)
    }

    pub fn from_little_endian(value: T) -> Self {
        // Use from_be to invert byte order
        Self(T::from_be(value))
    }

    pub fn get(&self) -> T {
        T::from_be(self.0)
    }
}

impl<T: Number> FromU8Buffer for BigEndian<T> {
    fn from_u8_buffer(buffer: &[u8]) -> Self {
        BigEndian(T::from_le_bytes(buffer))
    }
}

impl<T: Number> Debug for BigEndian<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.get())
    }
}

impl<T: Number> Display for BigEndian<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::BigEndian;

    #[test_case]
    fn from_little_endian_u16_roundtrip() {
        let original: u16 = 0x1234;
        let be = BigEndian::from_little_endian(original);
        assert_eq!(be.get(), original);
    }

    #[test_case]
    fn from_big_endian_u16_stores_directly() {
        // from_big_endian stores the raw value, get() converts from big-endian
        // So if we store 0x1234 directly and interpret as big-endian, we get the swapped value
        let be = BigEndian::<u16>::from_big_endian(0x1234);
        assert_eq!(be.get(), 0x3412);
    }

    #[test_case]
    fn roundtrip_u32() {
        let original: u32 = 0xDEADBEEF;
        let be = BigEndian::from_little_endian(original);
        assert_eq!(be.get(), original);
    }

    #[test_case]
    fn roundtrip_u64() {
        let original: u64 = 0x123456789ABCDEF0;
        let be = BigEndian::from_little_endian(original);
        assert_eq!(be.get(), original);
    }

    #[test_case]
    fn u8_is_identity() {
        let be1 = BigEndian::<u8>::from_little_endian(0x42);
        let be2 = BigEndian::<u8>::from_big_endian(0x42);
        assert_eq!(be1.get(), 0x42);
        assert_eq!(be2.get(), 0x42);
    }

    #[test_case]
    fn default_is_zero() {
        let be: BigEndian<u16> = BigEndian::default();
        assert_eq!(be.get(), 0);
    }

    #[test_case]
    fn equality_works() {
        let a = BigEndian::<u16>::from_little_endian(0x1234);
        let b = BigEndian::<u16>::from_little_endian(0x1234);
        let c = BigEndian::<u16>::from_little_endian(0x4321);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
