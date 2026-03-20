#[allow(non_snake_case)]
pub const fn KiB(value: usize) -> usize {
    value * 1024
}

#[allow(non_snake_case)]
pub const fn MiB(value: usize) -> usize {
    KiB(value) * 1024
}

#[allow(non_snake_case)]
pub const fn GiB(value: usize) -> usize {
    MiB(value) * 1024
}
