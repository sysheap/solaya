use core::{
    fmt::Write,
    sync::atomic::{AtomicU8, Ordering},
};

use crate::klibc::{MMIO, Spinlock};
use sys::klibc::send_sync::UnsafeSendSync;

pub const UART_BASE_ADDRESS: usize = 0x1000_0000;

const THR: usize = 0;
const IER: usize = 1;
const IIR: usize = 2; // Read: Interrupt Identification Register
const FCR: usize = 2; // Write: FIFO Control Register
const LCR: usize = 3;
const LSR: usize = 5;
const USR: usize = 0x1F; // DW APB UART: UART Status Register

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

    /// Read a UART register. Uses u32 access for 4-byte-spaced registers
    /// (DW_apb_uart requires reg-io-width=4), u8 for byte-spaced (QEMU).
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
        // Must detect reg_shift BEFORE any THR writes, while UART is still
        // idle from firmware — otherwise THRE clears and detection fails.
        self.reg_shift = detect_reg_shift(self.base);

        // Disable all interrupts first to prevent spurious interrupts
        // while we reconfigure.
        self.write_reg(IER, 0);

        // Configure 8-bit word length, enable FIFO.
        // Baud rate is left as-is: on QEMU it doesn't matter, and on real
        // hardware the firmware (U-Boot/OpenSBI) has already configured it.
        self.write_reg(LCR, LCR_WORD_LEN_8BIT);
        self.write_reg(FCR, FCR_FIFO_EN | FCR_RXSR | FCR_TXSR);

        // Clear any pending interrupts left by firmware.
        self.clear_pending_interrupts();

        // Now enable RX interrupts.
        self.write_reg(IER, IER_RX_AVAILABLE);

        self.is_init = true;
    }

    /// Clear all pending UART interrupt sources.
    /// On the DW APB UART, this includes the "Busy Detect" interrupt
    /// (triggered by writing LCR while busy), which can only be cleared
    /// by reading the USR register.
    pub fn clear_pending_interrupts(&self) {
        // Read IIR to clear THR Empty interrupt
        let _ = self.read_reg(IIR);
        // Read LSR to clear Line Status interrupt
        let _ = self.read_reg(LSR);
        // Read RBR to clear Received Data Available / Character Timeout
        let _ = self.read_reg(THR);
        // Read USR to clear DW APB UART Busy Detect interrupt
        if self.reg_shift >= 2 {
            let _ = self.read_reg(USR);
        }
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

/// Detect register spacing by probing LSR at both possible offsets.
/// Probe reg_shift=0 first: on QEMU the UART region is only 8 bytes,
/// so reading at offset 0x14 (reg_shift=2) would fault before the
/// trap handler is installed.
/// If neither matches immediately, the UART may still be draining
/// firmware output (booti). Loop on the reg_shift=2 offset until
/// THRE appears — it always will once the FIFO drains.
fn detect_reg_shift(base: usize) -> u8 {
    // QEMU: THRE is always set (infinite FIFO), detected immediately.
    if MMIO::<u8>::new(base + LSR).read() & LSR_THRE != 0 {
        return 0;
    }

    // DW_apb_uart: wait for UART to finish draining firmware output.
    loop {
        if MMIO::<u32>::new(base + (LSR << 2)).read() as u8 & LSR_THRE != 0 {
            return 2;
        }
        core::hint::spin_loop();
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
        // Clear any non-RX interrupt sources (e.g. DW APB UART Busy Detect)
        uart.clear_pending_interrupts();
    }

    for &byte in &raw_bytes {
        if check_reboot_magic(byte) {
            crate::println!("\n[UART] Reboot magic received, rebooting...");
            arch::sbi::extensions::srst_extension::sbi_system_reset(1, 0).assert_success();
            loop {
                core::hint::spin_loop();
            }
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

#[cfg(target_arch = "riscv64")]
const REBOOT_MAGIC: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
#[cfg(target_arch = "riscv64")]
static REBOOT_SEQ_INDEX: AtomicU8 = AtomicU8::new(0);

#[cfg(target_arch = "riscv64")]
fn advance_reboot_sequence(current_index: u8, byte: u8) -> Option<u8> {
    if byte == REBOOT_MAGIC[current_index as usize] {
        let next = current_index + 1;
        if next as usize == REBOOT_MAGIC.len() {
            return None;
        }
        Some(next)
    } else {
        Some(0)
    }
}

#[cfg(target_arch = "riscv64")]
fn check_reboot_magic(byte: u8) -> bool {
    let idx = REBOOT_SEQ_INDEX.load(Ordering::Relaxed);
    match advance_reboot_sequence(idx, byte) {
        None => true,
        Some(new_idx) => {
            REBOOT_SEQ_INDEX.store(new_idx, Ordering::Relaxed);
            false
        }
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if !self.is_init {
            return Ok(());
        }
        for c in s.bytes() {
            if c == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(c);
        }
        Ok(())
    }
}
