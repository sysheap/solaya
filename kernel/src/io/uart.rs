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

        // Set baud rate via divisor latch.
        // divisor = ceil(22_729_000 / (2400 * 16)) = 592
        let divisor_least: u8 = (BAUD_DIVISOR & 0xff) as u8;
        let divisor_most: u8 = (BAUD_DIVISOR >> 8) as u8;

        // Open divisor latch (DLAB bit in LCR) to access DLL/DLM registers
        self.regs.lcr().write(LCR_WORD_LEN_8BIT | LCR_DLAB);

        // With DLAB set, thr_rbr becomes DLL and ier becomes DLM
        self.regs.thr_rbr().write(divisor_least);
        self.regs.ier().write(divisor_most);

        // Close divisor latch to restore normal register access
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

pub fn on_uart_interrupt() {
    let mut raw_bytes = crate::klibc::array_vec::ArrayVec::<u8, 64>::new();
    {
        let uart = QEMU_UART.lock();
        while let Some(input) = uart.read() {
            let _ = raw_bytes.push(input);
        }
    }

    let mut signal_to_send: Option<u32> = None;
    let tty = crate::io::tty_device::console_tty();
    for &byte in &raw_bytes {
        let result = tty.lock().process_input_byte(byte);
        if !result.echo.is_empty() {
            let mut uart = QEMU_UART.lock();
            for &echo_byte in &result.echo {
                uart.write_byte(echo_byte);
            }
        }
        if let crate::io::tty_device::InputAction::Signal(sig) = result.action {
            signal_to_send = Some(sig);
        }
    }

    if let Some(sig) = signal_to_send {
        let fg_pgid = {
            let mut dev = tty.lock();
            let pgid = dev.fg_pgid();
            dev.record_tty_signal(sig, pgid);
            pgid
        };
        crate::cpu::Cpu::with_scheduler(|mut s| {
            s.send_tty_signal(sig, fg_pgid);
        });
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
