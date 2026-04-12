//! Trait-level sanity test for `BusContext` and its extension traits.
//!
//! Builds a mock PCI-backed `BusContext` entirely on the host, counts each
//! trait method call, and verifies that `&dyn BusContext` is object-safe
//! and that `as_pci()` reaches the PCI-specific surface.

extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

use driver_api::{
    BarIndex, BusContext, BusError, DmaBuffer, IrqController, IrqHandler, IrqId, IrqRegistration,
    MmioRegion, PciBusContextExt, PciCapabilityHeader,
};
use hal::mmio::MMIO;

#[derive(Default)]
struct Counts {
    dma: AtomicUsize,
    irq: AtomicUsize,
    caps: AtomicUsize,
    map_bar: AtomicUsize,
    set_cmd: AtomicUsize,
    clear_cmd: AtomicUsize,
    read_cfg: AtomicUsize,
}

struct NoopController;
impl IrqController for NoopController {
    fn unregister(&self, _slot: u64) {}
}

struct MockPciBus {
    counts: Counts,
    // Backing storage for MMIO regions (kept alive for the test's lifetime).
    fake_bar: Vec<u8>,
    fake_caps: Vec<u8>,
}

impl MockPciBus {
    fn new() -> Self {
        Self {
            counts: Counts::default(),
            fake_bar: vec![0u8; 4096],
            fake_caps: vec![0u8; 64],
        }
    }
}

impl BusContext for MockPciBus {
    fn dma_alloc_coherent(&self, len: usize) -> Result<DmaBuffer, BusError> {
        self.counts.dma.fetch_add(1, Ordering::SeqCst);
        DmaBuffer::new_coherent(len)
    }

    fn register_irq(
        &self,
        _irq: IrqId,
        _handler: Arc<dyn IrqHandler>,
    ) -> Result<IrqRegistration, BusError> {
        self.counts.irq.fetch_add(1, Ordering::SeqCst);
        Ok(IrqRegistration::new(Arc::new(NoopController), 0))
    }

    fn as_pci(&self) -> Option<&dyn PciBusContextExt> {
        Some(self)
    }
}

impl PciBusContextExt for MockPciBus {
    fn capabilities(&self) -> Box<dyn Iterator<Item = MMIO<PciCapabilityHeader>> + '_> {
        self.counts.caps.fetch_add(1, Ordering::SeqCst);
        // A single fake capability at offset 0.
        let base = self.fake_caps.as_ptr() as usize;
        Box::new(core::iter::once(MMIO::new(base)))
    }

    fn map_bar(&self, _index: BarIndex) -> Result<MmioRegion, BusError> {
        self.counts.map_bar.fetch_add(1, Ordering::SeqCst);
        Ok(MmioRegion {
            virt_base: self.fake_bar.as_ptr() as usize,
            len: self.fake_bar.len(),
        })
    }

    fn set_command_bits(&self, _bits: u16) {
        self.counts.set_cmd.fetch_add(1, Ordering::SeqCst);
    }

    fn clear_command_bits(&self, _bits: u16) {
        self.counts.clear_cmd.fetch_add(1, Ordering::SeqCst);
    }

    fn read_config_u32(&self, _offset: u16) -> u32 {
        self.counts.read_cfg.fetch_add(1, Ordering::SeqCst);
        0xDEAD_BEEF
    }
}

#[test]
fn bus_context_is_object_safe() {
    let bus = MockPciBus::new();
    let dyn_bus: &dyn BusContext = &bus;

    let _buf = dyn_bus.dma_alloc_coherent(4096).expect("dma alloc");
    let handler: Arc<dyn IrqHandler> = Arc::new(NoopHandler);
    let _reg = dyn_bus.register_irq(IrqId(42), handler).expect("irq");

    let pci = dyn_bus.as_pci().expect("mock is PCI-backed");
    let _caps: Vec<_> = pci.capabilities().collect();
    let _bar = pci.map_bar(BarIndex(0)).expect("bar");
    pci.set_command_bits(0x04);
    pci.clear_command_bits(0x400);
    assert_eq!(pci.read_config_u32(0x00), 0xDEAD_BEEF);

    assert_eq!(bus.counts.dma.load(Ordering::SeqCst), 1);
    assert_eq!(bus.counts.irq.load(Ordering::SeqCst), 1);
    assert_eq!(bus.counts.caps.load(Ordering::SeqCst), 1);
    assert_eq!(bus.counts.map_bar.load(Ordering::SeqCst), 1);
    assert_eq!(bus.counts.set_cmd.load(Ordering::SeqCst), 1);
    assert_eq!(bus.counts.clear_cmd.load(Ordering::SeqCst), 1);
    assert_eq!(bus.counts.read_cfg.load(Ordering::SeqCst), 1);
}

#[test]
fn default_as_dt_returns_none() {
    let bus = MockPciBus::new();
    let dyn_bus: &dyn BusContext = &bus;
    assert!(dyn_bus.as_dt().is_none());
}

struct NoopHandler;
impl IrqHandler for NoopHandler {
    fn handle(&self) {}
}
