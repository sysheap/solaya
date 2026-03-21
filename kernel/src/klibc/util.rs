use core::{
    fmt::Display,
    ops::{BitAnd, BitAndAssign, BitOrAssign, Not, Rem, Shl, Shr, Sub},
};

use crate::memory::PAGE_SIZE;

const _: () = assert!(core::mem::size_of::<usize>() == core::mem::size_of::<u64>());

/// Lossless `u64` → `usize` conversion. Clippy warns about `as usize` on u64
/// because it could truncate on 32-bit targets. The compile-time assert above
/// guarantees we are on a 64-bit platform, so this is safe.
#[allow(clippy::wrong_self_convention)]
pub trait UsizeExt {
    fn as_usize(self) -> usize;
}

impl UsizeExt for u64 {
    fn as_usize(self) -> usize {
        self as usize
    }
}

pub fn wrapping_add_signed(base: usize, offset: i64) -> usize {
    if offset >= 0 {
        base.wrapping_add(offset.unsigned_abs().as_usize())
    } else {
        base.wrapping_sub(offset.unsigned_abs().as_usize())
    }
}

pub fn align_up_page_size(value: usize) -> usize {
    align_up(value, PAGE_SIZE)
}

pub const fn align_up(value: usize, alignment: usize) -> usize {
    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        value + alignment - remainder
    }
}

pub fn align_down_ptr<T>(ptr: *const T, alignment: usize) -> *const T {
    assert!(
        alignment.is_power_of_two(),
        "alignment must be a power of two"
    );
    ptr.mask(!(alignment - 1))
}

#[cfg(miri)]
pub fn align_down(value: usize, alignment: usize) -> usize {
    assert!(
        alignment.is_power_of_two(),
        "alignment must be a power of two"
    );
    value & !(alignment - 1)
}

pub struct PrintMemorySizeHumanFriendly(pub usize);

impl Display for PrintMemorySizeHumanFriendly {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut size = self.0 as f64;
        for format in ["", "KiB", "MiB", "GiB"] {
            if size < 1024.0 {
                return write!(f, "{size:.2} {format}");
            }
            size /= 1024.0;
        }
        write!(f, "{size:.2} TiB")
    }
}

pub fn copy_slice<T: Copy>(src: &[T], dst: &mut [T]) {
    assert!(dst.len() >= src.len());
    dst[..src.len()].copy_from_slice(src);
}

pub const fn minimum_amount_of_pages(value: usize) -> usize {
    align_up(value, PAGE_SIZE) / PAGE_SIZE
}

// Re-export unsafe utility functions from sys
pub use sys::klibc::util::{read_from_bytes, ref_from_bytes, slice_from_bytes};

pub trait BufferExtension {
    fn interpret_as<T>(&self) -> &T;
    fn split_as<T>(&self) -> (&T, &[u8]);
}

impl BufferExtension for [u8] {
    fn interpret_as<T>(&self) -> &T {
        ref_from_bytes(self)
    }

    fn split_as<T>(&self) -> (&T, &[u8]) {
        let (header_bytes, rest) = self.split_at(core::mem::size_of::<T>());
        (header_bytes.interpret_as(), rest)
    }
}

pub trait ByteInterpretable {
    fn as_slice(&self) -> &[u8] {
        sys::klibc::util::as_byte_slice(self)
    }
}

pub fn is_power_of_2_or_zero<DataType>(value: DataType) -> bool
where
    DataType:
        BitAnd<Output = DataType> + PartialEq<DataType> + From<u8> + Sub<Output = DataType> + Copy,
{
    value & (value - DataType::from(1)) == DataType::from(0)
}

pub fn is_aligned<DataType>(value: DataType, alignment: DataType) -> bool
where
    DataType: Rem<DataType, Output = DataType> + PartialEq<DataType> + From<u8>,
{
    value % alignment == DataType::from(0)
}

pub fn set_or_clear_bit<DataType>(
    data: &mut DataType,
    should_set_bit: bool,
    bit_position: usize,
) -> DataType
where
    DataType: BitOrAssign
        + BitAndAssign
        + Not<Output = DataType>
        + From<u8>
        + Shl<usize, Output = DataType>
        + Copy,
{
    if should_set_bit {
        set_bit(data, bit_position);
    } else {
        clear_bit(data, bit_position)
    }
    *data
}

pub fn set_bit<DataType>(data: &mut DataType, bit_position: usize)
where
    DataType: BitOrAssign + Not<Output = DataType> + From<u8> + Shl<usize, Output = DataType>,
{
    *data |= DataType::from(1) << bit_position
}

pub fn clear_bit<DataType>(data: &mut DataType, bit_position: usize)
where
    DataType: BitAndAssign + Not<Output = DataType> + From<u8> + Shl<usize, Output = DataType>,
{
    *data &= !(DataType::from(1) << bit_position)
}

