use core::fmt::Write;

use crate::{
    klibc::{MMIO, Spinlock},
    mmio_struct,
};
use sys::klibc::send_sync::UnsafeSendSync;

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
        // Configure 8-bit word length, enable FIFO, and enable RX interrupts.
        // Baud rate is left as-is: on QEMU it doesn't matter, and on real
        // hardware the firmware (U-Boot/OpenSBI) has already configured it.
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

#[cfg(target_arch = "riscv64")]
pub fn on_uart_interrupt() {
    let mut raw_bytes = crate::klibc::array_vec::ArrayVec::<u8, 64>::new();
    {
        let uart = CONSOLE_UART.lock();
        while let Some(input) = uart.read() {
            let _ = raw_bytes.push(input);
        }
    }

    let mut signal_to_send: Option<u32> = None;
    let tty = crate::io::tty_device::console_tty();
    for &byte in &raw_bytes {
        let result = tty.lock().process_input_byte(byte);
        if !result.echo.is_empty() {
            let mut uart = CONSOLE_UART.lock();
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
