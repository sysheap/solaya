use std::{net::UdpSocket, os::fd::AsRawFd};

use common::ioctl::set_ip_address;
use userspace::spawn::spawn;

fn main() {
    println!("init process started");

    match spawn("dhcpd", &[]) {
        Ok(mut child) => {
            let status = child.wait().expect("Failed to wait for dhcpd");
            match status.code() {
                Some(0) => {}
                Some(2) => {
                    println!("init: no network device, skipping network services");
                    start_shell();
                    return;
                }
                _ => {
                    set_fallback_ip();
                }
            }
        }
        Err(_) => {
            set_fallback_ip();
        }
    }

    let _ = spawn("tcp_echo", &[]);
    let _ = spawn("webserver", &[]);
    start_shell();
}

fn set_fallback_ip() {
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind failed");
    set_ip_address(sock.as_raw_fd(), [192, 168, 1, 2]);
    println!("init: fallback IP 192.168.1.2 configured");
}

fn start_shell() {
    println!("starting shell");
    let mut child = spawn("dash", &[]).expect("Failed to spawn shell");
    child.wait().expect("Failed to wait for shell");
    println!("Initial shell has exited...");
}
