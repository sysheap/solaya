macro_rules! linker_symbol {
    ($name:ident) => {
        pub fn $name() -> usize {
            unsafe extern "C" {
                static $name: usize;
            }
            core::ptr::addr_of!($name) as usize
        }
    };
}

linker_symbol!(__start_text);
linker_symbol!(__stop_text);
linker_symbol!(__start_rodata);
linker_symbol!(__stop_rodata);
linker_symbol!(__start_eh_frame);
linker_symbol!(__stop_eh_frame);
linker_symbol!(__start_data);
linker_symbol!(__stop_data);
linker_symbol!(__start_bss);
linker_symbol!(__stop_bss);
linker_symbol!(__start_kernel_stack);
linker_symbol!(__stop_kernel_stack);
linker_symbol!(__start_symbols);

/// Return the address of the asm_handle_trap function (defined in trap.S).
pub fn asm_handle_trap_addr() -> usize {
    unsafe extern "C" {
        fn asm_handle_trap();
    }
    asm_handle_trap as *const () as usize
}

/// Return the address of the start_hart function (defined in boot.S).
pub fn start_hart_addr() -> usize {
    unsafe extern "C" {
        fn start_hart();
    }
    start_hart as *const () as usize
}
