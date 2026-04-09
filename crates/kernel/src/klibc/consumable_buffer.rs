use super::util::align_up;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumableBuffer<'a> {
    buffer: &'a [u8],
    position: usize,
}

impl<'a> ConsumableBuffer<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }

    pub fn buffer(&self) -> &'a [u8] {
        self.buffer
    }

    pub fn reset(&mut self) {
        self.position = 0;
    }

    pub fn reset_and_clone(&self) -> Self {
        Self {
            buffer: self.buffer,
            position: 0,
        }
    }

    pub fn consume_slice(&mut self, size: usize) -> Option<&'a [u8]> {
        if self.position + size > self.buffer.len() {
            return None;
        }

        if size == 0 {
            return Some(&[]);
        }

        let result = &self.buffer[self.position..self.position + size];
        self.position += size;
        Some(result)
    }

    pub fn consume_sized_type<T: FromU8Buffer>(&mut self) -> Option<T> {
        let size = core::mem::size_of::<T>();
        let result = self.consume_slice(size)?;
        Some(T::from_u8_buffer(result))
    }

    pub fn consume_unsized_type<T: FromU8BufferUnsized>(&mut self) -> Option<T> {
        let result = T::from_u8_buffer(self.rest());
        if let Some(result) = result {
            let size = result.size_in_bytes();
            if self.position + size > self.buffer.len() {
                return None;
            }
            self.position += size;
        }
        result
    }

    pub fn consume_alignment(&mut self, alignment: usize) -> Option<()> {
        let aligned_value = align_up(self.position, alignment);
        let diff = aligned_value - self.position;
        self.consume_slice(diff)?;
        Some(())
    }

    pub fn consume_str(&mut self) -> Option<&'a str> {
        let mut length = 0;
        while self.position + length < self.buffer.len() && self.buffer[self.position + length] != 0
        {
            length += 1;
        }
        // Check if we really found a null-terminated string
        if self.position + length >= self.buffer.len() || self.buffer[self.position + length] != 0 {
            return None;
        }

        let string =
            core::str::from_utf8(&self.buffer[self.position..self.position + length]).ok()?;

        // Consume null byte
        length += 1;

        self.position += length;

        Some(string)
    }

    pub fn empty(&self) -> bool {
        self.position >= self.buffer.len()
    }

    pub fn size_left(&self) -> usize {
        if self.position >= self.buffer.len() {
            0
        } else {
            self.buffer.len() - self.position
        }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn rest(&self) -> &[u8] {
        &self.buffer[self.position..]
    }
}

pub trait FromU8Buffer: Copy {
    fn from_u8_buffer(buffer: &[u8]) -> Self;
}

impl<T: common::numbers::Number> FromU8Buffer for T {
    fn from_u8_buffer(buffer: &[u8]) -> Self {
        T::from_le_bytes(buffer)
    }
}

pub trait FromU8BufferUnsized: Copy {
    fn from_u8_buffer(buffer: &[u8]) -> Option<Self>;
    fn size_in_bytes(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::ConsumableBuffer;

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
