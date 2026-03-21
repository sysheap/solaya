const DUMMY_ADDR: usize = 0xFFFF_F000;

macro_rules! linker_symbol_stub {
    ($name:ident) => {
        pub fn $name() -> usize {
            DUMMY_ADDR
        }
    };
}

linker_symbol_stub!(__start_text);
linker_symbol_stub!(__stop_text);
linker_symbol_stub!(__start_rodata);
linker_symbol_stub!(__stop_rodata);
linker_symbol_stub!(__start_eh_frame);
linker_symbol_stub!(__stop_eh_frame);
linker_symbol_stub!(__start_data);
linker_symbol_stub!(__stop_data);
linker_symbol_stub!(__start_bss);
linker_symbol_stub!(__stop_bss);
linker_symbol_stub!(__start_kernel_stack);
linker_symbol_stub!(__stop_kernel_stack);
linker_symbol_stub!(__start_symbols);

pub fn asm_handle_trap_addr() -> usize {
    DUMMY_ADDR
}

pub fn start_hart_addr() -> usize {
    DUMMY_ADDR
}
