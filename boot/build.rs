fn main() {
    println!("cargo:rerun-if-changed=../kernel/solaya.ld");
    println!("cargo:rustc-link-arg=-Tkernel/solaya.ld");
}
