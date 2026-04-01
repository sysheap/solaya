use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::infra::qemu::{QemuInstance, QemuOptions};

fn fill_sequential_u64(buf: &mut [u8], counter: &mut u64) {
    for chunk in buf.chunks_exact_mut(8) {
        chunk.copy_from_slice(&counter.to_le_bytes());
        *counter += 1;
    }
}

#[tokio::test]
async fn tcp_throughput_send() -> anyhow::Result<()> {
    let mut solaya =
        QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;

    solaya
        .run_prog_waiting_for("tcp_bench_send", "tcp_bench_send listening on 1234\n")
        .await
        .expect("tcp_bench_send must succeed to start");

    let port = solaya.network_port().expect("Network must be enabled");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await?;

    solaya.stdout().assert_read_until("Connection from").await?;

    let mut total = 0usize;
    let mut buf = vec![0u8; 65536];
    let start = std::time::Instant::now();
    loop {
        match stream.read(&mut buf).await? {
            0 => break,
            n => total += n,
        }
    }
    let elapsed = start.elapsed();
    let throughput_mib = total as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
    eprintln!("Guest->Host: {total} bytes in {elapsed:?} = {throughput_mib:.1} MiB/s");

    assert_eq!(total, 4 * 1024 * 1024, "Expected 4MB received");

    Ok(())
}

#[tokio::test]
async fn tcp_throughput() -> anyhow::Result<()> {
    let mut solaya =
        QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;

    solaya
        .run_prog_waiting_for("tcp_bench", "tcp_bench listening on 1234\n")
        .await
        .expect("tcp_bench program must succeed to start");

    let port = solaya.network_port().expect("Network must be enabled");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await?;

    solaya.stdout().assert_read_until("Connection from").await?;

    // Send 4MB of data
    let chunk = vec![0xABu8; 65536];
    let total_to_send: usize = 4 * 1024 * 1024;
    let mut sent = 0;
    while sent < total_to_send {
        let n = chunk.len().min(total_to_send - sent);
        stream.write_all(&chunk[..n]).await?;
        sent += n;
    }
    drop(stream);

    // Read the benchmark result from guest stdout
    solaya.stdout().assert_read_until("BENCH_RESULT: ").await?;
    let result_line = solaya.stdout().assert_read_until("\n").await?;
    let result = String::from_utf8_lossy(&result_line);
    eprintln!("TCP throughput: {}", result.trim());

    assert!(
        result.contains(&format!("bytes={total_to_send}")),
        "Expected all {total_to_send} bytes received, got: {result}"
    );

    Ok(())
}

#[tokio::test]
async fn dhcp() -> anyhow::Result<()> {
    // start_with asserts "dhcpd: configured ip" when network is enabled
    let _solaya = QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;
    Ok(())
}

#[tokio::test]
async fn udp() -> anyhow::Result<()> {
    let mut solaya =
        QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;

    solaya
        .run_prog_waiting_for("udp", "Listening on 1234\n")
        .await
        .expect("udp program must succeed to start");

    let port = solaya.network_port().expect("Network must be enabled");
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    socket.connect(format!("127.0.0.1:{}", port)).await?;

    socket.send("42\n".as_bytes()).await?;
    solaya.stdout().assert_read_until("42\n").await?;

    solaya
        .stdin()
        .write_all("Hello from Solaya!\n".as_bytes())
        .await?;
    solaya.stdin().flush().await?;

    let mut buf = [0; 128];
    let bytes = socket.recv(&mut buf).await?;
    let response = String::from_utf8_lossy(&buf[0..bytes]);

    assert_eq!(response, "Hello from Solaya!\n");

    socket.send("Finalize test\n".as_bytes()).await?;
    solaya.stdout().assert_read_until("Finalize test\n").await?;

    Ok(())
}

#[tokio::test]
async fn tcp_echo() -> anyhow::Result<()> {
    let solaya = QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;

    // tcp_echo is auto-started by init after DHCP
    let port = solaya.network_port().expect("Network must be enabled");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?;

    stream.write_all(b"Hello TCP!").await?;
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).await?;
    assert_eq!(&buf[..n], b"Hello TCP!");

    stream.write_all(b"Second message").await?;
    let n = stream.read(&mut buf).await?;
    assert_eq!(&buf[..n], b"Second message");

    drop(stream);

    Ok(())
}

