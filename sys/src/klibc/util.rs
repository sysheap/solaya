use core::{
    fmt::Display,
    ops::{BitAnd, BitAndAssign, BitOrAssign, Not, Rem, Shl, Shr, Sub},
};

use crate::memory::PAGE_SIZE;

const _: () = assert!(core::mem::size_of::<usize>() == core::mem::size_of::<u64>());

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

pub trait BufferExtension {
    fn interpret_as<T>(&self) -> &T;
    fn split_as<T>(&self) -> (&T, &[u8]);
}

impl BufferExtension for [u8] {
    fn interpret_as<T>(&self) -> &T {
        // SAFETY: Size and alignment are verified by assertions.
        unsafe {
            assert!(self.len() >= core::mem::size_of::<T>());
            let ptr: *const T = self.as_ptr().cast::<T>();
            assert!(
                ptr.is_aligned(),
                "pointer not aligned for {}",
                core::any::type_name::<T>()
            );
            &*ptr
        }
    }

    fn split_as<T>(&self) -> (&T, &[u8]) {
        let (header_bytes, rest) = self.split_at(core::mem::size_of::<T>());
        (header_bytes.interpret_as(), rest)
    }
}

pub trait ByteInterpretable {
    fn as_slice(&self) -> &[u8] {
        // SAFETY: It is always safe to interpret an allocated struct as bytes
        unsafe {
            core::slice::from_raw_parts(
                (self as *const Self).cast::<u8>(),
                core::mem::size_of_val(self),
            )
        }
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn cstr_from_null_terminated_ptr(ptr: *const core::ffi::c_char) -> &'static core::ffi::CStr {
    assert!(
        !ptr.is_null(),
        "cstr_from_null_terminated_ptr: null pointer"
    );
    // SAFETY: Caller guarantees ptr points to a null-terminated string.
    unsafe { core::ffi::CStr::from_ptr(ptr) }
}

pub fn as_byte_slice<T: ?Sized>(value: &T) -> &[u8] {
    // SAFETY: It is always safe to interpret an allocated struct as bytes.
    unsafe {
        core::slice::from_raw_parts(
            (value as *const T).cast::<u8>(),
            core::mem::size_of_val(value),
        )
    }
}

pub fn ref_from_bytes<T>(data: &[u8]) -> &T {
    assert!(data.len() >= core::mem::size_of::<T>());
    let ptr = data.as_ptr().cast::<T>();
    assert!(ptr.is_aligned(), "ref_from_bytes: pointer not aligned");
    // SAFETY: Size and alignment checked above.
    unsafe { &*ptr }
}

pub fn slice_from_bytes<T>(data: &[u8], offset: usize, count: usize) -> &[T] {
    let elem_size = core::mem::size_of::<T>();
    let total = elem_size * count;
    assert!(
        offset + total <= data.len(),
        "slice_from_bytes: offset {} + total {} > len {}",
        offset,
        total,
        data.len()
    );
    let ptr = data[offset..].as_ptr().cast::<T>();
    assert!(ptr.is_aligned(), "slice_from_bytes: pointer not aligned");
    // SAFETY: Bounds and alignment checked above.
    unsafe { core::slice::from_raw_parts(ptr, count) }
}

pub fn read_from_bytes<T: Copy>(data: &[u8]) -> T {
    assert!(
        data.len() >= core::mem::size_of::<T>(),
        "read_from_bytes: need {} bytes, got {}",
        core::mem::size_of::<T>(),
        data.len()
    );
    // SAFETY: Length checked above. read_unaligned handles any alignment.
    unsafe { core::ptr::read_unaligned(data.as_ptr().cast::<T>()) }
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

/// A heap buffer guaranteed to be 8-byte aligned, suitable for ELF parsing.
pub struct AlignedBuffer {
    inner: alloc::vec::Vec<u64>,
    len: usize,
}

impl AlignedBuffer {
    pub fn new(size: usize) -> Self {
        let u64_count = size.div_ceil(8);
        Self {
            inner: alloc::vec![0u64; u64_count],
            len: size,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: u64 has alignment >= u8, the allocation covers at least
        // `len` bytes, and the zeroed u64 values are valid u8 values.
        unsafe { core::slice::from_raw_parts(self.inner.as_ptr().cast(), self.len) }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: same as as_bytes; additionally we have exclusive access.
        unsafe { core::slice::from_raw_parts_mut(self.inner.as_mut_ptr().cast(), self.len) }
    }
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
