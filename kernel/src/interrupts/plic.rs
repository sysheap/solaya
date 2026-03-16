#![allow(unsafe_code)]
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{
    info,
    klibc::{MMIO, Spinlock, runtime_initialized::RuntimeInitializedData},
};
use sys::CpuId;

pub const PLIC_BASE: usize = 0x0c00_0000;
pub const PLIC_SIZE: usize = 0x1000_0000;

pub struct Plic {
    priority_register_base: MMIO<u32>,
    enable_register: MMIO<u32>,
    threshold_register: MMIO<u32>,
    claim_complete_register: MMIO<u32>,
}

impl Plic {
    fn new(plic_base: usize, cpu_id: CpuId) -> Self {
        let context = cpu_id.as_usize() * 2 + 1;
        Self {
            priority_register_base: MMIO::new(plic_base),
            enable_register: MMIO::new(plic_base + 0x2000 + (0x80 * context)),
            threshold_register: MMIO::new(plic_base + 0x20_0000 + (0x1000 * context)),
            claim_complete_register: MMIO::new(plic_base + 0x20_0004 + (0x1000 * context)),
        }
    }
    fn enable(&mut self, interrupt_id: u32) {
        let word_offset = interrupt_id / 32;
        let bit = interrupt_id % 32;
        // SAFETY: Each 32-bit word in the enable register array covers 32
        // interrupt IDs. word_offset selects the correct word within the
        // PLIC enable register region.
        unsafe {
            let mut reg = self.enable_register.add(word_offset as usize);
            reg |= 1 << bit;
        }
    }

    fn set_priority(&mut self, interrupt_id: u32, priority: u32) {
        assert!(priority <= 7);
        // SAFETY: Each interrupt source has a 4-byte priority register at
        // base + 4*interrupt_id, which is within the PLIC MMIO region.
        unsafe {
            self.priority_register_base
                .add(interrupt_id as usize)
                .write(priority);
        }
    }

    fn set_threshold(&mut self, threshold: u32) {
        assert!(threshold <= 7);
        self.threshold_register.write(threshold);
    }

    pub fn get_next_pending(&mut self) -> Option<InterruptSource> {
        let open_interrupt = self.claim_complete_register.read();

        match open_interrupt {
            0 => None,
            UART_INTERRUPT_NUMBER => Some(InterruptSource::Uart),
            #[cfg(feature = "virtio-net")]
            id if id == VIRTIO_NET_IRQ.load(Ordering::Relaxed) => Some(InterruptSource::VirtioNet),
            #[cfg(feature = "virtio-blk")]
            id if VIRTIO_BLK_IRQS.lock().contains(&id) => Some(InterruptSource::VirtioBlock(id)),
            id if id == VIRTIO_INPUT_IRQ.load(Ordering::Relaxed) => {
                Some(InterruptSource::VirtioInput)
            }
            id => panic!("Unknown PLIC interrupt source ID {id}"),
        }
    }

    pub fn complete_interrupt(&mut self, source: InterruptSource) {
        let interrupt_id = match source {
            InterruptSource::Uart => UART_INTERRUPT_NUMBER,
            #[cfg(feature = "virtio-net")]
            InterruptSource::VirtioNet => VIRTIO_NET_IRQ.load(Ordering::Relaxed),
            #[cfg(feature = "virtio-blk")]
            InterruptSource::VirtioBlock(irq) => irq,
            InterruptSource::VirtioInput => VIRTIO_INPUT_IRQ.load(Ordering::Relaxed),
        };
        self.claim_complete_register.write(interrupt_id);
    }
}

pub static PLIC: RuntimeInitializedData<Spinlock<Plic>> = RuntimeInitializedData::new();

const UART_INTERRUPT_NUMBER: u32 = 10;
#[cfg(feature = "virtio-net")]
static VIRTIO_NET_IRQ: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "virtio-blk")]
static VIRTIO_BLK_IRQS: Spinlock<alloc::vec::Vec<u32>> = Spinlock::new(alloc::vec::Vec::new());
static VIRTIO_INPUT_IRQ: AtomicU32 = AtomicU32::new(0);

pub enum InterruptSource {
    Uart,
    #[cfg(feature = "virtio-net")]
    VirtioNet,
    #[cfg(feature = "virtio-blk")]
    VirtioBlock(u32),
    VirtioInput,
}

pub fn init_uart_interrupt(cpu_id: CpuId) {
    info!("Initializing plic uart interrupt");

    PLIC.initialize(Spinlock::new(Plic::new(PLIC_BASE, cpu_id)));

    let mut plic = PLIC.lock();
    plic.set_threshold(0);
    plic.enable(UART_INTERRUPT_NUMBER);
    plic.set_priority(UART_INTERRUPT_NUMBER, 1);
}

#[cfg(feature = "virtio-net")]
pub fn init_virtio_net_interrupt(interrupt_id: u32) {
    info!("Initializing plic virtio net interrupt (IRQ {interrupt_id})");
    VIRTIO_NET_IRQ.store(interrupt_id, Ordering::Relaxed);
    let mut plic = PLIC.lock();
    plic.enable(interrupt_id);
    plic.set_priority(interrupt_id, 1);
}

#[cfg(feature = "virtio-blk")]
pub fn init_virtio_block_interrupt(interrupt_id: u32) {
    info!("Initializing plic virtio block interrupt (IRQ {interrupt_id})");
    VIRTIO_BLK_IRQS.lock().push(interrupt_id);
    let mut plic = PLIC.lock();
    plic.enable(interrupt_id);
    plic.set_priority(interrupt_id, 1);
}

pub fn init_virtio_input_interrupt(interrupt_id: u32) {
    info!("Initializing plic virtio input interrupt (IRQ {interrupt_id})");
    VIRTIO_INPUT_IRQ.store(interrupt_id, Ordering::Relaxed);
    let mut plic = PLIC.lock();
    plic.enable(interrupt_id);
    plic.set_priority(interrupt_id, 1);
}
