//! Kernel-side UART glue: the interrupt handler that dispatches received
//! bytes to the TTY line discipline and the panic-path reboot poller.
//!
//! The actual UART driver and the single `CONSOLE_UART` static live in the
//! `console` crate. This module only contains the pieces that need to
//! reach into kernel internals (scheduler, TTY device, drivers).

use core::sync::atomic::{AtomicU8, Ordering};

use alloc::sync::Arc;
use driver_api::{CharDevice, IoError, IrqHandler};
use headers::errno::Errno;

pub use console::uart::CONSOLE_UART;

/// `CharDevice` adapter for the console UART.
///
/// Carries the TTY line discipline internally: `write` goes through the TTY
/// `process_output` path (handles ONLCR, echo, etc.) before hitting the
/// UART; `read` drains cooked bytes from the TTY input buffer.
///
/// The TTY itself still lives in `io/tty_device` and is wired up at init.
/// Fully decoupling TTY from UART stays deferred (#250 item #5).
pub struct ConsoleCharDevice;

impl CharDevice for ConsoleCharDevice {
    fn name(&self) -> &str {
        "console"
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
        let tty = crate::io::tty_device::console_tty();
        let data = tty.lock().get_input(buf.len());
        if data.is_empty() {
            return Err(Errno::EAGAIN);
        }
        buf[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }

    fn write(&self, data: &[u8]) -> Result<usize, IoError> {
        let tty = crate::io::tty_device::console_tty();
        let processed = tty.lock().process_output(data);
        let mut uart = CONSOLE_UART.lock();
        for &b in &processed {
            uart.write_byte(b);
        }
        Ok(data.len())
    }
}

/// Register the console UART as a `CharDevice` in both the registry and
/// devfs. Called once during kernel init.
pub fn register_console_char_device() {
    let device: Arc<dyn CharDevice> = Arc::new(ConsoleCharDevice);
    crate::drivers::CharDeviceRegistry::global().register(device.clone());
    crate::fs::devfs::register_char_device("console", device);
}

/// `IrqHandler` for the console UART. Stateless — all state lives in the
/// module-global `CONSOLE_UART` and the TTY line discipline.
pub struct UartIrqHandler;

impl IrqHandler for UartIrqHandler {
    fn handle(&self) {
        handle_uart_interrupt();
    }
}

fn handle_uart_interrupt() {
    let mut raw_bytes = klib::array_vec::ArrayVec::<u8, 64>::new();
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
            crate::platform::reset::trigger_reset();
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

const REBOOT_MAGIC: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
static REBOOT_SEQ_INDEX: AtomicU8 = AtomicU8::new(0);

fn check_reboot_magic(byte: u8) -> bool {
    let idx = REBOOT_SEQ_INDEX.load(Ordering::Relaxed);
    if byte == REBOOT_MAGIC[idx as usize] {
        let next = idx + 1;
        if next as usize == REBOOT_MAGIC.len() {
            return true;
        }
        REBOOT_SEQ_INDEX.store(next, Ordering::Relaxed);
    } else {
        REBOOT_SEQ_INDEX.store(0, Ordering::Relaxed);
    }
    false
}

/// Poll UART for the reboot magic sequence (0xDEADBEEF).
/// Called from panic handler with interrupts disabled.
#[cfg(not(test))]
pub fn poll_for_reboot() -> ! {
    CONSOLE_UART.panic_force_unlock();
    crate::println!("Polling for reboot...");
    loop {
        let byte = CONSOLE_UART.lock().read();
        if let Some(byte) = byte
            && check_reboot_magic(byte)
        {
            let mut uart = CONSOLE_UART.lock();
            for &b in b"\n[UART] Reboot magic received, rebooting...\n" {
                uart.write_byte(b);
            }
            drop(uart);
            crate::platform::reset::trigger_reset();
        }
        core::hint::spin_loop();
    }
}
