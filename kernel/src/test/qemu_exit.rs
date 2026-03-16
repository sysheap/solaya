use crate::{
    io::TEST_DEVICE_ADDRESS,
    klibc::{MMIO, Spinlock},
};

const EXIT_SUCCESS_CODE: u32 = 0x5555;
#[allow(dead_code)]
const EXIT_FAILURE_CODE: u32 = 0x3333;
#[allow(dead_code)]
const EXIT_RESET_CODE: u32 = 0x7777;

static TEST_DEVICE: Spinlock<MMIO<u32>> = Spinlock::new(MMIO::new(TEST_DEVICE_ADDRESS));

pub fn exit_success() -> ! {
    TEST_DEVICE.lock().write(EXIT_SUCCESS_CODE);
    wait_for_the_end();
}

#[allow(dead_code)]
pub fn exit_failure(code: u16) -> ! {
    TEST_DEVICE
        .lock()
        .write(EXIT_FAILURE_CODE | ((code as u32) << 16));
    wait_for_the_end();
}

#[allow(dead_code)]
pub fn exit_reset() -> ! {
    TEST_DEVICE.lock().write(EXIT_RESET_CODE);
    wait_for_the_end();
}

pub fn wait_for_the_end() -> ! {
    // SAFETY: We are shutting down — disabling interrupts prevents further preemption.
    unsafe {
        sys::cpu::disable_global_interrupts();
    }
    loop {
        sys::cpu::wait_for_interrupt();
    }
}
