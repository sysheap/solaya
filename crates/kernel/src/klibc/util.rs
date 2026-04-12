pub use klib::util::{
    InBytes, UsizeExt, align_up, as_byte_slice, copy_slice, is_aligned, ref_from_bytes,
    wrapping_add_signed,
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
