pub use sys::mmio::{MMIO, read_bytes};

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

            impl $crate::klibc::mmio::MMIO<$name> {
                $(
                    #[allow(dead_code)]
                    pub const fn $field_name(&self) -> $crate::klibc::mmio::MMIO<$field_type> {
                        self.new_type_with_offset(core::mem::offset_of!($name, $field_name))
                    }
                )*
            }
        };
}
