use crate::processes::{process::ProcessRef, userspace_ptr::UserspacePtr};
use alloc::vec::Vec;
use common::pointer::Pointer;
use core::marker::PhantomData;
use headers::errno::Errno;

#[derive(Clone)]
pub struct LinuxUserspaceArg<T> {
    arg: usize,
    process: ProcessRef,
    // fn() -> T is always Send, unlike PhantomData<T> which inherits T's bounds.
    // LinuxUserspaceArg stores only a usize, not an actual T.
    phantom: PhantomData<fn() -> T>,
}

impl<T> LinuxUserspaceArg<T> {
    pub fn new(arg: usize, process: ProcessRef) -> Self {
        Self {
            arg,
            process,
            phantom: PhantomData,
        }
    }

    pub fn arg_nonzero(&self) -> bool {
        self.arg != 0
    }

    pub fn raw_arg(&self) -> usize {
        self.arg
    }
}

impl<T: Copy> LinuxUserspaceArg<*const T> {
    pub fn validate_ptr(&self) -> Result<T, Errno> {
        self.process
            .with_lock(|p| p.read_userspace_ptr(&self.into()))
    }
}

impl<T: Copy> LinuxUserspaceArg<Option<*const T>> {
    pub fn validate_ptr(&self) -> Result<Option<T>, Errno> {
        if self.arg == 0 {
            return Ok(None);
        }
        self.process
            .with_lock(|p| p.read_userspace_ptr(&self.into()))
            .map(|r| Some(r))
    }
}

impl<T: Copy> LinuxUserspaceArg<*const T> {
    pub fn validate_slice(&self, len: usize) -> Result<Vec<T>, Errno> {
        self.process
            .with_lock(|p| p.read_userspace_slice(&self.into(), len))
    }
}

impl<T: Copy> LinuxUserspaceArg<*mut T> {
    pub fn validate_slice(&self, len: usize) -> Result<Vec<T>, Errno> {
        self.process
            .with_lock(|p| p.read_userspace_slice(&self.into(), len))
    }
    pub fn write_slice(&self, values: &[T]) -> Result<(), Errno> {
        self.process
            .with_lock(|p| p.write_userspace_slice(&self.into(), values))?;
        Ok(())
    }
}

impl<T: Clone> LinuxUserspaceArg<Option<*mut T>> {
    pub fn write_if_not_none(&self, value: T) -> Result<Option<()>, Errno> {
        if self.arg == 0 {
            return Ok(None);
        }
        self.process
            .with_lock(|p| p.write_userspace_ptr(&self.into(), value))?;
        Ok(Some(()))
    }
}

impl<PTR: Pointer> From<&LinuxUserspaceArg<PTR>> for UserspacePtr<PTR> {
    fn from(value: &LinuxUserspaceArg<PTR>) -> Self {
        Self::new(PTR::as_pointer(value.arg))
    }
}

impl<PTR: Pointer> From<&LinuxUserspaceArg<Option<PTR>>> for UserspacePtr<PTR> {
    fn from(value: &LinuxUserspaceArg<Option<PTR>>) -> Self {
        Self::new(PTR::as_pointer(value.arg))
    }
}

impl<T> From<&LinuxUserspaceArg<*mut T>> for UserspacePtr<*const T> {
    fn from(value: &LinuxUserspaceArg<*mut T>) -> Self {
        Self::new(value.arg as *const T)
    }
}
