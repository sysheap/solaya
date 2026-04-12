//! Trait-level sanity test for `InputDevice` polling through a trait object.

extern crate alloc;

use alloc::sync::Arc;
use std::sync::Mutex;

use driver_api::{InputDevice, InputEvent};

struct MockInput {
    name: alloc::string::String,
    queue: Mutex<alloc::collections::VecDeque<InputEvent>>,
}

impl MockInput {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            queue: Mutex::new(alloc::collections::VecDeque::new()),
        }
    }

    fn push(&self, event: InputEvent) {
        self.queue
            .lock()
            .expect("mock not poisoned")
            .push_back(event);
    }
}

impl InputDevice for MockInput {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll_event(&self) -> Option<InputEvent> {
        self.queue.lock().expect("mock not poisoned").pop_front()
    }
}

#[test]
fn trait_object_poll_events() {
    let dev_raw = Arc::new(MockInput::new("kbd0"));
    dev_raw.push(InputEvent {
        event_type: 1,
        code: 30,
        value: 1,
    });
    dev_raw.push(InputEvent {
        event_type: 1,
        code: 30,
        value: 0,
    });

    let dev: Arc<dyn InputDevice> = dev_raw;
    assert_eq!(dev.name(), "kbd0");

    let ev = dev.poll_event().expect("first event");
    assert_eq!(ev.event_type, 1);
    assert_eq!(ev.code, 30);
    assert_eq!(ev.value, 1);

    let ev = dev.poll_event().expect("second event");
    assert_eq!(ev.value, 0);

    assert!(dev.poll_event().is_none());
}