pub fn get_bit<DataType>(data: DataType, bit_position: usize) -> bool
where
    DataType: Shr<usize, Output = DataType>
        + BitAnd<DataType, Output = DataType>
        + PartialEq<DataType>
        + From<u8>,
{
    ((data >> bit_position) & DataType::from(0x1)) == DataType::from(1)
}

pub fn set_multiple_bits<DataType, ValueType>(
    data: &mut DataType,
    value: ValueType,
    number_of_bits: usize,
    bit_position: usize,
) -> DataType
where
    DataType: BitAndAssign
        + BitOrAssign
        + Not<Output = DataType>
        + From<u8>
        + Shl<usize, Output = DataType>
        + Copy,
    ValueType: Copy + BitAnd + From<u8> + Shl<usize, Output = ValueType>,
    <ValueType as BitAnd>::Output: PartialOrd<ValueType>,
{
    let mut mask: DataType = !(DataType::from(0));

    for idx in 0..number_of_bits {
        mask &= !(DataType::from(1) << (bit_position + idx));
    }

    *data &= mask;

    mask = DataType::from(0);

    for idx in 0..number_of_bits {
        if (value & (ValueType::from(1) << idx)) > ValueType::from(0) {
            mask |= DataType::from(1) << (bit_position + idx);
        }
    }

    *data |= mask;
    *data
}

pub fn get_multiple_bits<DataType, ValueType>(
    data: DataType,
    number_of_bits: usize,
    bit_position: usize,
) -> ValueType
where
    DataType: Shr<usize, Output = DataType> + BitAnd<u64, Output = ValueType>,
{
    (data >> bit_position)
        & (2u64.pow(u32::try_from(number_of_bits).expect("bit count fits in u32")) - 1)
}

pub trait InBytes {
    fn in_bytes(&self) -> usize;
}

impl<T> InBytes for alloc::vec::Vec<T> {
    fn in_bytes(&self) -> usize {
        self.len() * core::mem::size_of::<T>()
    }
}

