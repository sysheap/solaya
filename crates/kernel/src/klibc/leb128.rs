use super::consumable_buffer::{ConsumableBuffer, FromU8BufferUnsized};

#[derive(Clone, Copy)]
pub struct UnsignedLEB128 {
    value: u64,
    size: usize,
}

impl UnsignedLEB128 {
    pub fn get(self) -> u64 {
        self.value
    }

    fn parse(buffer: &[u8]) -> Option<Self> {
        let mut result = 0u64;
        let mut shift = 0u8;

        let mut buffer = ConsumableBuffer::new(buffer);
        let mut size = 0;

        loop {
            let next_byte = buffer.consume_sized_type::<u8>()? as u64;
            size += 1;
            result |= (next_byte & 0b1111111) << shift;
            if next_byte & 0b10000000 == 0 {
                return Some(Self {
                    value: result,
                    size,
                });
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
    }
}

impl FromU8BufferUnsized for UnsignedLEB128 {
    fn from_u8_buffer(buffer: &[u8]) -> Option<Self> {
        UnsignedLEB128::parse(buffer)
    }

    fn size_in_bytes(&self) -> usize {
        self.size
    }
}

#[derive(Clone, Copy)]
pub struct SignedLEB128 {
    value: i64,
    size: usize,
}

impl SignedLEB128 {
    pub fn get(&self) -> i64 {
        self.value
    }

    fn parse(buffer: &[u8]) -> Option<Self> {
        let mut result = 0i64;
        let mut shift = 0u8;
        let result_size = 64;

        let mut buffer = ConsumableBuffer::new(buffer);
        let mut size = 0;
        let mut next_byte: i64;
        loop {
            next_byte = buffer.consume_sized_type::<u8>()? as i64;
            size += 1;
            result |= (next_byte & 0b1111111) << shift;
            shift += 7;
            if shift >= result_size {
                return None;
            }
            if next_byte & 0b10000000 == 0 {
                break;
            }
        }

        if shift < result_size && next_byte & 0x40 != 0 {
            result |= !0 << shift;
        }
        Some(Self {
            value: result,
            size,
        })
    }
}

impl FromU8BufferUnsized for SignedLEB128 {
    fn from_u8_buffer(buffer: &[u8]) -> Option<Self> {
        SignedLEB128::parse(buffer)
    }

    fn size_in_bytes(&self) -> usize {
        self.size
    }
}
