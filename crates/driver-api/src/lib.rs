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

use alloc::{boxed::Box, vec::Vec};
use core::{fmt, future::Future, pin::Pin};

pub use headers::errno::Errno as IoError;

/// Error returned by a driver's `probe`.
#[derive(Debug)]
pub enum ProbeError {
    /// The driver does not handle this device; try the next factory.
    DoesNotMatch,
    /// The driver matched but failed to initialize.
    InitializationFailed(&'static str),
}

/// 48-bit Ethernet MAC address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    pub const fn new(address: [u8; 6]) -> Self {
        Self(address)
    }

    pub fn as_bytes(&self) -> [u8; 6] {
        self.0
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
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

/// Ethernet-style network device.
///
/// The trait is `Send + Sync`; implementors typically store the underlying
/// hardware state behind a `Spinlock` so `send`/`receive` can take `&self`
/// while mutating ring indices internally.
///
/// `receive` is batched (drains everything currently available) to match the
/// driver surface today — `network_rx_task` polls once per interrupt and
/// expects to get all pending frames in one call.
pub trait NetDevice: Send + Sync {
    /// Stable short name, e.g. `"eth0"`.
    fn name(&self) -> &str;

    /// Hardware MAC address. Stable for the lifetime of the device.
    fn mac(&self) -> MacAddress;

    /// Maximum transmission unit in bytes (payload only, not counting the
    /// Ethernet header).
    fn mtu(&self) -> u16;

    /// Enqueue one frame for transmission. Infallible — if the driver cannot
    /// accept the frame it must panic (matches today's behavior).
    fn send(&self, frame: Vec<u8>);

    /// Drain all currently available received frames.
    fn receive(&self) -> Vec<Vec<u8>>;
}
