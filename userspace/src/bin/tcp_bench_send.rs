use std::{io::Write, net::TcpListener, time::Instant};

const PORT: u16 = 1234;

fn main() {
    let listener = TcpListener::bind(format!("0.0.0.0:{PORT}")).expect("bind must work");
    println!("tcp_bench_send listening on {PORT}");

    let (mut stream, addr) = listener.accept().expect("accept must work");
    println!("Connection from {addr}");

    let start = Instant::now();
    let chunk = [0xABu8; 65536];
    let total_to_send: usize = 4 * 1024 * 1024;
    let mut sent = 0;

    while sent < total_to_send {
        let n = chunk.len().min(total_to_send - sent);
        match stream.write_all(&chunk[..n]) {
            Ok(()) => sent += n,
            Err(e) => {
                eprintln!("write error: {e}");
                break;
            }
        }
    }

    let elapsed_ms = start.elapsed().as_millis();
    let throughput_kbps = if elapsed_ms > 0 {
        sent as u64 * 8 / elapsed_ms as u64
    } else {
        0
    };

    println!(
        "BENCH_RESULT: bytes={sent} elapsed_ms={elapsed_ms} throughput_kbps={throughput_kbps}"
    );
}
