//! Trait-level sanity test for `CharDevice`. Proves the trait is
//! object-safe and exercises read/write through `Arc<dyn CharDevice>`.

extern crate alloc;

use alloc::{sync::Arc, vec};
use std::sync::Mutex;

use driver_api::{CharDevice, IoError};
use headers::errno::Errno;

struct MockChar {
    name: alloc::string::String,
    rx: Mutex<alloc::collections::VecDeque<u8>>,
    tx: Mutex<alloc::vec::Vec<u8>>,
}

impl MockChar {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            rx: Mutex::new(alloc::collections::VecDeque::new()),
            tx: Mutex::new(alloc::vec::Vec::new()),
        }
    }

    fn queue_rx(&self, data: &[u8]) {
        self.rx.lock().expect("mock not poisoned").extend(data);
    }
}

impl CharDevice for MockChar {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
        let mut rx = self.rx.lock().expect("mock not poisoned");
        if rx.is_empty() {
            return Err(Errno::EAGAIN);
        }
        let n = core::cmp::min(buf.len(), rx.len());
        for slot in buf.iter_mut().take(n) {
            *slot = rx.pop_front().expect("checked not empty");
        }
        Ok(n)
    }

    fn write(&self, data: &[u8]) -> Result<usize, IoError> {
        self.tx
            .lock()
            .expect("mock not poisoned")
            .extend_from_slice(data);
        Ok(data.len())
    }
}

#[test]
fn trait_object_round_trip() {
    let dev_raw = Arc::new(MockChar::new("mock-tty"));
    dev_raw.queue_rx(b"hi");
    let dev: Arc<dyn CharDevice> = dev_raw.clone();

    assert_eq!(dev.name(), "mock-tty");

    let mut buf = vec![0u8; 4];
    let n = dev.read(&mut buf).expect("read");
    assert_eq!(n, 2);
    assert_eq!(&buf[..2], b"hi");

    let n = dev.write(b"out").expect("write");
    assert_eq!(n, 3);
    assert_eq!(
        dev_raw.tx.lock().expect("mock not poisoned").as_slice(),
        b"out"
    );
}

#[test]
fn read_with_no_data_returns_eagain() {
    let dev: Arc<dyn CharDevice> = Arc::new(MockChar::new("empty"));
    let mut buf = vec![0u8; 4];
    let err = dev.read(&mut buf).expect_err("no data queued");
    assert_eq!(err, Errno::EAGAIN);
}
