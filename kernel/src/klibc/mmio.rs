#![allow(unsafe_code)]
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

use common::numbers::Number;

/// Read bytes from an MMIO region, using word-sized reads where aligned.
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

            impl $crate::klibc::mmio::MMIO<$name> {
                $(
                    #[allow(dead_code)]
                    pub const fn $field_name(&self) -> $crate::klibc::mmio::MMIO<$field_type> {
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

#[cfg(test)]
mod tests {
    use core::{
        any::Any,
        cell::UnsafeCell,
        mem::offset_of,
        ptr::{addr_of, addr_of_mut},
    };

    use crate::io::uart::QEMU_UART;

    use super::*;

    mmio_struct! {
        #[repr(C)]
        struct mmio_b {
            b1: u16,
            b2: [u8; 3],
            b3: u64,
        }
    }

    mmio_struct! {
        #[repr(C)]
        struct mmio_a{
            a1: u64,
            a2: u8,
            a3: mmio_b,
            a4: u8
        }
    }

    fn get_test_data() -> mmio_a {
        mmio_a {
            a1: 18,
            a2: 43,
            a3: mmio_b {
                b1: 20,
                b2: [100, 102, 103],
                b3: 22,
            },
            a4: 199,
        }
    }

    macro_rules! check_offset {
        ($value:ident, $mmio: ident, $( $field_path:ident ).+) => {
            let addr1 = addr_of!($value.$($field_path).+ );
            let addr2 = $mmio.$( $field_path()).+.addr;
            assert_eq!(addr1, addr2);
        };
    }

    fn mmio<T>(value: *mut T) -> MMIO<T> {
        MMIO { addr: value }
    }

    #[test_case]
    fn print_works() {
        let mut value = get_test_data();

        unsafe {
            QEMU_UART.force_unlock();
        }

        crate::println!("value at {:p}", &value);

        let mmio = mmio(&mut value);

        crate::println!("{:?}", mmio);
    }

    #[test_case]
    fn offsets() {
        let mut value = get_test_data();

        let mmio = mmio(&mut value);

        check_offset!(value, mmio, a1);
        check_offset!(value, mmio, a2);
        check_offset!(value, mmio, a3);

        check_offset!(value, mmio, a3.b1);
        check_offset!(value, mmio, a3.b2);
        check_offset!(value, mmio, a3.b3);

        check_offset!(value, mmio, a4);
    }

    #[test_case]
    fn struct_case() {
        let value = UnsafeCell::new(get_test_data());
        let ptr = value.get();

        let mmio = mmio(ptr);

        mmio.a1().write(0);
        mmio.a2().write(1);
        mmio.a3().b1().write(2);
        mmio.a3().b2().write_index(0, 3);
        mmio.a3().b2().write_index(1, 4);
        mmio.a3().b2().write_index(2, 5);
        mmio.a3().b3().write(6);
        mmio.a4().write(7);

        let read_value = unsafe { value.get().read_unaligned() };
        unsafe {
            assert_eq!(core::ptr::addr_of!(read_value.a1).read_unaligned(), 0);
            assert_eq!(core::ptr::addr_of!(read_value.a2).read_unaligned(), 1);
            assert_eq!(core::ptr::addr_of!(read_value.a3.b1).read_unaligned(), 2);
            assert_eq!(core::ptr::addr_of!(read_value.a3.b2[0]).read_unaligned(), 3);
            assert_eq!(core::ptr::addr_of!(read_value.a3.b2[1]).read_unaligned(), 4);
            assert_eq!(core::ptr::addr_of!(read_value.a3.b2[2]).read_unaligned(), 5);
            assert_eq!(core::ptr::addr_of!(read_value.a3.b3).read_unaligned(), 6);
            assert_eq!(core::ptr::addr_of!(read_value.a4).read_unaligned(), 7);
        }
    }

    #[test_case]
    fn scalar() {
        let mut value = UnsafeCell::new(42);
        let ptr = value.get();

        let mut mmio = mmio(ptr);

        assert_eq!(mmio.addr as *const i32, ptr);

        assert_eq!(mmio.read(), 42);

        mmio.write(128);

        assert_eq!(*value.get_mut(), 128);
    }
}
