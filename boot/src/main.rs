#![no_std]
#![no_main]

// Force the linker to include the solaya kernel library.
// Without this, the linker would discard the library since boot doesn't
// call any kernel symbols directly — all calls go through assembly.
extern crate solaya;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    solaya::panic::panic_handler(info)
}
