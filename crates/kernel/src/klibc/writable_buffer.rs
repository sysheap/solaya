use super::util::copy_slice;

pub struct WritableBuffer<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

pub enum WritableBufferError {
    BufferTooSmall,
}

impl<'a> WritableBuffer<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    pub fn write_usize(&mut self, value: usize) -> Result<(), WritableBufferError> {
        let target = &mut self.buf[self.offset..];
        let value = value.to_le_bytes();
        if core::mem::size_of_val(&value) > target.len() {
            return Err(WritableBufferError::BufferTooSmall);
        }
        copy_slice(&value, target);
        self.offset += core::mem::size_of_val(&value);
        Ok(())
    }

    pub fn write_slice(&mut self, value: &[u8]) -> Result<(), WritableBufferError> {
        let target = &mut self.buf[self.offset..];
        if value.len() > target.len() {
            return Err(WritableBufferError::BufferTooSmall);
        }
        copy_slice(value, target);
        self.offset += value.len();
        Ok(())
    }
}
