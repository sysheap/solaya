#[cfg(test)]
mod tests {
    use crate::memory::PAGE_SIZE;
    use klib::util::{BufferExtension, align_up, copy_slice};
    use mm::util::minimum_amount_of_pages;

    #[test_case]
    fn align_up_basic() {
        assert_eq!(align_up(26, 4), 28);
        assert_eq!(align_up(37, 3), 39);
        assert_eq!(align_up(64, 2), 64);
    }

    #[test_case]
    fn align_up_number_of_pages() {
        assert_eq!(minimum_amount_of_pages(PAGE_SIZE - 15), 1);
        assert_eq!(minimum_amount_of_pages(PAGE_SIZE + 15), 2);
        assert_eq!(minimum_amount_of_pages(PAGE_SIZE * 2), 2);
    }

    #[test_case]
    fn copy_from_slice() {
        let src = [1, 2, 3, 4, 5];
        let mut dst = [0, 0, 0, 0, 0, 0, 0];
        copy_slice(&src, &mut dst);
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
