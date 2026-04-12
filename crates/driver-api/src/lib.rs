//! Typed driver API for Solaya.
//!
//! Trait-only crate (plus a thin `DmaBuffer` wrapper). Defines the contract
//! every concrete driver implements and every kernel subsystem consumes.
//! Contains no driver code, no device probing, and no per-device state.
//!
//! Layering invariant: may depend on `abi`, `headers`, `klib`, `hal`, `mm`.
//! May not depend on `console`, `drivers`, or `solaya` (the kernel). The `mm`
//! dependency is used only by the `dma` module to back `DmaBuffer` with the
//! global page allocator.
//!
//! Unsafe policy: every module except `dma` is `#![forbid(unsafe_code)]`.
//! `dma` carries the typed reinterpretation of raw DMA-backed memory (the
//! single unavoidable `unsafe`), contained to a handful of documented blocks
//! inside `dma.rs`. The kernel crate remains `#![forbid(unsafe_code)]` and
//! reaches DMA memory only through the safe accessors exposed here.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unsafe_code)]
#![feature(macro_metavar_expr_concat)]

extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{fmt, future::Future, pin::Pin};

pub mod bus;
pub mod catalog;
#[allow(unsafe_code)]
pub mod dma;
#[allow(unsafe_code)]
pub mod net_notifier;
pub use bus::{
    BarIndex, BusContext, DtBusContextExt, IrqId, MmioRegion, PciBusContextExt,
    PciCapabilityHeader, PciCapabilityHeaderExt,
};
pub use catalog::{DriverCatalog, DriverFactory, DriverInstance};
pub use dma::DmaBuffer;

pub use headers::errno::Errno as IoError;

/// Error returned by a driver's `probe`.
#[derive(Debug)]
pub enum ProbeError {
    /// The driver does not handle this device; try the next factory.
    DoesNotMatch,
    /// The driver matched but failed to initialize.
    InitializationFailed(&'static str),
}

/// Error returned by bus-level operations (MMIO mapping, DMA allocation,
/// IRQ registration).
#[derive(Debug)]
pub enum BusError {
    NoSuchBar,
    MmioMapFailed,
    OutOfMemory,
    IrqUnavailable,
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

/// Character device — byte-stream read/write (serial, TTY, pipe-like devices).
///
/// `read` and `write` take `&self`; implementors serialise access internally
/// (typically via a `Spinlock`). The contract for `read` matches existing
/// console behavior: return `Err(Errno::EAGAIN)` when no bytes are available
/// and the caller should retry.
pub trait CharDevice: Send + Sync {
    /// Stable short name, e.g. `"console"`.
    fn name(&self) -> &str;

    /// Read up to `buf.len()` bytes. Returns the number of bytes read, or
    /// `Err(EAGAIN)` if no data is currently available.
    fn read(&self, buf: &mut [u8]) -> Result<usize, IoError>;

    /// Write `data`. Returns the number of bytes written (typically
    /// `data.len()` for synchronous console-style devices).
    fn write(&self, data: &[u8]) -> Result<usize, IoError>;
}

/// Static framebuffer description, returned by `DisplayDevice::framebuffer`.
///
/// `phys_addr` is the CPU-visible address of the framebuffer — today the
/// identity-mapped MMIO base of the display controller. `stride` is in bytes.
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bpp: u8,
    pub phys_addr: u64,
}

/// Linear-framebuffer display device.
///
/// Minimal surface — enough for devfs mmap/read/write. `flush` is deferred
/// until a real compositor consumer exists.
pub trait DisplayDevice: Send + Sync {
    /// Stable short name, e.g. `"fb0"`.
    fn name(&self) -> &str;

    /// Framebuffer geometry + physical base address.
    fn framebuffer(&self) -> FramebufferInfo;

    /// Read `buf.len()` bytes from the framebuffer starting at `offset`.
    /// Returns the number of bytes copied (may short-read at end).
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, IoError>;

    /// Write `data` to the framebuffer starting at `offset`.
    /// Returns the number of bytes written.
    fn write_at(&self, offset: usize, data: &[u8]) -> Result<usize, IoError>;
}

/// One input event, raw virtio-input layout (type, code, value).
///
/// Matches the wire format on the virtio event queue so devfs consumers can
/// pass the bytes through unchanged. If/when evdev-compatible events become
/// necessary, this type grows to match `struct input_event` from UAPI.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InputEvent {
    pub event_type: u16,
    pub code: u16,
    pub value: u32,
}

/// Input device (keyboard, mouse, virtio-input).
///
/// Events are buffered by the driver; `poll_event` pops one event or returns
/// `None` if the queue is empty.
pub trait InputDevice: Send + Sync {
    /// Stable short name, e.g. `"keyboard0"`.
    fn name(&self) -> &str;

    /// Pop one buffered event, or `None` if the queue is empty.
    fn poll_event(&self) -> Option<InputEvent>;
}

/// Hardware random number generator.
pub trait RngDevice: Send + Sync {
    /// Stable short name, e.g. `"random"`.
    fn name(&self) -> &str;

    /// Fill `buf` with random bytes. Returns the number of bytes written
    /// (typically `buf.len()`).
    fn fill(&self, buf: &mut [u8]) -> Result<usize, IoError>;
}

/// Interrupt handler — invoked from the trap handler when the driver's IRQ
/// fires.
///
/// Implementations must be short and non-blocking: the typical body reads the
/// device's ISR register to acknowledge the interrupt, then wakes a bottom-half
/// async task via a stored `Waker`. Do not allocate, do not take locks held
/// elsewhere across a long critical section, do not sleep.
///
/// The IRQ controller holds an `Arc<dyn IrqHandler>` and calls `handle()`
/// through the trait object from the trap handler. Drivers therefore typically
/// store their MMIO / wake state inside the implementor itself, not in a
/// module-local static.
pub trait IrqHandler: Send + Sync {
    /// Called in interrupt context. Acknowledge the device, wake the
    /// bottom-half task, return.
    fn handle(&self);
}

/// IRQ-controller backend (e.g. PLIC) that tears down a registration when the
/// driver drops its [`IrqRegistration`] token.
///
/// Drivers never implement this trait — the kernel's interrupt controller
/// does. It exists only so [`IrqRegistration::drop`] can call back into the
/// concrete controller without `driver-api` depending on any kernel type.
///
/// `unregister` receives the opaque `slot` that the controller handed out when
/// the registration was created; the controller uses it to locate and remove
/// the handler entry.
pub trait IrqController: Send + Sync {
    /// Remove the handler associated with `slot`. Idempotent — calling twice
    /// with the same slot is a no-op.
    fn unregister(&self, slot: u64);
}

/// RAII guard handed back by a bus's `register_irq` call. Dropping it removes
/// the handler from the underlying IRQ controller (disabling the IRQ line if
/// no other handlers remain).
///
/// Drivers store this token inside their handle struct so interrupt teardown
/// is automatic when the driver is dropped.
#[must_use = "dropping this immediately unregisters the IRQ handler"]
pub struct IrqRegistration {
    controller: Arc<dyn IrqController>,
    slot: u64,
}

impl IrqRegistration {
    /// Construct an `IrqRegistration` from a controller + opaque slot. Only
    /// IRQ-controller implementations (in the kernel) call this.
    pub fn new(controller: Arc<dyn IrqController>, slot: u64) -> Self {
        Self { controller, slot }
    }
}

impl Drop for IrqRegistration {
    fn drop(&mut self) {
        self.controller.unregister(self.slot);
    }
}
