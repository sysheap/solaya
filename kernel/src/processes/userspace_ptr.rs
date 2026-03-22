use common::pointer::Pointer;
use headers::errno::Errno;

use crate::{klibc::SpinlockGuard, processes::process::Process};
use sys::klibc::send_sync::UnsafeSendSync;

#[derive(Debug)]
pub struct UserspacePtr<PTR: Pointer> {
    ptr: UnsafeSendSync<PTR>,
}

impl<PTR: Pointer> UserspacePtr<PTR> {
    pub fn new(ptr: PTR) -> Self {
        Self {
            ptr: UnsafeSendSync(ptr),
        }
    }

    pub fn get(&self) -> PTR {
        *self.ptr
    }
}

impl<T> UserspacePtr<*mut T> {
    pub fn write_with_process_lock(
        &self,
        process_lock: &SpinlockGuard<'_, Process>,
        value: T,
    ) -> Result<(), Errno> {
        process_lock.write_userspace_ptr(self, value)
    }
}

pub struct ContainsUserspacePtr<T>(pub UnsafeSendSync<T>);

impl<T: core::fmt::Debug> core::fmt::Debug for ContainsUserspacePtr<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}