#[tokio::test]
async fn webserver() -> anyhow::Result<()> {
    let solaya = QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;

    // webserver is auto-started by init after DHCP (listens on port 80)
    let port = solaya.web_port().expect("Web port must be available");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?;

    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let response = String::from_utf8_lossy(&response);

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "Expected 200 OK, got: {response}"
    );
    assert!(response.contains("Content-Type: text/html"));
    assert!(response.contains("<title>Solaya</title>"));

    Ok(())
}

#[tokio::test]
async fn tcp_stress() -> anyhow::Result<()> {
    const BURST_SIZE: usize = 10 * 1024 * 1024;
    const TOTAL: usize = 20 * 1024 * 1024;

    let mut solaya =
        QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;

    solaya
        .run_prog_waiting_for("tcp_stress", "tcp_stress listening on 1234\n")
        .await
        .expect("tcp_stress must succeed to start");

    let port = solaya.network_port().expect("Network must be enabled");
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await?;

    solaya.stdout().assert_read_until("Connection from").await?;

    let (mut reader, mut writer) = stream.into_split();

    let write_task = async {
        let mut buf = vec![0u8; BURST_SIZE];
        let mut counter: u64 = 0;
        let mut sent = 0usize;
        let start = std::time::Instant::now();

        while sent < TOTAL {
            let n = BURST_SIZE.min(TOTAL - sent);
            fill_sequential_u64(&mut buf[..n], &mut counter);
            writer.write_all(&buf[..n]).await?;
            sent += n;

            let elapsed = start.elapsed();
            let mib = sent as f64 / (1024.0 * 1024.0);
            let speed = mib / elapsed.as_secs_f64();
            eprintln!("Host->Guest: {mib:.0} MiB sent ({speed:.1} MiB/s)");
        }
        writer.shutdown().await?;

        let elapsed = start.elapsed();
        let speed = TOTAL as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
        eprintln!(
            "Host->Guest done: {} MiB in {elapsed:?} = {speed:.1} MiB/s",
            TOTAL / (1024 * 1024)
        );
        anyhow::Ok(())
    };

    let read_task = async {
        let mut buf = vec![0u8; BURST_SIZE];
        let mut counter: u64 = 0;
        let mut received = 0usize;
        let mut leftover = Vec::with_capacity(8);
        let start = std::time::Instant::now();
        let mut last_print = 0usize;

        loop {
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            received += n;

            let mut offset = 0;
            if !leftover.is_empty() {
                let need = 8 - leftover.len();
                if n >= need {
                    leftover.extend_from_slice(&buf[..need]);
                    let val = u64::from_le_bytes(leftover[..8].try_into().unwrap());
                    assert_eq!(val, counter, "Host recv: mismatch at counter {counter}");
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
                assert_eq!(val, counter, "Host recv: mismatch at counter {counter}");
                counter += 1;
            }
            if aligned_len < remaining.len() {
                leftover.extend_from_slice(&remaining[aligned_len..]);
            }

            if received - last_print >= BURST_SIZE {
                let elapsed = start.elapsed();
                let mib = received as f64 / (1024.0 * 1024.0);
                let speed = mib / elapsed.as_secs_f64();
                eprintln!("Guest->Host: {mib:.0} MiB received ({speed:.1} MiB/s)");
                last_print = received;
            }
        }

        let elapsed = start.elapsed();
        let speed = received as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
        eprintln!(
            "Guest->Host done: {} MiB in {elapsed:?} = {speed:.1} MiB/s",
            received / (1024 * 1024)
        );

        assert_eq!(received, TOTAL, "Expected 1 GiB received from guest");
        anyhow::Ok(())
    };

    let (write_result, read_result) = tokio::join!(write_task, read_task);
    write_result?;
    read_result?;

    solaya.stdout().assert_read_until("STRESS_DONE").await?;

    Ok(())
}
