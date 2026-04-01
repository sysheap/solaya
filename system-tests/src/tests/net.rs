use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::infra::qemu::{QemuInstance, QemuOptions};

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
    let mut solaya =
        QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;

    solaya
        .run_prog_waiting_for("tcp_echo", "TCP listening on 1234\n")
        .await
        .expect("tcp_echo program must succeed to start");

    let port = solaya.network_port().expect("Network must be enabled");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?;

    solaya.stdout().assert_read_until("Connection from").await?;

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
    let mut solaya =
        QemuInstance::start_with(QemuOptions::default().add_network_card(true)).await?;

    solaya
        .run_prog_waiting_for("webserver", "HTTP listening on 1234\n")
        .await
        .expect("webserver must succeed to start");

    let port = solaya.network_port().expect("Network must be enabled");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?;

    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await?;

    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let response = String::from_utf8_lossy(&buf[..n]);

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "Expected 200 OK, got {n} bytes: {response}"
    );
    assert!(response.contains("Content-Type: text/html"));
    assert!(response.contains("<title>Solaya</title>"));

    Ok(())
}
