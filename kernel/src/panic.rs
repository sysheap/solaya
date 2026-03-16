#![cfg_attr(miri, allow(unused_imports))]
use crate::{println, test::qemu_exit::wait_for_the_end};
use core::{
    panic::PanicInfo,
    sync::atomic::{AtomicIsize, AtomicU8},
};

#[cfg(test)]
use crate::test::qemu_exit::exit_failure;

static PANIC_COUNTER: AtomicU8 = AtomicU8::new(0);
static CPU_ENTERED_PANIC: AtomicIsize = AtomicIsize::new(-1);

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use core::sync::atomic::Ordering;

    use crate::{asm::wfi_loop, cpu::Cpu, io::uart::QEMU_UART};

    // SAFETY: We are panicking; no further interrupt handling is expected.
    unsafe {
        sys::cpu::disable_global_interrupts();
    }

    let cpu_id = Cpu::cpu_id().as_usize() as isize;

    // Check if we are the first cpu encountering a panic
    if CPU_ENTERED_PANIC
        .compare_exchange(-1, cpu_id, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
        && CPU_ENTERED_PANIC.load(Ordering::Relaxed) != cpu_id
    {
        // Suspend here because panic happened on another cpu
        wfi_loop();
    }

    // SAFETY: We are panicking and need to print regardless of who holds
    // the UART lock (they will never resume).
    unsafe {
        QEMU_UART.force_unlock();
    }

    println!("\nKERNEL Panic");
    println!("\nPanic Occurred on cpu {}!", Cpu::cpu_id());
    println!("Message: {}", info.message());
    if let Some(location) = info.location() {
        println!("Location: {}", location);
    }
    let kernel_page_tables = Cpu::maybe_kernel_page_tables();
    if let Some(kernel_page_tables) = kernel_page_tables {
        println!("Kernel Page Tables {kernel_page_tables}");
    }
    abort_if_double_panic();
    crate::debugging::backtrace::print();

    crate::debugging::dump_current_state();

    println!("\nPanic Occurred on cpu {}!", Cpu::cpu_id());
    println!("Message: {}", info.message());
    if let Some(location) = info.location() {
        println!("Location: {}", location);
    }
    println!("Time to attach gdb ;) use 'just attach'");

    #[cfg(test)]
    exit_failure(1);

    #[cfg(not(test))]
    wait_for_the_end();
}

fn abort_if_double_panic() {
    let current = PANIC_COUNTER.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

    if current >= 1 {
        println!("Panic in panic! ABORTING!");
        println!("Time to attach gdb ;) use 'just attach'");

        #[cfg(test)]
        exit_failure(1);

        #[cfg(not(test))]
        wait_for_the_end();
    }
}
