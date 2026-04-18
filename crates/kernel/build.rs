use std::env;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "riscv64".into());
    println!("cargo:rerun-if-changed={arch}.ld");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    // For unit tests (which produce a binary from this library crate)
    println!("cargo:rustc-link-arg=-Tcrates/kernel/{arch}.ld");
}
