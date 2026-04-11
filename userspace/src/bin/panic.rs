use abi::ioctl::trigger_kernel_panic;

extern crate userspace;

fn main() {
    println!("Hello from Panic! Triggering kernel panic");
    trigger_kernel_panic();
}
