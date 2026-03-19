#![allow(unsafe_code)]
macro_rules! getter_address {
    ($name:ident) => {
        #[cfg(all(target_arch = "riscv64", not(miri)))]
        pub fn $name() -> VirtAddr {
            // SAFETY: These symbols are defined by the linker script. We only
            // take their address (never read their value), which is always safe.
            unsafe extern "C" {
                static $name: usize;
            }
            VirtAddr::new(core::ptr::addr_of!($name) as usize)
        }
        #[cfg(any(not(target_arch = "riscv64"), miri))]
        pub fn $name() -> VirtAddr {
            VirtAddr::new($crate::klibc::util::align_down(
                u32::MAX as usize,
                $crate::memory::PAGE_SIZE,
            ))
        }
    };
}

macro_rules! getter {
    ($name:ident) => {
        // The linker generates magic variables which marks section start and end in the form
        // __start_SECTION and __stop_SECTION
        getter_address!(${concat(__start_, $name)});
        getter_address!(${concat(__stop_, $name)});
        pub fn ${concat($name, _size)}() -> usize {
            Self::${concat(__stop_, $name)}() - Self::${concat(__start_, $name)}()
        }
        pub fn ${concat($name, _range)}() -> core::ops::Range<VirtAddr> {
            Self::${concat(__start_, $name)}()..Self::${concat(__stop_, $name)}()
        }
    };
}

// Idea taken by https://veykril.github.io/tlborm/decl-macros/building-blocks/counting.html
macro_rules! count_idents {
    () => { 0 };
    ($first:ident $($rest:ident)*) => {1 + count_idents!($($rest)*)};
}

macro_rules! sections {
    ($($name:ident, $xwr:expr;)*) => {
        use $crate::memory::address::VirtAddr;
        use $crate::memory::page_tables::MappingDescription;
        use $crate::memory::page_table_entry::XWRMode;
        use $crate::memory::PAGE_SIZE;
        use $crate::debugging;
        use $crate::klibc::util::align_up;

        pub struct LinkerInformation;

        #[allow(dead_code)]
        impl LinkerInformation {
            $(getter!($name);)*

            // We don't know the end of the symbols yet because it
            // will be binary patched
            getter_address!(__start_symbols);

            // The heap will start directly page aligned after the symbols
            pub fn __start_heap() -> VirtAddr {
                VirtAddr::new(align_up(debugging::symbols::symbols_end(), PAGE_SIZE))
            }

            #[cfg(all(target_arch = "riscv64", not(miri)))]
            pub fn all_mappings() -> [MappingDescription; count_idents!($($name)*)] {
                [
                    $(MappingDescription {
                      virtual_address_start: LinkerInformation::${concat(__start_, $name)}(),
                      size: LinkerInformation::${concat($name, _size)}(),
                      privileges: $xwr,
                      name: stringify!($name)
                    },)*
                ]
            }
            #[cfg(any(not(target_arch = "riscv64"), miri))]
            pub fn all_mappings() -> [MappingDescription; 0] {
                []
            }
        }
    };
}

sections! {
    text, XWRMode::ReadExecute;
    rodata, XWRMode::ReadOnly;
    eh_frame, XWRMode::ReadOnly;
    data, XWRMode::ReadWrite;
    bss, XWRMode::ReadWrite;
    kernel_stack, XWRMode::ReadWrite;
}

#[cfg(all(target_arch = "riscv64", not(miri)))]
impl LinkerInformation {
    pub fn get_eh_frame_bytes() -> &'static [u8] {
        let start = Self::__start_eh_frame().as_ptr::<u8>();
        let size = Self::eh_frame_size();
        // SAFETY: The eh_frame section is mapped by the kernel page tables.
        // Start and size come from linker-defined symbols.
        unsafe { core::slice::from_raw_parts(start, size) }
    }

    pub fn get_symbols_cstr() -> &'static core::ffi::CStr {
        let ptr = Self::__start_symbols().as_ptr::<core::ffi::c_char>();
        // SAFETY: The symbols section is null-terminated by the build process
        // (objcopy --update-section appends a NUL byte).
        unsafe { core::ffi::CStr::from_ptr(ptr) }
    }
}

#[cfg(any(not(target_arch = "riscv64"), miri))]
impl LinkerInformation {
    pub fn get_eh_frame_bytes() -> &'static [u8] {
        &[]
    }

    pub fn get_symbols_cstr() -> &'static core::ffi::CStr {
        core::ffi::CStr::from_bytes_with_nul(b"\0").expect("valid empty CStr")
    }
}
