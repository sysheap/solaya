use std::{io::Read, net::TcpListener, time::Instant};

const PORT: u16 = 1234;

fn main() {
    let listener = TcpListener::bind(format!("0.0.0.0:{PORT}")).expect("bind must work");
    println!("tcp_bench listening on {PORT}");

    let (mut stream, addr) = listener.accept().expect("accept must work");
    println!("Connection from {addr}");

    let start = Instant::now();
    let mut total_bytes: u64 = 0;
    let mut buf = [0u8; 65536];

    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => total_bytes += n as u64,
            Err(e) => {
                eprintln!("read error: {e}");
                break;
            }
        }
    }

    let elapsed_ms = start.elapsed().as_millis();
    let throughput_kbps = if elapsed_ms > 0 {
        total_bytes * 8 / elapsed_ms as u64
    } else {
        0
    };

    println!(
        "BENCH_RESULT: bytes={total_bytes} elapsed_ms={elapsed_ms} throughput_kbps={throughput_kbps}"
    );
}
