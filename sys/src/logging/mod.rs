use core::fmt;

pub mod configuration;

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::println!("[CPU {}][info][{}] {}", $crate::cpu::cpu_id(), module_path!(), format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::println!("[CPU {}][warn][{}] {}", $crate::cpu::cpu_id(), module_path!(), format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if const { $crate::logging::configuration::should_log_module(module_path!()) } {
            $crate::println!("[CPU {}][debug][{}] {}", $crate::cpu::cpu_id(), module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::logging::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[cfg(not(miri))]
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use crate::io::uart;
    use core::fmt::Write;
    uart::QEMU_UART
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
