use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use headers::errno::Errno;

use crate::klibc::Spinlock;

use super::vfs::{DirEntry, NodeType, VfsNode, VfsNodeRef, alloc_ino};

struct TmpfsMetadata {
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u32,
}

pub struct TmpfsFile {
    ino: u64,
    data: Spinlock<Vec<u8>>,
    metadata: Spinlock<TmpfsMetadata>,
}

impl TmpfsFile {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            ino: alloc_ino(),
            data: Spinlock::new(Vec::new()),
            metadata: Spinlock::new(TmpfsMetadata {
                mode: headers::fs::S_IFREG | 0o644,
                uid: 0,
                gid: 0,
                nlink: 1,
            }),
        })
    }
}

impl VfsNode for TmpfsFile {
    fn node_type(&self) -> NodeType {
        NodeType::File
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        self.data.lock().len()
    }

    fn mode(&self) -> u32 {
        self.metadata.lock().mode
    }

    fn uid(&self) -> u32 {
        self.metadata.lock().uid
    }

    fn gid(&self) -> u32 {
        self.metadata.lock().gid
    }

    fn set_mode(&self, mode: u32) -> Result<(), Errno> {
        self.metadata.lock().mode = mode;
        Ok(())
    }

    fn set_owner(&self, uid: u32, gid: u32) -> Result<(), Errno> {
        let mut meta = self.metadata.lock();
        meta.uid = uid;
        meta.gid = gid;
        Ok(())
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        let data = self.data.lock();
        if offset >= data.len() {
            return Ok(0);
        }
        let available = &data[offset..];
        let n = buf.len().min(available.len());
        buf[..n].copy_from_slice(&available[..n]);
        Ok(n)
    }

    fn write(&self, offset: usize, data: &[u8]) -> Result<usize, Errno> {
        let mut content = self.data.lock();
        let end = offset + data.len();
        if end > content.len() {
            content.resize(end, 0);
        }
        content[offset..end].copy_from_slice(data);
        Ok(data.len())
    }

    fn truncate(&self, length: usize) -> Result<(), Errno> {
        self.data.lock().resize(length, 0);
        Ok(())
    }

    fn mode(&self) -> u32 {
        self.metadata.lock().mode
    }

    fn uid(&self) -> u32 {
        self.metadata.lock().uid
    }

    fn gid(&self) -> u32 {
        self.metadata.lock().gid
    }

    fn nlink(&self) -> u32 {
        self.metadata.lock().nlink
    }
}

pub struct TmpfsDir {
    ino: u64,
    children: Spinlock<BTreeMap<String, VfsNodeRef>>,
    metadata: Spinlock<TmpfsMetadata>,
}

impl TmpfsDir {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            ino: alloc_ino(),
            children: Spinlock::new(BTreeMap::new()),
            metadata: Spinlock::new(TmpfsMetadata {
                mode: headers::fs::S_IFDIR | 0o755,
                uid: 0,
                gid: 0,
                nlink: 2,
            }),
        })
    }
}

impl VfsNode for TmpfsDir {
    fn node_type(&self) -> NodeType {
        NodeType::Directory
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        0
    }

    fn mode(&self) -> u32 {
        self.metadata.lock().mode
    }

    fn uid(&self) -> u32 {
        self.metadata.lock().uid
    }

    fn gid(&self) -> u32 {
        self.metadata.lock().gid
    }

    fn set_mode(&self, mode: u32) -> Result<(), Errno> {
        self.metadata.lock().mode = mode;
        Ok(())
    }

    fn set_owner(&self, uid: u32, gid: u32) -> Result<(), Errno> {
        let mut meta = self.metadata.lock();
        meta.uid = uid;
        meta.gid = gid;
        Ok(())
    }

    fn lookup(&self, name: &str) -> Result<VfsNodeRef, Errno> {
        self.children.lock().get(name).cloned().ok_or(Errno::ENOENT)
    }

    fn create(&self, name: &str, node_type: NodeType) -> Result<VfsNodeRef, Errno> {
        let mut children = self.children.lock();
        if children.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        let node: VfsNodeRef = match node_type {
            NodeType::File => TmpfsFile::new(),
            NodeType::Directory => {
                self.metadata.lock().nlink += 1;
                TmpfsDir::new()
            }
        };
        children.insert(name.to_string(), node.clone());
        Ok(node)
    }

    fn unlink(&self, name: &str) -> Result<(), Errno> {
        let mut children = self.children.lock();
        let node = children.remove(name).ok_or(Errno::ENOENT)?;
        if node.node_type() == NodeType::Directory {
            self.metadata.lock().nlink -= 1;
        }
        Ok(())
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, Errno> {
        let children = self.children.lock();
        Ok(children
            .iter()
            .map(|(name, node)| DirEntry {
                name: name.clone(),
                ino: node.ino(),
                node_type: node.node_type(),
            })
            .collect())
    }

    fn mode(&self) -> u32 {
        self.metadata.lock().mode
    }

    fn uid(&self) -> u32 {
        self.metadata.lock().uid
    }

    fn gid(&self) -> u32 {
        self.metadata.lock().gid
    }

    fn nlink(&self) -> u32 {
        self.metadata.lock().nlink
    }
}
