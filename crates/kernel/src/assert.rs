#[allow(dead_code)]
pub fn assert_unreachable() -> ! {
    panic!("assert_unreachable");
}

pub(crate) use abi::static_assert_size;
