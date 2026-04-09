use alloc::{sync::Arc, vec::Vec};
use headers::errno::Errno;

use crate::fs::vfs::{NodeType, VfsNode};

pub struct Ext2File {
    ino: u64,
    data: Vec<u8>,
    file_size: usize,
}

impl Ext2File {
    pub fn new(ino: u64, data: Vec<u8>, file_size: usize) -> Arc<Self> {
        Arc::new(Self {
            ino,
            data,
            file_size,
        })
    }
}

impl VfsNode for Ext2File {
    fn node_type(&self) -> NodeType {
        NodeType::File
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        self.file_size
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        if offset >= self.file_size {
            return Ok(0);
        }
        let available = &self.data[offset..self.file_size];
        let n = buf.len().min(available.len());
        buf[..n].copy_from_slice(&available[..n]);
        Ok(n)
    }

    fn write(&self, _offset: usize, _data: &[u8]) -> Result<usize, Errno> {
        Err(Errno::EROFS)
    }

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
        Err(Errno::EROFS)
    }
}
