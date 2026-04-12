//! Bus-abstraction traits that hide the concrete bus (PCI, device tree, etc.)
//! from driver code.
//!
//! [`BusContext`] is the shared surface every driver sees: DMA allocation and
//! IRQ registration. The PCI-specific operations (capability walking, BAR
//! mapping, command-register bits) live on [`PciBusContextExt`], exposed via
//! `BusContext::as_pci()`. The DT-specific operations (reg-property probing)
//! live on [`DtBusContextExt`], exposed via `BusContext::as_dt()`.
//!
//! Drivers genuinely bound to one bus (virtio-* drivers are PCI-only) take
//! `&dyn BusContext` and call `as_pci().expect("pci-only driver")` at the top
//! of `initialize`. Drivers that work on either bus (there are none today)
//! would consult only [`BusContext`]. This keeps the common surface tiny
//! while giving PCI/DT drivers the access they need.
//!
//! Moving this machinery into `driver-api` lets concrete drivers live
//! outside the kernel crate (Phase 7) without reaching into `crate::pci::*`,
//! `crate::device_tree::*`, or `crate::interrupts::plic::*`.

use alloc::{boxed::Box, sync::Arc};
use hal::mmio::MMIO;

use crate::{BusError, DmaBuffer, IrqHandler, IrqRegistration};

/// BAR index within a PCI device's configuration space (0..=5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarIndex(pub u8);

/// IRQ number as understood by the IRQ controller (for the PLIC on RISC-V,
/// this is the global source ID that `plic::register` consumes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqId(pub u32);

/// A CPU-visible MMIO region returned from a bus-mapping call. `virt_base` is
/// a valid usize suitable for `MMIO::<T>::new`; `len` is the mapped length in
/// bytes.
#[derive(Debug, Clone, Copy)]
pub struct MmioRegion {
    pub virt_base: usize,
    pub len: usize,
}

impl MmioRegion {
    /// Re-interpret this region as a typed MMIO handle at `byte_offset`.
    /// The caller is responsible for checking that `byte_offset +
    /// size_of::<T>() <= self.len`.
    pub fn typed_at<T>(&self, byte_offset: usize) -> MMIO<T> {
        assert!(
            byte_offset + core::mem::size_of::<T>() <= self.len,
            "MmioRegion::typed_at out of bounds"
        );
        MMIO::new(self.virt_base + byte_offset)
    }
}

/// Two-field header common to every PCI capability. Drivers receive an
/// `MMIO<PciCapabilityHeader>` from [`PciBusContextExt::capabilities`] and
/// reinterpret it as their capability type via [`PciCapabilityHeaderExt::as_type`].
#[repr(C, packed)]
pub struct PciCapabilityHeader {
    pub id: u8,
    pub next: u8,
}

/// Field-access helpers for `MMIO<PciCapabilityHeader>`. Lives in driver-api
/// so drivers don't need the kernel's `mmio_struct!` macro.
pub trait PciCapabilityHeaderExt {
    /// Capability ID (vendor-specific, MSI, power-management, ...).
    fn id(&self) -> u8;
    /// Pointer to the next capability (unused by drivers — the bus walks
    /// the list — but exposed for completeness).
    fn next_offset(&self) -> u8;
    /// Reinterpret the capability at its base address as a driver-defined
    /// type `T`. Useful for vendor-specific capability layouts that extend
    /// past the two-byte header.
    fn as_type<T>(&self) -> MMIO<T>;
}

impl PciCapabilityHeaderExt for MMIO<PciCapabilityHeader> {
    fn id(&self) -> u8 {
        MMIO::<u8>::new(self.addr()).read()
    }

    fn next_offset(&self) -> u8 {
        MMIO::<u8>::new(self.addr() + 1).read()
    }

    fn as_type<T>(&self) -> MMIO<T> {
        MMIO::new(self.addr())
    }
}

/// Bus-agnostic surface: operations that every bus implements the same way.
/// Drivers that must reach bus-specific state go through [`Self::as_pci`] or
/// [`Self::as_dt`].
pub trait BusContext: Send + Sync {
    /// Allocate a coherent DMA buffer of `len` bytes. Backed by the global
    /// page allocator on every bus today.
    fn dma_alloc_coherent(&self, len: usize) -> Result<DmaBuffer, BusError>;

    /// Register `handler` for the IRQ that this bus context describes.
    /// Returns the RAII registration token — drop it to unregister.
    fn register_irq(
        &self,
        irq: IrqId,
        handler: Arc<dyn IrqHandler>,
    ) -> Result<IrqRegistration, BusError>;

    /// PCI-specific view, if this context wraps a PCI device. Default `None`.
    fn as_pci(&self) -> Option<&dyn PciBusContextExt> {
        None
    }

    /// Device-tree view, if this context wraps a DT node. Default `None`.
    fn as_dt(&self) -> Option<&dyn DtBusContextExt> {
        None
    }
}

/// Operations a PCI device driver needs on its bus: capability walking, BAR
/// mapping, command-register manipulation, and raw config-space reads.
pub trait PciBusContextExt {
    /// Enumerate the PCI capability list. Each item is an MMIO handle at a
    /// capability's [`PciCapabilityHeader`]; reinterpret to the
    /// driver-specific layout with `MMIO::new_type::<T>()`.
    fn capabilities(&self) -> Box<dyn Iterator<Item = MMIO<PciCapabilityHeader>> + '_>;

    /// Map BAR `index`, returning its CPU-visible MMIO region. Drivers add
    /// per-capability `offset` values themselves via [`MmioRegion::typed_at`].
    fn map_bar(&self, index: BarIndex) -> Result<MmioRegion, BusError>;

    /// Set the given bits in the command register (used to enable bus-master
    /// DMA, MMIO space, etc.).
    fn set_command_bits(&self, bits: u16);

    /// Clear the given bits in the command register (used to clear the
    /// interrupt-disable bit on legacy INTx lines, etc.).
    fn clear_command_bits(&self, bits: u16);

    /// Read a raw `u32` from PCI configuration space at `byte_offset`.
    /// Returns 0 if the offset is out of range.
    fn read_config_u32(&self, byte_offset: u16) -> u32;
}

/// Operations a device-tree-bound driver needs on its bus: the base address
/// and size from the `reg` property.
pub trait DtBusContextExt {
    /// Base address from the node's `reg` property.
    fn reg_base(&self) -> usize;

    /// Size of the region described by the `reg` property.
    fn reg_size(&self) -> usize;
}

/// Command-register bit constants, shared between `PciBusContextExt`
/// implementors and driver code that asks the bus to toggle them.
pub mod pci_command {
    pub const IO_SPACE: u16 = 1 << 0;
    pub const MEMORY_SPACE: u16 = 1 << 1;
    pub const BUS_MASTER: u16 = 1 << 2;
    pub const INTERRUPT_DISABLE: u16 = 1 << 10;
}
