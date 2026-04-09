fn main() {
    println!("cargo:rerun-if-changed=../crates/kernel/solaya.ld");
    println!("cargo:rustc-link-arg=-Tcrates/kernel/solaya.ld");
}
