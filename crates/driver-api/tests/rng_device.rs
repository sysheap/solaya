//! Trait-level sanity test for `RngDevice::fill` through a trait object.

extern crate alloc;

use alloc::{sync::Arc, vec};
use std::sync::Mutex;

use driver_api::{IoError, RngDevice};

struct MockRng {
    name: alloc::string::String,
    counter: Mutex<u8>,
}

impl MockRng {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            counter: Mutex::new(0),
        }
    }
}

impl RngDevice for MockRng {
    fn name(&self) -> &str {
        &self.name
    }

    fn fill(&self, buf: &mut [u8]) -> Result<usize, IoError> {
        let mut c = self.counter.lock().expect("mock not poisoned");
        for slot in buf.iter_mut() {
            *slot = *c;
            *c = c.wrapping_add(1);
        }
        Ok(buf.len())
    }
}

#[test]
fn trait_object_fill() {
    let dev: Arc<dyn RngDevice> = Arc::new(MockRng::new("rng0"));
    assert_eq!(dev.name(), "rng0");

    let mut buf = vec![0u8; 4];
    let n = dev.fill(&mut buf).expect("fill");
    assert_eq!(n, 4);
    assert_eq!(buf, vec![0, 1, 2, 3]);

    let mut buf = vec![0u8; 2];
    let n = dev.fill(&mut buf).expect("fill");
    assert_eq!(n, 2);
    assert_eq!(buf, vec![4, 5]);
}
