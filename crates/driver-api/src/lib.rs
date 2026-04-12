//! Typed driver API for Solaya.
//!
//! Trait-only crate: defines the contract every concrete driver implements and
//! every kernel subsystem consumes. Contains no driver code, no device
//! probing, and no state.
//!
//! Layering invariant: may depend on `abi`, `headers`, `klib`, `hal`. May not
//! depend on `console`, `mm`, `drivers`, or `solaya` (the kernel). `mm` is a
//! planned dependency (DMA types in Phase 5) but is not needed yet, and adding
//! it today forces host tests through a riscv64-only crate.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use core::{future::Future, pin::Pin};

pub use headers::errno::Errno as IoError;

/// Error returned by a driver's `probe`.
#[derive(Debug)]
pub enum ProbeError {
    /// The driver does not handle this device; try the next factory.
    DoesNotMatch,
    /// The driver matched but failed to initialize.
    InitializationFailed(&'static str),
}

/// Block storage device (disk, partition, virtio-blk, etc.).
///
/// `offset_bytes` is a byte offset from the start of the device. The trait
/// object is `Send + Sync` so it can be stored in registries and shared
/// between async tasks.
pub trait BlockDevice: Send + Sync {
    /// Stable short name, e.g. `"vda"`. Used for devfs entry names.
    fn name(&self) -> &str;

    /// Total number of blocks on the device.
    fn num_blocks(&self) -> u64;

    /// Size of one block in bytes.
    fn block_size(&self) -> usize;

    /// Read bytes starting at `offset_bytes` into `buf`.
    ///
    /// Returns the number of bytes read. May short-read at end-of-device.
    fn read<'a>(
        &'a self,
        offset_bytes: u64,
        buf: &'a mut [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, IoError>> + Send + 'a>>;

    /// Write bytes from `data` starting at `offset_bytes`.
    ///
    /// Returns the number of bytes written.
    fn write<'a>(
        &'a self,
        offset_bytes: u64,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, IoError>> + Send + 'a>>;
}
