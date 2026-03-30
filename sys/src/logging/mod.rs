use core::fmt;

#[cfg(target_arch = "riscv64")]
pub mod configuration;

#[cfg(target_arch = "riscv64")]
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::println!("[CPU {}][info][{}] {}", $crate::cpu::cpu_id(), module_path!(), format_args!($($arg)*));
    };
}
#[cfg(not(target_arch = "riscv64"))]
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        ()
    };
}

#[cfg(target_arch = "riscv64")]
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::println!("[CPU {}][warn][{}] {}", $crate::cpu::cpu_id(), module_path!(), format_args!($($arg)*));
    };
}
#[cfg(not(target_arch = "riscv64"))]
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        ()
    };
}

#[cfg(target_arch = "riscv64")]
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if const { $crate::logging::configuration::should_log_module(module_path!()) } {
            $crate::println!("[CPU {}][debug][{}] {}", $crate::cpu::cpu_id(), module_path!(), format_args!($($arg)*));
        }
    };
}
#[cfg(not(target_arch = "riscv64"))]
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        ()
    };
}

#[cfg(target_arch = "riscv64")]
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::logging::_print(format_args!($($arg)*)));
}
#[cfg(not(target_arch = "riscv64"))]
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        ()
    };
}

#[cfg(target_arch = "riscv64")]
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
#[cfg(not(target_arch = "riscv64"))]
#[macro_export]
macro_rules! println {
    () => {
        ()
    };
    ($($arg:tt)*) => {
        ()
    };
}

#[cfg(all(target_arch = "riscv64", not(miri)))]
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use crate::io::uart;
    use core::fmt::Write;
    uart::CONSOLE_UART
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
