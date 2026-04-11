//! Console I/O: the UART driver, the single `CONSOLE_UART` static, and the
//! `info!`/`warn!`/`debug!`/`print!`/`println!` logging macros.
//!
//! Layering invariant: may depend on `hal`, `klib`, `abi`. Owns the
//! one place where kernel output becomes bytes on the wire. Callers must
//! not define a second UART or a second logging macro anywhere in the
//! workspace.
#![cfg_attr(not(any(miri, test)), no_std)]

extern crate alloc;

pub mod configuration;
pub mod uart;

use core::fmt;

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::println!(
            "[CPU {}][info][{}] {}",
            ::hal::cpu_id(),
            module_path!(),
            format_args!($($arg)*),
        );
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::println!(
            "[CPU {}][warn][{}] {}",
            ::hal::cpu_id(),
            module_path!(),
            format_args!($($arg)*),
        );
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if const { $crate::configuration::should_log_module(module_path!()) } {
            $crate::println!(
                "[CPU {}][debug][{}] {}",
                ::hal::cpu_id(),
                module_path!(),
                format_args!($($arg)*),
            );
        }
    };
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[cfg(not(miri))]
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use crate::uart::CONSOLE_UART;
    use core::fmt::Write;
    CONSOLE_UART
        .lock()
        .write_fmt(args)
        .expect("Failed to write to UART");
}

#[cfg(miri)]
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    stdout.write_fmt(args).expect("Failed to write to stdout");
    stdout.flush().expect("Failed to flush stdout");
}
