//! `BusContext` implementation that wraps a device-tree node's resources
//! (MMIO region from its `reg` property, IRQ from its `interrupts` property).
//!
//! DT-bound drivers receive a `DtBusContext` instead of a `PciBusContext`
//! so they can allocate DMA, register IRQs, and probe / parse their own DT
//! properties through the same trait that PCI drivers use.

use alloc::sync::Arc;
use driver_api::{
    BusContext, BusError, DmaBuffer, DtBusContextExt, IrqHandler, IrqId, IrqRegistration,
    PciBusContextExt,
};
use klib::big_endian::BigEndian;

use crate::{device_tree::Node, interrupts::plic};

/// `BusContext` over a single device-tree node. The node reference keeps
/// all property data reachable for the lifetime of the context so driver
/// factories can probe and parse lazily.
pub struct DtBusContext<'a> {
    node: Node<'a>,
    reg_base: usize,
    reg_size: usize,
}

impl<'a> DtBusContext<'a> {
    pub fn new(node: Node<'a>, reg_base: usize, reg_size: usize) -> Self {
        Self {
            node,
            reg_base,
            reg_size,
        }
    }
}

impl BusContext for DtBusContext<'_> {
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

impl DtBusContextExt for DtBusContext<'_> {
    fn reg_base(&self) -> usize {
        self.reg_base
    }

    fn reg_size(&self) -> usize {
        self.reg_size
    }

    fn compatible(&self) -> Option<&str> {
        self.node.get_property("compatible")?.consume_str()
    }

    fn first_interrupt(&self) -> Option<u32> {
        self.node
            .get_property("interrupts")?
            .consume_sized_type::<BigEndian<u32>>()
            .map(|be| be.get())
    }

    fn property_bytes(&self, name: &str) -> Option<&[u8]> {
        Some(self.node.get_property(name)?.buffer())
    }
}
