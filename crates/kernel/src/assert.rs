#[allow(dead_code)]
pub fn assert_unreachable() -> ! {
    panic!("assert_unreachable");
}

pub(crate) use common::static_assert_size;
