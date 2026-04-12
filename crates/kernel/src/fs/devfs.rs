use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use driver_api::{BlockDevice, CharDevice, DisplayDevice, InputDevice, RngDevice};
use headers::errno::Errno;

use crate::klibc::Spinlock;

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

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
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

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
        Ok(())
    }
}

struct CharNode {
    ino: u64,
    device: Arc<dyn CharDevice>,
}

impl VfsNode for CharNode {
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
        self.device.read(buf)
    }

    fn write(&self, _offset: usize, data: &[u8]) -> Result<usize, Errno> {
        self.device.write(data)
    }

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
        Ok(())
    }
}

struct DisplayNode {
    ino: u64,
    device: Arc<dyn DisplayDevice>,
    size: usize,
}

impl VfsNode for DisplayNode {
    fn node_type(&self) -> NodeType {
        NodeType::File
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        self.size
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, Errno> {
        self.device.read_at(offset, buf)
    }

    fn write(&self, offset: usize, data: &[u8]) -> Result<usize, Errno> {
        self.device.write_at(offset, data)
    }

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
        Ok(())
    }
}

struct InputNode {
    ino: u64,
    device: Arc<dyn InputDevice>,
}

impl VfsNode for InputNode {
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
        let event_size = core::mem::size_of::<driver_api::InputEvent>();
        let max_events = buf.len() / event_size;
        let mut written = 0;
        for _ in 0..max_events {
            let Some(event) = self.device.poll_event() else {
                break;
            };
            let bytes = klib::util::as_byte_slice(&event);
            buf[written..written + event_size].copy_from_slice(bytes);
            written += event_size;
        }
        if written == 0 {
            return Err(Errno::EAGAIN);
        }
        Ok(written)
    }

    fn write(&self, _offset: usize, data: &[u8]) -> Result<usize, Errno> {
        Ok(data.len())
    }

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
        Ok(())
    }
}

struct RngNode {
    ino: u64,
    device: Arc<dyn RngDevice>,
}

impl VfsNode for RngNode {
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
        self.device.fill(buf)
    }

    fn write(&self, _offset: usize, data: &[u8]) -> Result<usize, Errno> {
        Ok(data.len())
    }

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
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

/// Generic devfs node for any `BlockDevice`. Exposes the device as a regular
/// file; `block_device()` returns the Arc so callers can perform direct I/O
/// without going through in-memory VFS caching.
struct BlockNode {
    ino: u64,
    device: Arc<dyn BlockDevice>,
}

impl VfsNode for BlockNode {
    fn node_type(&self) -> NodeType {
        NodeType::File
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        (self.device.num_blocks() as usize) * self.device.block_size()
    }

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
        Err(Errno::EINVAL)
    }

    fn block_device(&self) -> Option<Arc<dyn BlockDevice>> {
        Some(self.device.clone())
    }
}

/// Register a block device with devfs. The devfs entry name is taken from
/// `device.name()` (e.g. `"vda"`). Must be called after `init()`.
pub fn register_block_device(device: Arc<dyn BlockDevice>) {
    let name = String::from(device.name());
    let node: VfsNodeRef = Arc::new(BlockNode {
        ino: alloc_ino(),
        device,
    });
    register_device(&name, node);
}

/// Register a character device with devfs under `name` (e.g. `"console"`).
/// The devfs entry name is passed in explicitly — one CharDevice may be
/// exposed under multiple names.
pub fn register_char_device(name: &str, device: Arc<dyn CharDevice>) {
    let node: VfsNodeRef = Arc::new(CharNode {
        ino: alloc_ino(),
        device,
    });
    register_device(name, node);
}

/// Register a display device. The devfs entry name is taken from
/// `device.name()` (e.g. `"fb0"`). `size` is the framebuffer byte length.
pub fn register_display_device(device: Arc<dyn DisplayDevice>) {
    let name = String::from(device.name());
    let fb = device.framebuffer();
    let size = fb.stride as usize * fb.height as usize;
    let node: VfsNodeRef = Arc::new(DisplayNode {
        ino: alloc_ino(),
        device,
        size,
    });
    register_device(&name, node);
}

/// Register an input device. The devfs entry name is taken from
/// `device.name()` (e.g. `"keyboard0"`).
pub fn register_input_device(device: Arc<dyn InputDevice>) {
    let name = String::from(device.name());
    let node: VfsNodeRef = Arc::new(InputNode {
        ino: alloc_ino(),
        device,
    });
    register_device(&name, node);
}

/// Register an RNG device. The devfs entry name is taken from
/// `device.name()` (e.g. `"random"`).
pub fn register_rng_device(device: Arc<dyn RngDevice>) {
    let name = String::from(device.name());
    let node: VfsNodeRef = Arc::new(RngNode {
        ino: alloc_ino(),
        device,
    });
    register_device(&name, node);
}
