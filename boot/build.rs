fn main() {
    // Select the arch-specific linker script via cargo's CARGO_CFG_TARGET_ARCH
    // (set to "riscv64" when --target is riscv64gc-unknown-none-elf).
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    println!("cargo:rerun-if-changed=../crates/kernel/{arch}.ld");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rustc-link-arg=-Tcrates/kernel/{arch}.ld");
}
