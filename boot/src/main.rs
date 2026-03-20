#![no_std]
#![no_main]

// Force the linker to include the solaya kernel library.
// Without this, the linker would discard the library since boot doesn't
// call any kernel symbols directly — all calls go through assembly.
extern crate solaya;

// SAFETY: Called from boot.S as the kernel entry point.
#[unsafe(no_mangle)]
extern "C" fn kernel_init(hart_id: usize, device_tree_pointer: *const ()) -> ! {
    solaya::kernel_init(hart_id, device_tree_pointer)
}

// SAFETY: Called from boot.S for secondary harts.
#[unsafe(no_mangle)]
extern "C" fn prepare_for_scheduling() -> ! {
    solaya::prepare_for_scheduling()
}

// SAFETY: Called from trap.S assembly.
#[unsafe(no_mangle)]
extern "C" fn handle_trap() {
    solaya::interrupts::trap::handle_trap()
}

// SAFETY: Called from trap.S assembly.
#[unsafe(no_mangle)]
extern "C" fn get_process_satp_value() -> usize {
    solaya::interrupts::trap::get_process_satp_value()
}

// SAFETY: Called from panic.S assembly.
#[unsafe(no_mangle)]
fn asm_panic_rust() {
    solaya::asm::asm_panic_rust()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    solaya::panic::panic_handler(info)
}
