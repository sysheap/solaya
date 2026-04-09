use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use headers::errno::Errno;

use crate::fs::vfs::{DirEntry, NodeType, VfsNode, VfsNodeRef};

pub struct Ext2Dir {
    ino: u64,
    children: BTreeMap<String, VfsNodeRef>,
}

impl Ext2Dir {
    pub fn new(ino: u64, children: BTreeMap<String, VfsNodeRef>) -> Arc<Self> {
        Arc::new(Self { ino, children })
    }
}

impl VfsNode for Ext2Dir {
    fn node_type(&self) -> NodeType {
        NodeType::Directory
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        0
    }

    fn lookup(&self, name: &str) -> Result<VfsNodeRef, Errno> {
        self.children.get(name).cloned().ok_or(Errno::ENOENT)
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, Errno> {
        Ok(self
            .children
            .iter()
            .map(|(name, node)| DirEntry {
                name: name.clone(),
                ino: node.ino(),
                node_type: node.node_type(),
            })
            .collect())
    }

    fn create(&self, _name: &str, _node_type: NodeType) -> Result<VfsNodeRef, Errno> {
        Err(Errno::EROFS)
    }

    fn unlink(&self, _name: &str) -> Result<(), Errno> {
        Err(Errno::EROFS)
    }

    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(Errno::EISDIR)
    }

    fn write(&self, _offset: usize, _data: &[u8]) -> Result<usize, Errno> {
        Err(Errno::EISDIR)
    }
}
