pub fn asm_panic_rust() {
    panic!("asm panic");
}

pub extern "C" fn wfi_loop() -> ! {
    loop {}
}
