#[cfg(test)]
mod tests {
    use klib::big_endian::BigEndian;

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
