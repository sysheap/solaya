use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

use common::numbers::Number;

pub fn read_bytes(addr: usize, buf: &mut [u8]) {
    let mut pos = 0;
    let len = buf.len();
    let head = addr % 8;
    if head != 0 {
        let n = (8 - head).min(len);
        for byte in &mut buf[..n] {
            let mmio: MMIO<u8> = MMIO::new(addr + pos);
            *byte = mmio.read();
            pos += 1;
        }
    }
    while pos + 8 <= len {
        let mmio: MMIO<u64> = MMIO::new(addr + pos);
        buf[pos..pos + 8].copy_from_slice(&mmio.read().to_le_bytes());
        pos += 8;
    }
    while pos < len {
        let mmio: MMIO<u8> = MMIO::new(addr + pos);
        buf[pos] = mmio.read();
        pos += 1;
    }
}

#[allow(clippy::upper_case_acronyms)]
pub struct MMIO<T> {
    addr: *mut T,
}

impl<T> MMIO<T> {
    pub const fn new(addr: usize) -> Self {
        Self {
            addr: addr as *mut T,
        }
    }

    /// # Safety
    /// The resulting address must be within the same MMIO region.
    pub const unsafe fn add(&self, count: usize) -> Self {
        // SAFETY: Caller guarantees the offset stays within the MMIO region.
        unsafe {
            Self {
                addr: self.addr.add(count),
            }
        }
    }

    /// # Safety
    /// The address must be valid for the target type `U`.
    pub const unsafe fn new_type<U>(&self) -> MMIO<U> {
        // SAFETY: Forwarded to new_type_with_offset with offset 0.
        unsafe { self.new_type_with_offset(0) }
    }

    /// # Safety
    /// The address + offset must be valid for the target type `U` and within
    /// the same MMIO region.
    pub const unsafe fn new_type_with_offset<U>(&self, offset: usize) -> MMIO<U> {
        // SAFETY: Caller guarantees the resulting address is valid for U.
        unsafe {
            MMIO::<U> {
                addr: self.addr.byte_add(offset).cast::<U>(),
            }
        }
    }
}

impl<T: Copy> MMIO<T> {
    pub fn read(&self) -> T {
        // SAFETY: The MMIO address was provided at construction and is
        // guaranteed to be valid for volatile reads of type T.
        unsafe { self.addr.read_volatile() }
    }

    pub fn write(&mut self, value: T) {
        // SAFETY: The MMIO address was provided at construction and is
        // guaranteed to be valid for volatile writes of type T.
        unsafe {
            self.addr.write_volatile(value);
        }
    }
}

impl<T: Copy, const LENGTH: usize> MMIO<[T; LENGTH]> {
    pub fn read_index(&self, index: usize) -> T {
        self.get_index(index).read()
    }

    pub fn write_index(&mut self, index: usize, value: T) {
        self.get_index(index).write(value);
    }

    fn get_index(&self, index: usize) -> MMIO<T> {
        assert!(index < LENGTH, "Access out of bounds");
        // SAFETY: Bounds-checked above; the offset stays within the array region.
        unsafe { self.new_type_with_offset(index * core::mem::size_of::<T>()) }
    }
}

impl<T: Number + BitOr<T, Output = T>> BitOrAssign<T> for MMIO<T> {
    fn bitor_assign(&mut self, rhs: T) {
        self.write(self.read() | rhs)
    }
}

impl<T: Number + BitAnd<T, Output = T>> BitAndAssign<T> for MMIO<T> {
    fn bitand_assign(&mut self, rhs: T) {
        self.write(self.read() & rhs)
    }
}

// SAFETY: MMIO wraps a raw pointer to a hardware register. Sending the
// pointer to another thread is safe; callers must provide synchronization
// for concurrent access (e.g., Spinlock).
unsafe impl<T> Send for MMIO<T> {}
// SAFETY: MMIO performs volatile reads/writes to hardware registers. Sharing
// the pointer between threads is safe; concurrent access semantics are
// defined by the hardware (e.g., reading ISR status is idempotent).
unsafe impl<T> Sync for MMIO<T> {}

impl<T> core::fmt::Pointer for MMIO<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:p}", self.addr)
    }
}

impl<T: core::fmt::Debug + Copy> core::fmt::Debug for MMIO<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.read())
    }
}

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
                        // SAFETY: offset_of! gives the correct byte offset for
                        // this field within the MMIO struct layout.
                        unsafe {
                            self.new_type_with_offset(core::mem::offset_of!($name, $field_name))
                        }
                    }
                )*
            }
        };
}
