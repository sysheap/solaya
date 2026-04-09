#[cfg(not(miri))]
use core::arch::asm;

pub const CLINT_BASE: usize = 0x2000000;
pub const CLINT_SIZE: usize = 0x10000;

#[cfg(not(miri))]
pub fn get_current_clocks() -> u64 {
    let current: u64;
    // SAFETY: rdtime reads the platform timer; it has no side-effects and
    // returns the value in a general-purpose register.
    unsafe {
        asm!("rdtime {current}", current = out(reg)current);
    };
    current
}

#[cfg(miri)]
pub fn get_current_clocks() -> u64 {
    0
}
