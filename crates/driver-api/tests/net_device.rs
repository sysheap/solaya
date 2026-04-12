//! Trait-level sanity test for `NetDevice` using a mock implementation.
//!
//! Runs on host x86_64 via `cargo test -p driver-api`. Proves the trait is
//! object-safe and can be stored behind `Arc<dyn NetDevice>`.

extern crate alloc;

use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use std::sync::Mutex;

use driver_api::{MacAddress, NetDevice};

struct MockNet {
    name: String,
    mac: MacAddress,
    mtu: u16,
    tx: Mutex<Vec<Vec<u8>>>,
    rx: Mutex<Vec<Vec<u8>>>,
}

impl MockNet {
    fn new(name: &str, mac: MacAddress, mtu: u16) -> Self {
        Self {
            name: name.to_string(),
            mac,
            mtu,
            tx: Mutex::new(Vec::new()),
            rx: Mutex::new(Vec::new()),
        }
    }

    fn push_rx(&self, frame: Vec<u8>) {
        self.rx.lock().expect("mock not poisoned").push(frame);
    }

    fn sent(&self) -> Vec<Vec<u8>> {
        self.tx.lock().expect("mock not poisoned").clone()
    }
}

impl NetDevice for MockNet {
    fn name(&self) -> &str {
        &self.name
    }

    fn mac(&self) -> MacAddress {
        self.mac
    }

    fn mtu(&self) -> u16 {
        self.mtu
    }

    fn send(&self, frame: Vec<u8>) {
        self.tx.lock().expect("mock not poisoned").push(frame);
    }

    fn receive(&self) -> Vec<Vec<u8>> {
        core::mem::take(&mut *self.rx.lock().expect("mock not poisoned"))
    }
}

#[test]
fn trait_object_basics() {
    let mac = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let dev: Arc<dyn NetDevice> = Arc::new(MockNet::new("mock0", mac, 1500));

    assert_eq!(dev.name(), "mock0");
    assert_eq!(dev.mac(), mac);
    assert_eq!(dev.mtu(), 1500);
}

#[test]
fn send_and_receive_batched() {
    let mac = MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
    let mock = Arc::new(MockNet::new("mock1", mac, 1500));
    let dev: Arc<dyn NetDevice> = mock.clone();

    dev.send(vec![1, 2, 3]);
    dev.send(vec![4, 5]);
    assert_eq!(mock.sent(), vec![vec![1, 2, 3], vec![4, 5]]);

    // No frames yet — receive returns empty.
    assert!(dev.receive().is_empty());

    // Push some, they come out in one batch, and the queue is drained.
    mock.push_rx(vec![10, 11]);
    mock.push_rx(vec![12]);
    let batch = dev.receive();
    assert_eq!(batch, vec![vec![10, 11], vec![12]]);
    assert!(dev.receive().is_empty());
}

#[test]
fn mac_display_formats_as_colon_hex() {
    let mac = MacAddress::new([0xde, 0xad, 0xbe, 0xef, 0x01, 0x02]);
    assert_eq!(alloc::format!("{}", mac), "de:ad:be:ef:01:02");
}
