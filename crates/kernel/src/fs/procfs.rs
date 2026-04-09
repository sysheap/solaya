use alloc::sync::Arc;
use headers::errno::Errno;

use super::vfs::{NodeType, StaticDir, VfsNode, VfsNodeRef, alloc_ino};

struct ProcVersionFile {
    ino: u64,
}

const VERSION_STRING: &[u8] = b"Solaya 0.1.0\n";

impl VfsNode for ProcVersionFile {
    fn node_type(&self) -> NodeType {
        NodeType::File
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        VERSION_STRING.len()
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        if offset >= VERSION_STRING.len() {
            return Ok(0);
        }
        let available = &VERSION_STRING[offset..];
        let n = buf.len().min(available.len());
        buf[..n].copy_from_slice(&available[..n]);
        Ok(n)
    }

    fn write(&self, _offset: usize, _data: &[u8]) -> Result<usize, Errno> {
        Err(Errno::EACCES)
    }
}

pub(super) fn new() -> Arc<StaticDir> {
    StaticDir::new(vec![(
        "version",
        Arc::new(ProcVersionFile { ino: alloc_ino() }) as VfsNodeRef,
    )])
}
