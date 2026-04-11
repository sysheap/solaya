use hal::memory::PAGE_SIZE;
use klib::util::align_up;

pub fn align_up_page_size(value: usize) -> usize {
    align_up(value, PAGE_SIZE)
}

pub const fn minimum_amount_of_pages(value: usize) -> usize {
    align_up(value, PAGE_SIZE) / PAGE_SIZE
}
