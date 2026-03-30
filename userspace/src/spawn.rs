use std::process::{Child, Command};

pub fn spawn(program: &str, args: &[&str]) -> Result<Child, std::io::Error> {
    Command::new(format!("/bin/{program}")).args(args).spawn()
}
