use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use headers::errno::Errno;

use crate::{
    io::tty_device::{TtyDevice, console_tty},
    klibc::Spinlock,
};

use super::vfs::{DirEntry, NodeType, VfsNode, VfsNodeRef, alloc_ino};

struct DevNull {
    ino: u64,
}

impl VfsNode for DevNull {
    fn node_type(&self) -> NodeType {
        NodeType::File
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        0
    }

    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, Errno> {
        Ok(0)
    }

    fn write(&self, _offset: usize, data: &[u8]) -> Result<usize, Errno> {
        Ok(data.len())
    }

    fn truncate(&self) -> Result<(), Errno> {
        Ok(())
    }
}

struct DevZero {
    ino: u64,
}

impl VfsNode for DevZero {
    fn node_type(&self) -> NodeType {
        NodeType::File
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        0
    }

    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn write(&self, _offset: usize, data: &[u8]) -> Result<usize, Errno> {
        Ok(data.len())
    }

    fn truncate(&self) -> Result<(), Errno> {
        Ok(())
    }
}

struct DevConsole {
    ino: u64,
    device: TtyDevice,
}

impl VfsNode for DevConsole {
    fn node_type(&self) -> NodeType {
        NodeType::File
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        0
    }

    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        let mut dev = self.device.lock();
        let data = dev.get_input(buf.len());
        if data.is_empty() {
            return Err(Errno::EAGAIN);
        }
        buf[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }

    fn write(&self, _offset: usize, data: &[u8]) -> Result<usize, Errno> {
        let processed = self.device.lock().process_output(data);
        let mut uart = crate::io::uart::QEMU_UART.lock();
        for &b in &processed {
            uart.write_byte(b);
        }
        Ok(data.len())
    }

    fn truncate(&self) -> Result<(), Errno> {
        Ok(())
    }
}

struct DevfsDir {
    ino: u64,
    entries: Spinlock<BTreeMap<String, VfsNodeRef>>,
}

impl VfsNode for DevfsDir {
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
        self.entries.lock().get(name).cloned().ok_or(Errno::ENOENT)
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, Errno> {
        Ok(self
            .entries
            .lock()
            .iter()
            .map(|(name, node)| DirEntry {
                name: name.clone(),
                ino: node.ino(),
                node_type: node.node_type(),
            })
            .collect())
    }
}

static DEVFS: Spinlock<Option<Arc<DevfsDir>>> = Spinlock::new(None);

pub(super) fn new() -> VfsNodeRef {
    let mut entries = BTreeMap::new();
    entries.insert(
        String::from("null"),
        Arc::new(DevNull { ino: alloc_ino() }) as VfsNodeRef,
    );
    entries.insert(
        String::from("zero"),
        Arc::new(DevZero { ino: alloc_ino() }) as VfsNodeRef,
    );
    entries.insert(
        String::from("console"),
        Arc::new(DevConsole {
            ino: alloc_ino(),
            device: console_tty().clone(),
        }) as VfsNodeRef,
    );

    let dir = Arc::new(DevfsDir {
        ino: alloc_ino(),
        entries: Spinlock::new(entries),
    });
    *DEVFS.lock() = Some(dir.clone());
    dir
}

pub fn register_device(name: &str, node: VfsNodeRef) {
    let dir = DEVFS
        .lock()
        .clone()
        .expect("devfs must be initialized before registering devices");
    dir.entries.lock().insert(String::from(name), node);
}

pub fn alloc_dev_ino() -> u64 {
    alloc_ino()
}
