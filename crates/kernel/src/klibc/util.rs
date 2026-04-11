pub use klib::util::{
    InBytes, UsizeExt, align_up, as_byte_slice, copy_slice, is_aligned, is_power_of_2_or_zero,
    ref_from_bytes, wrapping_add_signed,
};
pub use mm::util::*;

#[cfg(miri)]
pub use klib::util::align_down;

pub trait BufferExtension {
    fn interpret_as<T>(&self) -> &T;
    fn split_as<T>(&self) -> (&T, &[u8]);
}

impl BufferExtension for [u8] {
    fn interpret_as<T>(&self) -> &T {
        ref_from_bytes(self)
    }

    fn split_as<T>(&self) -> (&T, &[u8]) {
        let (header_bytes, rest) = self.split_at(core::mem::size_of::<T>());
        (header_bytes.interpret_as(), rest)
    }
}

pub trait ByteInterpretable {
    fn as_slice(&self) -> &[u8] {
        as_byte_slice(self)
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::memory::PAGE_SIZE;

    #[kani::proof]
    fn align_up_is_at_least_input() {
        let value: usize = kani::any();
        let alignment: usize = kani::any();
        kani::assume(alignment > 0);
        kani::assume(alignment <= 4096);
        kani::assume(alignment.is_power_of_two());
        kani::assume(value <= usize::MAX - alignment);
        let result = align_up(value, alignment);
        assert!(result >= value);
    }

    #[kani::proof]
    fn align_up_is_multiple_of_alignment() {
        let value: usize = kani::any();
        let alignment: usize = kani::any();
        kani::assume(alignment > 0);
        kani::assume(alignment <= 4096);
        kani::assume(alignment.is_power_of_two());
        kani::assume(value <= usize::MAX - alignment);
        let result = align_up(value, alignment);
        assert!(result % alignment == 0);
    }

    #[kani::proof]
    fn align_up_idempotent() {
        let value: usize = kani::any();
        let alignment: usize = kani::any();
        kani::assume(alignment > 0);
        kani::assume(alignment <= 4096);
        kani::assume(alignment.is_power_of_two());
        kani::assume(value <= usize::MAX - alignment);
        let once = align_up(value, alignment);
        let twice = align_up(once, alignment);
        assert!(once == twice);
    }

    #[kani::proof]
    fn set_clear_bit_roundtrip() {
        let original: u64 = kani::any();
        let bit_pos: usize = kani::any();
        kani::assume(bit_pos < 64);
        let mut data = original;
        set_bit(&mut data, bit_pos);
        clear_bit(&mut data, bit_pos);
        let mut expected = original;
        clear_bit(&mut expected, bit_pos);
        assert!(data == expected);
    }

    #[kani::proof]
    fn set_bit_only_affects_target() {
        let original: u64 = kani::any();
        let bit_pos: usize = kani::any();
        kani::assume(bit_pos < 64);
        let mut data = original;
        set_bit(&mut data, bit_pos);
        let mask = !(1u64 << bit_pos);
        assert!((data & mask) == (original & mask));
    }

    #[kani::proof]
    #[kani::unwind(9)]
    fn set_get_multiple_bits_roundtrip() {
        let mut data: u64 = kani::any();
        let value: u8 = kani::any();
        let n_bits: usize = kani::any();
        let bit_pos: usize = kani::any();
        kani::assume(n_bits > 0 && n_bits <= 8);
        kani::assume(bit_pos <= 64 - n_bits);
        let mask = u8::MAX >> (8 - n_bits);
        kani::assume(value == value & mask);
        set_multiple_bits(&mut data, value, n_bits, bit_pos);
        let got: u8 = get_multiple_bits::<u64, u64>(data, n_bits, bit_pos)
            .try_into()
            .expect("fits in u8");
        assert!(got == value);
    }

    #[kani::proof]
    fn minimum_pages_covers_value() {
        let value: usize = kani::any();
        kani::assume(value > 0);
        kani::assume(value <= usize::MAX - PAGE_SIZE);
        let pages = minimum_amount_of_pages(value);
        assert!(pages * PAGE_SIZE >= value);
        assert!((pages - 1) * PAGE_SIZE < value);
    }
}

#[cfg(test)]
mod tests {
    use crate::memory::PAGE_SIZE;
    use klib::util::BufferExtension;

    #[test_case]
    fn align_up() {
        assert_eq!(super::align_up(26, 4), 28);
        assert_eq!(super::align_up(37, 3), 39);
        assert_eq!(super::align_up(64, 2), 64);
    }

    #[test_case]
    fn align_up_number_of_pages() {
        assert_eq!(super::minimum_amount_of_pages(PAGE_SIZE - 15), 1);
        assert_eq!(super::minimum_amount_of_pages(PAGE_SIZE + 15), 2);
        assert_eq!(super::minimum_amount_of_pages(PAGE_SIZE * 2), 2);
    }

    #[test_case]
    fn copy_from_slice() {
        let src = [1, 2, 3, 4, 5];
        let mut dst = [0, 0, 0, 0, 0, 0, 0];
        super::copy_slice(&src, &mut dst);
        assert_eq!(dst, [1, 2, 3, 4, 5, 0, 0]);
    }

    #[test_case]
    fn split_as_parses_header_and_remainder() {
        #[repr(C)]
        struct Header {
            tag: u16,
            len: u16,
            flags: u8,
        }

        let payload = [0xAA, 0xBB, 0xCC];
        let total_len = core::mem::size_of::<Header>() + payload.len();
        #[repr(C, align(2))]
        struct AlignedBuf([u8; 16]);
        let mut storage = AlignedBuf([0u8; 16]);
        let buf = &mut storage.0[..total_len];
        buf[0..2].copy_from_slice(&0xCAFEu16.to_ne_bytes());
        buf[2..4].copy_from_slice(&128u16.to_ne_bytes());
        buf[4] = 0x07;
        buf[core::mem::size_of::<Header>()..].copy_from_slice(&payload);

        let (header, rest) = buf.split_as::<Header>();

        assert_eq!(header.tag, 0xCAFE);
        assert_eq!(header.len, 128);
        assert_eq!(header.flags, 0x07);
        assert_eq!(rest, &payload);
    }
}
