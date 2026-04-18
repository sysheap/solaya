use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use headers::errno::Errno;

use hal::spinlock::Spinlock;

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

    fn nlink(&self) -> u32 {
        self.metadata.lock().nlink
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

    fn inc_nlink(&self) {
        self.metadata.lock().nlink += 1;
    }

    fn dec_nlink(&self) {
        self.metadata.lock().nlink -= 1;
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

    fn nlink(&self) -> u32 {
        self.metadata.lock().nlink
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
            NodeType::Symlink => return Err(Errno::EINVAL),
        };
        children.insert(name.to_string(), node.clone());
        Ok(node)
    }

    fn create_symlink(&self, name: &str, target: &str) -> Result<VfsNodeRef, Errno> {
        let mut children = self.children.lock();
        if children.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        let node: VfsNodeRef = TmpfsSymlink::new(target.to_string());
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

    fn link(&self, name: &str, node: VfsNodeRef) -> Result<(), Errno> {
        let mut children = self.children.lock();
        if children.contains_key(name) {
            return Err(Errno::EEXIST);
        }
        children.insert(name.to_string(), node);
        Ok(())
    }

    fn remove_child(&self, name: &str) -> Result<VfsNodeRef, Errno> {
        self.children.lock().remove(name).ok_or(Errno::ENOENT)
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
}

pub struct TmpfsSymlink {
    ino: u64,
    target: String,
}

impl TmpfsSymlink {
    pub fn new(target: String) -> Arc<Self> {
        Arc::new(Self {
            ino: alloc_ino(),
            target,
        })
    }
}

impl VfsNode for TmpfsSymlink {
    fn node_type(&self) -> NodeType {
        NodeType::Symlink
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        self.target.len()
    }

    fn nlink(&self) -> u32 {
        1
    }

    fn readlink(&self) -> Result<String, Errno> {
        Ok(self.target.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn create_symlink_stores_target_and_shows_up_in_lookup() {
        let dir = TmpfsDir::new();
        let link = dir.create_symlink("sh", "/bin/dash").expect("create");
        assert_eq!(link.node_type(), NodeType::Symlink);
        assert_eq!(link.readlink().expect("readlink"), "/bin/dash");

        let found = dir.lookup("sh").expect("lookup");
        assert_eq!(found.ino(), link.ino());
        assert_eq!(found.readlink().expect("readlink"), "/bin/dash");
    }

    #[test_case]
    fn create_symlink_rejects_duplicates() {
        let dir = TmpfsDir::new();
        dir.create_symlink("sh", "/bin/dash").expect("first");
        let err = dir
            .create_symlink("sh", "/bin/ash")
            .err()
            .expect("second must fail");
        assert_eq!(err, Errno::EEXIST);
    }

    #[test_case]
    fn link_shares_file_and_increments_nlink() {
        // Simulates what initramfs::extract does for a hardlink: the second
        // cpio entry with the same ino reuses the Arc of the first.
        let dir = TmpfsDir::new();
        let original = dir.create("cat", NodeType::File).expect("create");
        original.write(0, b"BIN").expect("write");
        original.inc_nlink();
        dir.link("head", original.clone()).expect("link");

        let looked_up = dir.lookup("head").expect("lookup");
        assert_eq!(looked_up.ino(), original.ino());
        assert_eq!(looked_up.nlink(), 2);

        let mut buf = [0u8; 3];
        let n = looked_up.read(0, &mut buf).expect("read");
        assert_eq!(&buf[..n], b"BIN");
    }

    #[test_case]
    fn link_rejects_duplicate_name() {
        let dir = TmpfsDir::new();
        let file = dir.create("a", NodeType::File).expect("create");
        let err = dir.link("a", file).err().expect("link must fail");
        assert_eq!(err, Errno::EEXIST);
    }
}
