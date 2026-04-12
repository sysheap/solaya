//! Trait-level sanity test for `DisplayDevice`. Proves the trait is
//! object-safe and the `FramebufferInfo` surface is reachable through
//! `Arc<dyn DisplayDevice>`.

extern crate alloc;

use alloc::{sync::Arc, vec};
use std::sync::Mutex;

use driver_api::{DisplayDevice, FramebufferInfo, IoError};

struct MockDisplay {
    name: alloc::string::String,
    info: FramebufferInfo,
    storage: Mutex<alloc::vec::Vec<u8>>,
}

impl MockDisplay {
    fn new(name: &str, width: u32, height: u32, bpp: u8) -> Self {
        let stride = width * u32::from(bpp) / 8;
        let size = (stride * height) as usize;
        Self {
            name: name.into(),
            info: FramebufferInfo {
                width,
                height,
                stride,
                bpp,
                phys_addr: 0,
            },
            storage: Mutex::new(vec![0u8; size]),
        }
    }
}

impl DisplayDevice for MockDisplay {
    fn name(&self) -> &str {
        &self.name
    }

    fn framebuffer(&self) -> FramebufferInfo {
        self.info
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, IoError> {
        let storage = self.storage.lock().expect("mock not poisoned");
        if offset >= storage.len() {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len(), storage.len() - offset);
        buf[..n].copy_from_slice(&storage[offset..offset + n]);
        Ok(n)
    }

    fn write_at(&self, offset: usize, data: &[u8]) -> Result<usize, IoError> {
        let mut storage = self.storage.lock().expect("mock not poisoned");
        if offset >= storage.len() {
            return Ok(0);
        }
        let n = core::cmp::min(data.len(), storage.len() - offset);
        storage[offset..offset + n].copy_from_slice(&data[..n]);
        Ok(n)
    }
}

#[test]
fn trait_object_framebuffer_info() {
    let dev: Arc<dyn DisplayDevice> = Arc::new(MockDisplay::new("mock-fb", 640, 480, 32));
    let fb = dev.framebuffer();
    assert_eq!(fb.width, 640);
    assert_eq!(fb.height, 480);
    assert_eq!(fb.stride, 640 * 4);
    assert_eq!(fb.bpp, 32);
    assert_eq!(dev.name(), "mock-fb");
}

#[test]
fn trait_object_read_write_round_trip() {
    let dev: Arc<dyn DisplayDevice> = Arc::new(MockDisplay::new("mock-fb", 4, 1, 8));
    let n = dev.write_at(0, &[1, 2, 3, 4]).expect("write");
    assert_eq!(n, 4);
    let mut buf = [0u8; 4];
    let n = dev.read_at(0, &mut buf).expect("read");
    assert_eq!(n, 4);
    assert_eq!(buf, [1, 2, 3, 4]);
}
