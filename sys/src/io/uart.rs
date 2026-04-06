use core::fmt::Write;

use crate::klibc::{MMIO, Spinlock, send_sync::UnsafeSendSync};

pub const UART_BASE_ADDRESS: usize = 0x1000_0000;

const THR: usize = 0;
const IER: usize = 1;
const FCR: usize = 2;
const LCR: usize = 3;
const LSR: usize = 5;

const LCR_WORD_LEN_8BIT: u8 = 0b11;
const FCR_FIFO_EN: u8 = 0x01;
const FCR_RXSR: u8 = 0x02;
const FCR_TXSR: u8 = 0x04;
const IER_RX_AVAILABLE: u8 = 1;
const LSR_DATA_READY: u8 = 1;
const LSR_THRE: u8 = 1 << 5;

pub static CONSOLE_UART: Spinlock<UnsafeSendSync<Uart>> =
    Spinlock::new(UnsafeSendSync(Uart::new(UART_BASE_ADDRESS)));

pub struct Uart {
    base: usize,
    reg_shift: u8,
    is_init: bool,
}

impl Uart {
    const fn new(uart_base_address: usize) -> Self {
        Self {
            base: uart_base_address,
            reg_shift: 0,
            is_init: false,
        }
    }

    fn reg_addr(&self, index: usize) -> usize {
        self.base + (index << self.reg_shift as usize)
    }

    fn read_reg(&self, index: usize) -> u8 {
        let addr = self.reg_addr(index);
        if self.reg_shift >= 2 {
            MMIO::<u32>::new(addr).read() as u8
        } else {
            MMIO::<u8>::new(addr).read()
        }
    }

    fn write_reg(&self, index: usize, value: u8) {
        let addr = self.reg_addr(index);
        if self.reg_shift >= 2 {
            MMIO::<u32>::new(addr).write(value as u32);
        } else {
            MMIO::<u8>::new(addr).write(value);
        }
    }

    pub fn init(&mut self) {
        self.reg_shift = detect_reg_shift(self.base);
        self.write_reg(LCR, LCR_WORD_LEN_8BIT);
        self.write_reg(FCR, FCR_FIFO_EN | FCR_RXSR | FCR_TXSR);
        self.write_reg(IER, IER_RX_AVAILABLE);

        self.is_init = true;
    }

    fn wait_for_tx_ready(&self) {
        while self.read_reg(LSR) & LSR_THRE == 0 {
            core::hint::spin_loop();
        }
    }

    pub fn write_byte(&mut self, character: u8) {
        self.wait_for_tx_ready();
        self.write_reg(THR, character);
    }

    pub fn read(&self) -> Option<u8> {
        if self.read_reg(LSR) & LSR_DATA_READY == 0 {
            return None;
        }
        Some(self.read_reg(THR))
    }
}

fn detect_reg_shift(base: usize) -> u8 {
    if MMIO::<u8>::new(base + LSR).read() & LSR_THRE != 0 {
        return 0;
    }

    loop {
        if MMIO::<u32>::new(base + (LSR << 2)).read() as u8 & LSR_THRE != 0 {
            return 2;
        }
        core::hint::spin_loop();
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
