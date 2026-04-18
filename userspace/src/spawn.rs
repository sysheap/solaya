use std::process::{Child, Command};

/// Spawn a userspace program.  Absolute paths pass through; bare names
/// get `/bin/` prepended, matching where our Rust binaries live after
/// buildroot's rootfs overlay stages them.
pub fn spawn(program: &str, args: &[&str]) -> Result<Child, std::io::Error> {
    let path = if program.starts_with('/') {
        program.to_string()
    } else {
        format!("/bin/{program}")
    };
    Command::new(path).args(args).spawn()
}
