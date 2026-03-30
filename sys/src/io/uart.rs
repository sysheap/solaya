use core::fmt::Write;

use crate::{
    klibc::{MMIO, Spinlock, send_sync::UnsafeSendSync},
    mmio_struct,
};

pub const UART_BASE_ADDRESS: usize = 0x1000_0000;

const LCR_WORD_LEN_8BIT: u8 = 0b11;
const FCR_ENABLE: u8 = 1;
const IER_RX_AVAILABLE: u8 = 1;
const LSR_DATA_READY: u8 = 1;

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

pub static CONSOLE_UART: Spinlock<UnsafeSendSync<Uart>> =
    Spinlock::new(UnsafeSendSync(Uart::new(UART_BASE_ADDRESS)));

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
