use crate::{
    device_tree, info,
    klibc::{MMIO, Spinlock, runtime_initialized::RuntimeInitializedData},
};
use alloc::vec::Vec;
use arch::CpuId;

pub static PLIC_BASE: RuntimeInitializedData<usize> = RuntimeInitializedData::new();
pub static PLIC_SIZE: RuntimeInitializedData<usize> = RuntimeInitializedData::new();

struct InterruptHandler {
    irq: u32,
    handler: fn(),
}

static INTERRUPT_HANDLERS: Spinlock<Vec<InterruptHandler>> = Spinlock::new(Vec::new());

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
        let mut reg = self
            .enable_register
            .add_within_region(word_offset as usize, *PLIC_SIZE / 4);
        reg |= 1 << bit;
    }

    fn set_priority(&mut self, interrupt_id: u32, priority: u32) {
        assert!(priority <= 7);
        self.priority_register_base
            .add_within_region(interrupt_id as usize, *PLIC_SIZE / 4)
            .write(priority);
    }

    fn set_threshold(&mut self, threshold: u32) {
        assert!(threshold <= 7);
        self.threshold_register.write(threshold);
    }

    pub fn claim(&mut self) -> Option<u32> {
        let irq = self.claim_complete_register.read();
        if irq == 0 { None } else { Some(irq) }
    }

    pub fn complete(&mut self, irq: u32) {
        self.claim_complete_register.write(irq);
    }
}

pub static PLIC: RuntimeInitializedData<Spinlock<Plic>> = RuntimeInitializedData::new();

pub fn discover_from_device_tree() {
    let plic_node = device_tree::THE
        .root_node()
        .find_node("plic")
        .expect("Device tree must have a plic node");
    let reg = plic_node
        .parse_reg_property()
        .expect("PLIC node must have a reg property");

    PLIC_BASE.initialize(reg.address);
    PLIC_SIZE.initialize(reg.size);

    info!("PLIC at {:#x} size {:#x}", *PLIC_BASE, *PLIC_SIZE);
}

pub fn init_plic(cpu_id: CpuId) {
    info!("Initializing PLIC");
    PLIC.initialize(Spinlock::new(Plic::new(*PLIC_BASE, cpu_id)));
    let mut plic = PLIC.lock();
    plic.set_threshold(0);
}

pub fn register_interrupt(irq: u32, handler: fn()) {
    info!("Registering PLIC interrupt (IRQ {irq})");
    let mut plic = PLIC.lock();
    plic.enable(irq);
    plic.set_priority(irq, 1);
    drop(plic);
    INTERRUPT_HANDLERS
        .lock()
        .push(InterruptHandler { irq, handler });
}

pub fn dispatch_interrupt(irq: u32) {
    let handlers = INTERRUPT_HANDLERS.lock();
    for entry in handlers.iter() {
        if entry.irq == irq {
            let handler = entry.handler;
            drop(handlers);
            handler();
            return;
        }
    }
    panic!("Unknown PLIC interrupt source ID {irq}");
}