impl<T, const N: usize> InBytes for [T; N] {
    fn in_bytes(&self) -> usize {
        N * core::mem::size_of::<T>()
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::memory::PAGE_SIZE;

    #[kani::proof]
    fn align_up_is_at_least_input() {
        let value: usize = kani::any();
        let alignment: usize = kani::any();
        kani::assume(alignment > 0);
        kani::assume(alignment <= 4096);
        kani::assume(alignment.is_power_of_two());
        kani::assume(value <= usize::MAX - alignment);
        let result = align_up(value, alignment);
        assert!(result >= value);
    }

    #[kani::proof]
    fn align_up_is_multiple_of_alignment() {
        let value: usize = kani::any();
        let alignment: usize = kani::any();
        kani::assume(alignment > 0);
        kani::assume(alignment <= 4096);
        kani::assume(alignment.is_power_of_two());
        kani::assume(value <= usize::MAX - alignment);
        let result = align_up(value, alignment);
        assert!(result % alignment == 0);
    }

    #[kani::proof]
    fn align_up_idempotent() {
        let value: usize = kani::any();
        let alignment: usize = kani::any();
        kani::assume(alignment > 0);
        kani::assume(alignment <= 4096);
        kani::assume(alignment.is_power_of_two());
        kani::assume(value <= usize::MAX - alignment);
        let once = align_up(value, alignment);
        let twice = align_up(once, alignment);
        assert!(once == twice);
    }

    #[kani::proof]
    fn set_clear_bit_roundtrip() {
        let original: u64 = kani::any();
        let bit_pos: usize = kani::any();
        kani::assume(bit_pos < 64);
        let mut data = original;
        set_bit(&mut data, bit_pos);
        clear_bit(&mut data, bit_pos);
        let mut expected = original;
        clear_bit(&mut expected, bit_pos);
        assert!(data == expected);
    }

    #[kani::proof]
    fn set_bit_only_affects_target() {
        let original: u64 = kani::any();
        let bit_pos: usize = kani::any();
        kani::assume(bit_pos < 64);
        let mut data = original;
        set_bit(&mut data, bit_pos);
        let mask = !(1u64 << bit_pos);
        assert!((data & mask) == (original & mask));
    }

    #[kani::proof]
    #[kani::unwind(9)]
    fn set_get_multiple_bits_roundtrip() {
        let mut data: u64 = kani::any();
        let value: u8 = kani::any();
        let n_bits: usize = kani::any();
        let bit_pos: usize = kani::any();
        kani::assume(n_bits > 0 && n_bits <= 8);
        kani::assume(bit_pos <= 64 - n_bits);
        let mask = u8::MAX >> (8 - n_bits);
        kani::assume(value == value & mask);
        set_multiple_bits(&mut data, value, n_bits, bit_pos);
        let got: u8 = get_multiple_bits::<u64, u64>(data, n_bits, bit_pos)
            .try_into()
            .expect("fits in u8");
        assert!(got == value);
    }

    #[kani::proof]
    fn minimum_pages_covers_value() {
        let value: usize = kani::any();
        kani::assume(value > 0);
        kani::assume(value <= usize::MAX - PAGE_SIZE);
        let pages = minimum_amount_of_pages(value);
        assert!(pages * PAGE_SIZE >= value);
        assert!((pages - 1) * PAGE_SIZE < value);
    }
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use crate::memory::PAGE_SIZE;

    #[test_case]
    fn align_up() {
        assert_eq!(super::align_up(26, 4), 28);
        assert_eq!(super::align_up(37, 3), 39);
        assert_eq!(super::align_up(64, 2), 64);
    }

    #[test_case]
    fn align_up_number_of_pages() {
        assert_eq!(super::minimum_amount_of_pages(PAGE_SIZE - 15), 1);
        assert_eq!(super::minimum_amount_of_pages(PAGE_SIZE + 15), 2);
        assert_eq!(super::minimum_amount_of_pages(PAGE_SIZE * 2), 2);
    }

    #[test_case]
    fn copy_from_slice() {
        let src = [1, 2, 3, 4, 5];
        let mut dst = [0, 0, 0, 0, 0, 0, 0];
        super::copy_slice(&src, &mut dst);
        assert_eq!(dst, [1, 2, 3, 4, 5, 0, 0]);
    }

    #[test_case]
    fn set_or_clear_bit() {
        let mut value: u64 = 0b1101101;
        super::set_or_clear_bit(&mut value, true, 1);
        assert_eq!(value, 0b1101111);
        super::set_or_clear_bit(&mut value, false, 1);
        assert_eq!(value, 0b1101101);
        super::set_or_clear_bit(&mut value, false, 0);
        assert_eq!(value, 0b1101100);
    }

    #[test_case]
    fn set_bit() {
        let mut value: u64 = 0b1101110;
        super::set_bit(&mut value, 0);
        assert_eq!(value, 0b1101111);
        super::set_bit(&mut value, 4);
        assert_eq!(value, 0b1111111);
    }

    #[test_case]
    fn clear_bit() {
        let mut value: u64 = 0b1101111;
        super::clear_bit(&mut value, 0);
        assert_eq!(value, 0b1101110);
        super::clear_bit(&mut value, 5);
        assert_eq!(value, 0b1001110);
        super::clear_bit(&mut value, 0);
        assert_eq!(value, 0b1001110);
    }

    #[test_case]
    fn get_bit() {
        let value: u64 = 0b1101101;
        assert!(super::get_bit(value, 0));
        assert!(!super::get_bit(value, 1));
        assert!(super::get_bit(value, 2));
    }

    #[test_case]
    fn set_multiple_bits() {
        let mut value: u64 = 0b1101101;
        super::set_multiple_bits(&mut value, 0b111, 3, 0);
        assert_eq!(value, 0b1101111);
        super::set_multiple_bits(&mut value, 0b110, 3, 1);
        assert_eq!(value, 0b1101101);
        super::set_multiple_bits(&mut value, 0b011, 3, 2);
        assert_eq!(value, 0b1101101);
    }

    #[test_case]
    fn get_multiple_bits() {
        let value: u64 = 0b1101101;
        assert_eq!(super::get_multiple_bits(value, 3, 0), 0b101);
        assert_eq!(super::get_multiple_bits(value, 3, 1), 0b110);
        assert_eq!(super::get_multiple_bits(value, 3, 2), 0b011);
    }

    #[test_case]
    fn split_as_parses_header_and_remainder() {
        use super::BufferExtension;

        #[repr(C)]
        struct Header {
            tag: u16,
            len: u16,
            flags: u8,
        }

        let payload = [0xAA, 0xBB, 0xCC];
        let total_len = core::mem::size_of::<Header>() + payload.len();
        // Allocate with Header alignment so interpret_as's is_aligned() check is guaranteed.
        let layout =
            alloc::alloc::Layout::from_size_align(total_len, core::mem::align_of::<Header>())
                .expect("Layout must be valid");
        let buf = unsafe {
            let ptr = alloc::alloc::alloc_zeroed(layout);
            core::slice::from_raw_parts_mut(ptr, total_len)
        };
        buf[0..2].copy_from_slice(&0xCAFEu16.to_ne_bytes());
        buf[2..4].copy_from_slice(&128u16.to_ne_bytes());
        buf[4] = 0x07;
        buf[core::mem::size_of::<Header>()..].copy_from_slice(&payload);

        let (header, rest) = buf.split_as::<Header>();

        assert_eq!(header.tag, 0xCAFE);
        assert_eq!(header.len, 128);
        assert_eq!(header.flags, 0x07);
        assert_eq!(rest, &payload);

        unsafe { alloc::alloc::dealloc(buf.as_mut_ptr(), layout) };
    }
}
