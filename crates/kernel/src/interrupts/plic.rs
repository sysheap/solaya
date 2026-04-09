use crate::{
    device_tree, info,
    klibc::{MMIO, Spinlock, big_endian::BigEndian, runtime_initialized::RuntimeInitializedData},
};
use alloc::vec::Vec;
use hal::CpuId;

struct InterruptHandler {
    irq: u32,
    handler: fn(),
}

static INTERRUPT_HANDLERS: Spinlock<Vec<InterruptHandler>> = Spinlock::new(Vec::new());

pub struct Plic {
    base: usize,
    size: usize,
    num_sources: u32,
    priority_register_base: MMIO<u32>,
    enable_register: MMIO<u32>,
    threshold_register: MMIO<u32>,
    claim_complete_register: MMIO<u32>,
}

const S_MODE_EXTERNAL_INTERRUPT: u32 = 0x09;

impl Plic {
    fn new(base: usize, size: usize, num_sources: u32, context: usize) -> Self {
        Self {
            base,
            size,
            num_sources,
            priority_register_base: MMIO::new(base),
            enable_register: MMIO::new(base + 0x2000 + (0x80 * context)),
            threshold_register: MMIO::new(base + 0x20_0000 + (0x1000 * context)),
            claim_complete_register: MMIO::new(base + 0x20_0004 + (0x1000 * context)),
        }
    }

    pub fn base(&self) -> usize {
        self.base
    }

    pub fn size(&self) -> usize {
        self.size
    }

    fn disable_all(&mut self) {
        let num_words = self.num_sources.div_ceil(32);
        let region_elements = self.size / 4;
        for word in 0..num_words as usize {
            self.enable_register
                .add_within_region(word, region_elements)
                .write(0);
        }
    }

    fn drain_pending(&mut self) {
        while let Some(irq) = self.claim() {
            self.complete(irq);
        }
    }

    fn enable(&mut self, interrupt_id: u32) {
        let word_offset = interrupt_id / 32;
        let bit = interrupt_id % 32;
        let mut reg = self
            .enable_register
            .add_within_region(word_offset as usize, self.size / 4);
        reg |= 1 << bit;
    }

    fn set_priority(&mut self, interrupt_id: u32, priority: u32) {
        assert!(priority <= 7);
        self.priority_register_base
            .add_within_region(interrupt_id as usize, self.size / 4)
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

pub fn discover_from_device_tree(boot_cpu_id: CpuId) {
    let root = device_tree::THE.root_node();
    let plic_node = root
        .find_node("plic")
        .or_else(|| {
            // Some boards name the PLIC "interrupt-controller" instead of "plic".
            root.find_node("interrupt-controller")
        })
        .expect("Device tree must have a PLIC node");
    let reg = plic_node
        .parse_reg_property()
        .expect("PLIC node must have a reg property");

    let num_sources = plic_node
        .get_property("riscv,ndev")
        .and_then(|mut p| p.consume_sized_type::<BigEndian<u32>>())
        .expect("PLIC must have riscv,ndev property")
        .get();

    let context = find_s_mode_context(boot_cpu_id, &plic_node);

    info!(
        "PLIC at {:#x} size {:#x}, S-mode context for boot hart: {context}",
        reg.address, reg.size
    );

    PLIC.initialize(Spinlock::new(Plic::new(
        reg.address,
        reg.size,
        num_sources,
        context,
    )));
}

/// Parse the PLIC's `interrupts-extended` property and the CPU nodes to find
/// the PLIC context index for the boot hart's S-mode external interrupt.
fn find_s_mode_context(boot_cpu_id: CpuId, plic_node: &device_tree::Node<'_>) -> usize {
    let root = device_tree::THE.root_node();

    // Find the boot hart's interrupt-controller phandle.
    let cpus_node = root
        .find_node("cpus")
        .expect("Device tree must have a cpus node");
    let mut boot_intc_phandle: Option<u32> = None;
    for cpu_node in cpus_node.children() {
        if !cpu_node.name.starts_with("cpu@") {
            continue;
        }
        let Some(mut reg_prop) = cpu_node.get_property("reg") else {
            continue;
        };
        let hart_id = reg_prop
            .consume_sized_type::<BigEndian<u32>>()
            .expect("CPU reg must be a u32")
            .get();
        if hart_id as usize == boot_cpu_id.as_usize() {
            let intc = cpu_node
                .find_node("interrupt-controller")
                .expect("CPU must have an interrupt-controller child");
            boot_intc_phandle = Some(
                intc.get_property("phandle")
                    .and_then(|mut p| p.consume_sized_type::<BigEndian<u32>>())
                    .expect("interrupt-controller must have a phandle")
                    .get(),
            );
            break;
        }
    }
    let boot_intc_phandle =
        boot_intc_phandle.expect("Boot hart must exist in device tree CPU nodes");

    // Parse the PLIC's interrupts-extended property as (phandle, irq_type) pairs.
    // The pair's index in the list is the PLIC context number.
    let mut ext_prop = plic_node
        .get_property("interrupts-extended")
        .expect("PLIC must have interrupts-extended property");
    let mut context_index = 0usize;
    while ext_prop.size_left() >= 8 {
        let phandle = ext_prop
            .consume_sized_type::<BigEndian<u32>>()
            .expect("PLIC interrupts-extended phandle must be a u32")
            .get();
        let irq_type = ext_prop
            .consume_sized_type::<BigEndian<u32>>()
            .expect("PLIC interrupts-extended irq_type must be a u32")
            .get();
        if phandle == boot_intc_phandle && irq_type == S_MODE_EXTERNAL_INTERRUPT {
            return context_index;
        }
        context_index += 1;
    }

    panic!(
        "Could not find S-mode external interrupt context for boot hart in PLIC interrupts-extended"
    );
}

pub fn init_plic() {
    info!("Initializing PLIC");
    let mut plic = PLIC.lock();
    plic.disable_all();
    plic.set_threshold(0);
    plic.drain_pending();
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
