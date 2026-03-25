use alloc::sync::Arc;
use headers::errno::Errno;

use crate::klibc::Spinlock;

use super::vfs::{NodeType, VfsNodeRef};

pub struct VfsOpenFileInner {
    node: VfsNodeRef,
    offset: usize,
    flags: i32,
}

pub type VfsOpenFile = Arc<Spinlock<VfsOpenFileInner>>;

pub fn open(node: VfsNodeRef, flags: i32) -> VfsOpenFile {
    Arc::new(Spinlock::new(VfsOpenFileInner {
        node,
        offset: 0,
        flags,
    }))
}

impl VfsOpenFileInner {
    pub fn node(&self) -> &VfsNodeRef {
        &self.node
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
        let n = self.node.read(self.offset, buf)?;
        self.offset += n;
        Ok(n)
    }

    pub fn pread(&self, offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        self.node.read(offset, buf)
    }

    pub fn pwrite(&self, offset: usize, data: &[u8]) -> Result<usize, Errno> {
        self.node.write(offset, data)
    }

    pub fn write(&mut self, data: &[u8]) -> Result<usize, Errno> {
        use headers::syscall_types::O_APPEND;
        if (self.flags.cast_unsigned() & O_APPEND) != 0 {
            self.offset = self.node.size();
        }
        let n = self.node.write(self.offset, data)?;
        self.offset += n;
        Ok(n)
    }

    pub fn seek(&mut self, offset: isize, whence: i32) -> Result<usize, Errno> {
        use headers::fs::{SEEK_CUR, SEEK_END, SEEK_SET};
        let new_offset = match whence {
            SEEK_SET => {
                if offset < 0 {
                    return Err(Errno::EINVAL);
                }
                offset.cast_unsigned()
            }
            SEEK_CUR => {
                let cur = self.offset as isize;
                let new = cur.checked_add(offset).ok_or(Errno::EINVAL)?;
                if new < 0 {
                    return Err(Errno::EINVAL);
                }
                new.cast_unsigned()
            }
            SEEK_END => {
                let size = self.node.size() as isize;
                let new = size.checked_add(offset).ok_or(Errno::EINVAL)?;
                if new < 0 {
                    return Err(Errno::EINVAL);
                }
                new.cast_unsigned()
            }
            _ => return Err(Errno::EINVAL),
        };
        self.offset = new_offset;
        Ok(new_offset)
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn advance_offset(&mut self, n: usize) {
        self.offset += n;
    }

    pub fn effective_write_offset(&mut self) -> usize {
        use headers::syscall_types::O_APPEND;
        if (self.flags.cast_unsigned() & O_APPEND) != 0 {
            self.offset = self.node.size();
        }
        self.offset
    }

    pub fn is_directory(&self) -> bool {
        self.node.node_type() == NodeType::Directory
    }
}
