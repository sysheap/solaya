//! `BusContext` / `PciBusContextExt` implementation that wraps a concrete
//! `PCIDevice` plus the kernel's global PLIC + page allocator.
//!
//! Handed to driver `initialize` functions so drivers can allocate DMA
//! buffers, register IRQs, walk capabilities, and map BARs without ever
//! importing `crate::pci::*` or `crate::interrupts::plic::*`.

use alloc::{boxed::Box, sync::Arc};
use driver_api::{
    BarIndex, BusContext, BusError, DmaBuffer, DtBusContextExt, IrqHandler, IrqId, IrqRegistration,
    MmioRegion, PciBusContextExt, PciCapabilityHeader,
};
use hal::mmio::MMIO;

use crate::{
    interrupts::plic,
    pci::{GeneralDevicePciHeaderExt, PCIDevice},
};
use hal::spinlock::Spinlock;

/// Wraps a `PCIDevice` behind a `Spinlock` so `BusContext`'s `&self` methods
/// can mutate the device (BAR init and command-register writes both take
/// `&mut` on the underlying MMIO).
pub struct PciBusContext<'a> {
    device: Spinlock<&'a mut PCIDevice>,
}

impl<'a> PciBusContext<'a> {
    pub fn new(device: &'a mut PCIDevice) -> Self {
        Self {
            device: Spinlock::new(device),
        }
    }
}

impl BusContext for PciBusContext<'_> {
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
        Some(self)
    }

    fn as_dt(&self) -> Option<&dyn DtBusContextExt> {
        None
    }
}

impl PciBusContextExt for PciBusContext<'_> {
    fn capabilities(&self) -> Box<dyn Iterator<Item = MMIO<PciCapabilityHeader>> + '_> {
        // The PCI capability list is a singly-linked chain through config
        // space. Walk it eagerly into a Vec so we can drop the device lock
        // before returning, then hand the addresses back as typed MMIO.
        let offsets: alloc::vec::Vec<usize> = {
            let dev = self.device.lock();
            dev.capabilities()
                .map(|cap| {
                    // `MMIO::addr()` returns the raw CPU-visible base.
                    cap.addr()
                })
                .collect()
        };
        Box::new(offsets.into_iter().map(MMIO::new))
    }

    fn map_bar(&self, index: BarIndex) -> Result<MmioRegion, BusError> {
        let space = self.device.lock().get_or_initialize_bar(index.0);
        Ok(MmioRegion {
            virt_base: space.cpu_address.as_usize(),
            len: space.size,
        })
    }

    fn set_command_bits(&self, bits: u16) {
        self.device
            .lock()
            .configuration_space_mut()
            .set_command_register_bits(bits);
    }

    fn clear_command_bits(&self, bits: u16) {
        self.device
            .lock()
            .configuration_space_mut()
            .clear_command_register_bits(bits);
    }

    fn read_config_u32(&self, byte_offset: u16) -> u32 {
        let base = self.device.lock().configuration_space().addr();
        MMIO::<u32>::new(base + byte_offset as usize).read()
    }

    fn plic_irq(&self) -> IrqId {
        IrqId(self.device.lock().plic_interrupt_id())
    }
}
