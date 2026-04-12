#[cfg(test)]
mod tests {
    use klib::parser::ConsumableBuffer;

    #[test_case]
    fn consume_slice_basic() {
        let data = [1, 2, 3, 4, 5];
        let mut buf = ConsumableBuffer::new(&data);
        assert_eq!(buf.consume_slice(3), Some([1, 2, 3].as_slice()));
        assert_eq!(buf.position(), 3);
        assert_eq!(buf.consume_slice(2), Some([4, 5].as_slice()));
        assert_eq!(buf.position(), 5);
    }

    #[test_case]
    fn consume_slice_beyond_end() {
        let data = [1, 2];
        let mut buf = ConsumableBuffer::new(&data);
        assert_eq!(buf.consume_slice(3), None);
        assert_eq!(buf.position(), 0);
    }

    #[test_case]
    fn consume_slice_zero_size() {
        let data = [1, 2];
        let mut buf = ConsumableBuffer::new(&data);
        assert_eq!(buf.consume_slice(0), Some([].as_slice()));
    }

    #[test_case]
    fn consume_str_basic() {
        let data = b"hello\0world\0";
        let mut buf = ConsumableBuffer::new(data);
        assert_eq!(buf.consume_str(), Some("hello"));
        assert_eq!(buf.consume_str(), Some("world"));
    }

    #[test_case]
    fn consume_str_no_null_terminator() {
        let data = b"abc";
        let mut buf = ConsumableBuffer::new(data);
        assert_eq!(buf.consume_str(), None);
    }

    #[test_case]
    fn consume_str_empty_string() {
        let data = b"\0";
        let mut buf = ConsumableBuffer::new(data);
        assert_eq!(buf.consume_str(), Some(""));
    }

    #[test_case]
    fn consume_str_on_exhausted_buffer() {
        let data = b"a\0";
        let mut buf = ConsumableBuffer::new(data);
        buf.consume_str();
        assert_eq!(buf.consume_str(), None);
    }

    #[test_case]
    fn consume_alignment() {
        let data = [0u8; 8];
        let mut buf = ConsumableBuffer::new(&data);
        buf.consume_slice(1);
        assert_eq!(buf.position(), 1);
        buf.consume_alignment(4);
        assert_eq!(buf.position(), 4);
    }

    #[test_case]
    fn consume_alignment_already_aligned() {
        let data = [0u8; 8];
        let mut buf = ConsumableBuffer::new(&data);
        buf.consume_slice(4);
        buf.consume_alignment(4);
        assert_eq!(buf.position(), 4);
    }

    #[test_case]
    fn size_left_and_empty() {
        let data = [1, 2, 3];
        let mut buf = ConsumableBuffer::new(&data);
        assert_eq!(buf.size_left(), 3);
        assert!(!buf.empty());
        buf.consume_slice(3);
        assert_eq!(buf.size_left(), 0);
        assert!(buf.empty());
    }

    #[test_case]
    fn reset_and_clone() {
        let data = [1, 2, 3];
        let mut buf = ConsumableBuffer::new(&data);
        buf.consume_slice(2);
        assert_eq!(buf.position(), 2);
        let cloned = buf.reset_and_clone();
        assert_eq!(cloned.position(), 0);
        assert_eq!(buf.position(), 2);
    }
}
