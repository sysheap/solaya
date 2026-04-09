#[cfg(test)]
mod tests {
    use crate::klibc::{
        consumable_buffer::ConsumableBuffer,
        leb128::{SignedLEB128, UnsignedLEB128},
    };

    #[test_case]
    fn unsigned_leb128() {
        const INPUT: &[u8] = &[0xe5, 0x8e, 0x26, 42];

        let mut buffer = ConsumableBuffer::new(INPUT);
        let value = buffer
            .consume_unsized_type::<UnsignedLEB128>()
            .expect("Type must be consumable")
            .get();
        assert_eq!(value, 624485);
        assert_eq!(buffer.consume_sized_type::<u8>(), Some(42));
        assert!(buffer.empty());
    }

    #[test_case]
    fn signed_leb128() {
        const INPUT: &[u8] = &[0xc0, 0xbb, 0x78, 42];

        let mut buffer = ConsumableBuffer::new(INPUT);
        let value = buffer
            .consume_unsized_type::<SignedLEB128>()
            .expect("Type must be consumable")
            .get();
        assert_eq!(value, -123456);
        assert_eq!(buffer.consume_sized_type::<u8>(), Some(42));
        assert!(buffer.empty());
    }

    #[test_case]
    fn unsigned_leb128_zero() {
        const INPUT: &[u8] = &[0x0, 42];
        let mut buffer = ConsumableBuffer::new(INPUT);
        let value = buffer
            .consume_unsized_type::<UnsignedLEB128>()
            .expect("Type must be consumable")
            .get();
        assert_eq!(value, 0);
        assert_eq!(buffer.consume_sized_type::<u8>(), Some(42));
        assert!(buffer.empty());
    }

    #[test_case]
    fn signed_leb128_zero() {
        const INPUT: &[u8] = &[0x0, 42];
        let mut buffer = ConsumableBuffer::new(INPUT);
        let value = buffer
            .consume_unsized_type::<SignedLEB128>()
            .expect("Type must be consumable")
            .get();
        assert_eq!(value, 0);
        assert_eq!(buffer.consume_sized_type::<u8>(), Some(42));
        assert!(buffer.empty());
    }
}
