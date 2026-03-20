use core::fmt::Write;

use crate::{
    klibc::{MMIO, Spinlock},
    mmio_struct,
};

pub const UART_BASE_ADDRESS: usize = 0x1000_0000;

const LCR_WORD_LEN_8BIT: u8 = 0b11;
const LCR_DLAB: u8 = 1 << 7;
const FCR_ENABLE: u8 = 1;
const IER_RX_AVAILABLE: u8 = 1;
const LSR_DATA_READY: u8 = 1;
const BAUD_DIVISOR: u16 = 592;

mmio_struct! {
    #[repr(C)]
    struct UartRegisters {
        thr_rbr: u8,
        ier: u8,
        fcr_iir: u8,
        lcr: u8,
        mcr: u8,
        lsr: u8,
    }
}

pub static QEMU_UART: Spinlock<Uart> = Spinlock::new(Uart::new(UART_BASE_ADDRESS));

// SAFETY: Uart wraps an MMIO address (fixed hardware register). Access is
// serialized through a Spinlock, making it safe to share across threads.
unsafe impl Sync for Uart {}
// SAFETY: Same reasoning as Sync — access is serialized through a Spinlock.
unsafe impl Send for Uart {}

pub struct Uart {
    regs: MMIO<UartRegisters>,
    is_init: bool,
}

impl Uart {
    const fn new(uart_base_address: usize) -> Self {
        Self {
            regs: MMIO::new(uart_base_address),
            is_init: false,
        }
    }

    pub fn init(&mut self) {
        self.regs.lcr().write(LCR_WORD_LEN_8BIT);
        self.regs.fcr_iir().write(FCR_ENABLE);
        self.regs.ier().write(IER_RX_AVAILABLE);

        let divisor_least: u8 = (BAUD_DIVISOR & 0xff) as u8;
        let divisor_most: u8 = (BAUD_DIVISOR >> 8) as u8;

        self.regs.lcr().write(LCR_WORD_LEN_8BIT | LCR_DLAB);
        self.regs.thr_rbr().write(divisor_least);
        self.regs.ier().write(divisor_most);
        self.regs.lcr().write(LCR_WORD_LEN_8BIT);

        self.is_init = true;
    }

    pub fn write_byte(&mut self, character: u8) {
        self.regs.thr_rbr().write(character);
    }

    pub fn read(&self) -> Option<u8> {
        if self.regs.lsr().read() & LSR_DATA_READY == 0 {
            return None;
        }
        Some(self.regs.thr_rbr().read())
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if !self.is_init {
            return Ok(());
        }
        for c in s.bytes() {
            self.write_byte(c);
        }
        Ok(())
    }
}
