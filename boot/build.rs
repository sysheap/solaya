fn main() {
    println!("cargo:rerun-if-changed=../kernel/qemu.ld");
    println!("cargo:rustc-link-arg=-Tkernel/qemu.ld");
}
