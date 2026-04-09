pub use klib::{array_vec, btreemap, non_empty_vec, runtime_initialized, sizes};

pub mod big_endian;
pub mod consumable_buffer;
pub mod elf;
pub mod leb128;
pub mod util;
pub mod writable_buffer;

pub use sys::klibc::{
    mmio::{self, MMIO},
    spinlock::{Spinlock, SpinlockGuard},
};

#[macro_export]
macro_rules! mmio_struct {
    {
        $(#[$meta:meta])*
        struct $name:ident {
            $($field_name:ident : $field_type:ty),* $(,)?
        }
    } => {
            $(#[$meta])*
            #[derive(Clone, Copy, Debug)]
            #[allow(non_camel_case_types, dead_code)]
            pub struct $name {
                $(
                    $field_name: $field_type,
                )*
            }

            #[allow(non_camel_case_types, dead_code)]
            pub trait ${concat($name, Fields)} {
                $(
                    fn $field_name(&self) -> $crate::klibc::mmio::MMIO<$field_type>;
                )*
            }

            impl ${concat($name, Fields)} for $crate::klibc::mmio::MMIO<$name> {
                $(
                    fn $field_name(&self) -> $crate::klibc::mmio::MMIO<$field_type> {
                        self.new_type_with_offset(core::mem::offset_of!($name, $field_name))
                    }
                )*
            }
        };
}
