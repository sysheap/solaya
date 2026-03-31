use std::net::UdpSocket;

const PORT: u16 = 1234;

fn main() {
    // Set static IP to 192.168.1.2
    let sock = UdpSocket::bind(format!("0.0.0.0:{PORT}")).expect("bind failed");
    let raw_fd = {
        use std::os::fd::AsRawFd;
        sock.as_raw_fd()
    };
    common::ioctl::set_ip_address(raw_fd, [192, 168, 1, 2]);
    println!("IP set to 192.168.1.2");
    println!("UDP echo listening on port {PORT}");

    let mut buf = [0u8; 1500];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, src)) => {
                let text = core::str::from_utf8(&buf[..n]).unwrap_or("<binary>");
                println!("Received {} bytes from {}: {}", n, src, text);
                sock.send_to(&buf[..n], src).expect("send_to failed");
            }
            Err(e) => {
                eprintln!("recv_from error: {e}");
                break;
            }
        }
    }
}
