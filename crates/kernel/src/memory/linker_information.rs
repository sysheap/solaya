macro_rules! getter_address {
    ($name:ident) => {
        pub fn $name() -> VirtAddr {
            VirtAddr::new(arch::linker_symbols::$name())
        }
    };
}

macro_rules! getter {
    ($name:ident) => {
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
        use $crate::memory::VirtAddr;
        use $crate::memory::page_tables::MappingDescription;
        use sys::memory::page_table::XWRMode;
        use $crate::memory::PAGE_SIZE;
        use $crate::debugging;
        use $crate::klibc::util::align_up;

        pub struct LinkerInformation;

        #[allow(dead_code)]
        impl LinkerInformation {
            $(getter!($name);)*

            getter_address!(__start_symbols);

            pub fn __start_heap() -> VirtAddr {
                VirtAddr::new(align_up(debugging::symbols::symbols_end(), PAGE_SIZE))
            }

            #[cfg(not(miri))]
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
            #[cfg(miri)]
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
