//! `BusContext` implementation that wraps a device-tree node's resources
//! (MMIO region from its `reg` property, IRQ from its `interrupts` property).
//!
//! DT-bound drivers (today: dwmac) receive a `DtBusContext` instead of a
//! `PciBusContext` so they can allocate DMA and register IRQs through the
//! same trait that PCI drivers use.

use alloc::sync::Arc;
use driver_api::{
    BusContext, BusError, DmaBuffer, DtBusContextExt, IrqHandler, IrqId, IrqRegistration,
    PciBusContextExt,
};

use crate::interrupts::plic;

pub struct DtBusContext {
    reg_base: usize,
    reg_size: usize,
}

impl DtBusContext {
    pub fn new(reg_base: usize, reg_size: usize) -> Self {
        Self { reg_base, reg_size }
    }
}

impl BusContext for DtBusContext {
    fn dma_alloc_coherent(&self, len: usize) -> Result<DmaBuffer, BusError> {
        DmaBuffer::new_coherent(len)
    }

    fn register_irq(
        &self,
        irq: IrqId,
        handler: Arc<dyn IrqHandler>,
    ) -> Result<IrqRegistration, BusError> {
        Ok(plic::register(irq.0, handler))
    }

    fn as_pci(&self) -> Option<&dyn PciBusContextExt> {
        None
    }

    fn as_dt(&self) -> Option<&dyn DtBusContextExt> {
        Some(self)
    }
}

impl DtBusContextExt for DtBusContext {
    fn reg_base(&self) -> usize {
        self.reg_base
    }

    fn reg_size(&self) -> usize {
        self.reg_size
    }
}
