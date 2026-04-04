macro_rules! linker_symbol {
    ($name:ident) => {
        #[cfg(not(miri))]
        pub fn $name() -> usize {
            unsafe extern "C" {
                static $name: usize;
            }
            core::ptr::addr_of!($name) as usize
        }

        #[cfg(miri)]
        pub fn $name() -> usize {
            0xFFFF_F000
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

#[cfg(not(miri))]
pub fn asm_handle_trap_addr() -> usize {
    unsafe extern "C" {
        fn asm_handle_trap();
    }
    asm_handle_trap as *const () as usize
}

#[cfg(miri)]
pub fn asm_handle_trap_addr() -> usize {
    0xFFFF_F000
}

#[cfg(not(miri))]
pub fn start_hart_addr() -> usize {
    unsafe extern "C" {
        fn start_hart();
    }
    start_hart as *const () as usize
}

#[cfg(miri)]
pub fn start_hart_addr() -> usize {
    0xFFFF_F000
}

#[cfg(not(miri))]
pub fn signal_trampoline_addr() -> usize {
    unsafe extern "C" {
        static __signal_trampoline: u8;
    }
    core::ptr::addr_of!(__signal_trampoline) as usize
}

#[cfg(miri)]
pub fn signal_trampoline_addr() -> usize {
    0xFFFF_F000
}

#[cfg(not(miri))]
pub fn powersave_fn_addr() -> usize {
    unsafe extern "C" {
        fn powersave();
    }
    powersave as *const () as usize
}

#[cfg(miri)]
pub fn powersave_fn_addr() -> usize {
    0xFFFF_F000
}
