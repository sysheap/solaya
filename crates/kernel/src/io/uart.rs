//! Kernel-side UART glue: the interrupt handler that dispatches received
//! bytes to the TTY line discipline and the panic-path reboot poller.
//!
//! The actual UART driver and the single `CONSOLE_UART` static live in the
//! `console` crate. This module only contains the pieces that need to
//! reach into kernel internals (scheduler, TTY device, drivers).

use core::sync::atomic::{AtomicU8, Ordering};

pub use console::uart::CONSOLE_UART;

pub fn on_uart_interrupt() {
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
            crate::drivers::jh7110::reset::trigger_reset();
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
            crate::drivers::jh7110::reset::trigger_reset();
        }
        core::hint::spin_loop();
    }
}
