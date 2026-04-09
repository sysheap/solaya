#![cfg_attr(miri, allow(unused_imports))]
use crate::println;
use core::{
    panic::PanicInfo,
    sync::atomic::{AtomicIsize, AtomicU8},
};

#[cfg(test)]
use crate::test::qemu_exit::exit_failure;

static PANIC_COUNTER: AtomicU8 = AtomicU8::new(0);
static CPU_ENTERED_PANIC: AtomicIsize = AtomicIsize::new(-1);

#[cfg(all(not(miri), test))]
#[panic_handler]
fn test_panic_handler(info: &PanicInfo) -> ! {
    panic_handler(info)
}

pub fn panic_handler(info: &PanicInfo) -> ! {
    use core::sync::atomic::Ordering;

    use crate::{asm::wfi_loop, cpu::cpu_id, io::uart::CONSOLE_UART};

    // sys wrapper exists specifically because kernel has forbid(unsafe_code) —
    // the raw arch::cpu::disable_global_interrupts call is unsafe and cannot
    // be emitted from here directly. Do not inline.
    sys::panic_support::panic_disable_interrupts();

    let my_cpu_id = cpu_id().as_usize() as isize;

    // Check if we are the first cpu encountering a panic
    if CPU_ENTERED_PANIC
        .compare_exchange(-1, my_cpu_id, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
        && CPU_ENTERED_PANIC.load(Ordering::Relaxed) != my_cpu_id
    {
        // Suspend here because panic happened on another cpu
        wfi_loop();
    }

    CONSOLE_UART.panic_force_unlock();

    println!("\nKERNEL Panic");
    println!("\nPanic Occurred on cpu {}!", cpu_id());
    println!("Message: {}", info.message());
    if let Some(location) = info.location() {
        println!("Location: {}", location);
    }

    abort_if_double_panic();
    crate::debugging::backtrace::print();

    println!("\nPanic Occurred on cpu {}!", cpu_id());
    println!("Message: {}", info.message());
    if let Some(location) = info.location() {
        println!("Location: {}", location);
    }
    println!("Time to attach gdb ;) use 'just attach'");

    #[cfg(test)]
    exit_failure(1);

    #[cfg(not(test))]
    crate::io::uart::poll_for_reboot();
}

fn abort_if_double_panic() {
    let current = PANIC_COUNTER.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

    if current >= 1 {
        println!("Panic in panic! ABORTING!");
        println!("Time to attach gdb ;) use 'just attach'");

        #[cfg(test)]
        exit_failure(1);

        #[cfg(not(test))]
        crate::io::uart::poll_for_reboot();
    }
}
