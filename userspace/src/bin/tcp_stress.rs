use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Instant,
};

const PORT: u16 = 1234;
const BURST_SIZE: usize = 10 * 1024 * 1024;
const TOTAL: usize = 20 * 1024 * 1024;

fn fill_sequential_u64(buf: &mut [u8], counter: &mut u64) {
    for chunk in buf.chunks_exact_mut(8) {
        chunk.copy_from_slice(&counter.to_le_bytes());
        *counter += 1;
    }
}

fn main() {
    let listener = TcpListener::bind(format!("0.0.0.0:{PORT}")).expect("bind must work");
    println!("tcp_stress listening on {PORT}");

    let (stream, addr) = listener.accept().expect("accept must work");
    println!("Connection from {addr}");

    let mut send_stream = stream.try_clone().expect("clone must work");
    let mut recv_stream = stream;

    let send_thread = thread::spawn(move || {
        let mut buf = vec![0u8; BURST_SIZE];
        let mut counter: u64 = 0;
        let mut sent = 0usize;
        let start = Instant::now();

        while sent < TOTAL {
            let n = BURST_SIZE.min(TOTAL - sent);
            fill_sequential_u64(&mut buf[..n], &mut counter);
            send_stream.write_all(&buf[..n]).expect("write must work");
            sent += n;
        }

        let elapsed_ms = start.elapsed().as_millis();
        println!("STRESS_SEND: bytes={sent} elapsed_ms={elapsed_ms}");
    });

    let recv_thread = thread::spawn(move || {
        let mut buf = vec![0u8; BURST_SIZE];
        let mut counter: u64 = 0;
        let mut received = 0usize;
        let mut leftover = Vec::with_capacity(8);
        let start = Instant::now();

        while received < TOTAL {
            let n = recv_stream.read(&mut buf).expect("read must work");
            assert!(n > 0, "unexpected EOF at {received} bytes");
            received += n;

            let mut offset = 0;
            if !leftover.is_empty() {
                let need = 8 - leftover.len();
                if n >= need {
                    leftover.extend_from_slice(&buf[..need]);
                    let val = u64::from_le_bytes(leftover[..8].try_into().unwrap());
                    assert!(
                        val == counter,
                        "Guest recv mismatch at counter {counter}: got {val}"
                    );
                    counter += 1;
                    offset = need;
                    leftover.clear();
                } else {
                    leftover.extend_from_slice(&buf[..n]);
                    continue;
                }
            }

            let remaining = &buf[offset..n];
            let aligned_len = remaining.len() - (remaining.len() % 8);
            for chunk in remaining[..aligned_len].chunks_exact(8) {
                let val = u64::from_le_bytes(chunk.try_into().unwrap());
                assert!(
                    val == counter,
                    "Guest recv mismatch at counter {counter}: got {val}"
                );
                counter += 1;
            }
            if aligned_len < remaining.len() {
                leftover.extend_from_slice(&remaining[aligned_len..]);
            }
        }

        let elapsed_ms = start.elapsed().as_millis();
        println!("STRESS_RECV: bytes={received} elapsed_ms={elapsed_ms}");
    });

    send_thread.join().expect("send thread must join");
    recv_thread.join().expect("recv thread must join");
    println!("STRESS_DONE");
}
