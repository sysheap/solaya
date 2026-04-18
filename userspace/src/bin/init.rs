use userspace::spawn::spawn;

fn main() {
    println!("init process started");
    if let Ok(mut child) = spawn("/bin/dhcpd", &[]) {
        let _ = child.wait();
    }
    println!("starting shell");
    let mut child = spawn("/bin/dash", &[]).expect("Failed to spawn shell");
    child.wait().expect("Failed to wait for shell");
    println!("Initial shell has exited...");
}
