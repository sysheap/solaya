use userspace::spawn::spawn;

fn main() {
    println!("init process started");
    // Spawn dhcpd in background (don't block shell startup)
    let _ = spawn("dhcpd", &[]);
    println!("starting shell");
    let mut child = spawn("dash", &[]).expect("Failed to spawn shell");
    child.wait().expect("Failed to wait for shell");
    println!("Initial shell has exited...");
}
