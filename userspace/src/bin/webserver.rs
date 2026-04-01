use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

const PORT: u16 = 1234;
const INDEX_HTML: &str = include_str!("../../static/index.html");

fn handle_connection(mut stream: std::net::TcpStream) {
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };

    let request = String::from_utf8_lossy(&buf[..n]);
    let (status, body) = if request.starts_with("GET ") {
        ("200 OK", INDEX_HTML)
    } else {
        ("400 Bad Request", "Bad Request")
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn main() {
    let listener = TcpListener::bind(format!("0.0.0.0:{PORT}")).expect("bind must work");
    println!("HTTP listening on {PORT}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(|| handle_connection(stream));
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}
