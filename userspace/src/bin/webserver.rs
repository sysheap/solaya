use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    net::{Shutdown, TcpListener, TcpStream},
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
    thread,
    time::Duration,
};

const PORT: u16 = 1234;
const INDEX_HTML: &str = include_str!("../../static/index.html");
const DOOM_HTML: &str = include_str!("../../static/doom.html");
const FB_SIZE: usize = 640 * 480 * 4;
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

// --- SHA-1 (RFC 3174) ---

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

// --- Base64 encode ---

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < data.len() {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < data.len() {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

// --- WebSocket framing ---

fn send_ws_frame(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    let mut header = Vec::with_capacity(10);
    header.push(0x82); // FIN + binary opcode
    if payload.len() < 126 {
        header.push(payload.len() as u8);
    } else if payload.len() <= 65535 {
        header.push(126);
        header.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    stream.write_all(&header)?;
    stream.write_all(payload)?;
    Ok(())
}

fn read_ws_frame(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut hdr = [0u8; 2];
    if stream.read_exact(&mut hdr).is_err() {
        return None;
    }

    let opcode = hdr[0] & 0x0F;
    if opcode == 0x08 {
        return None; // close frame
    }

    let masked = hdr[1] & 0x80 != 0;
    let len_byte = (hdr[1] & 0x7F) as usize;

    let payload_len = if len_byte < 126 {
        len_byte
    } else if len_byte == 126 {
        let mut buf = [0u8; 2];
        if stream.read_exact(&mut buf).is_err() {
            return None;
        }
        u16::from_be_bytes(buf) as usize
    } else {
        let mut buf = [0u8; 8];
        if stream.read_exact(&mut buf).is_err() {
            return None;
        }
        u64::from_be_bytes(buf) as usize
    };

    let mask_key = if masked {
        let mut mk = [0u8; 4];
        if stream.read_exact(&mut mk).is_err() {
            return None;
        }
        Some(mk)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len];
    if stream.read_exact(&mut payload).is_err() {
        return None;
    }

    if let Some(mk) = mask_key {
        for i in 0..payload.len() {
            payload[i] ^= mk[i % 4];
        }
    }

    Some(payload)
}

// --- HTTP helpers ---

fn serve_static(mut stream: TcpStream, body: &str, content_type: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.shutdown(Shutdown::Write);
}

fn serve_error(mut stream: TcpStream, status: &str) {
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.shutdown(Shutdown::Write);
}

// --- WebSocket handshake and streaming ---

fn ws_accept_key(client_key: &str) -> String {
    let mut input = client_key.to_string();
    input.push_str(WS_GUID);
    base64_encode(&sha1(input.as_bytes()))
}

fn handle_websocket(mut stream: TcpStream, client_key: &str) {
    let accept = ws_accept_key(client_key);
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if stream.write_all(response.as_bytes()).is_err() {
        return;
    }

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let fb_path = format!("/tmp/doom_fb_{id}");
    let key_path = format!("/tmp/doom_keys_{id}");

    // Pre-fill frame file with black pixels (alpha=255 for RGBA)
    {
        let mut fb_file = match File::create(&fb_path) {
            Ok(f) => f,
            Err(_) => return,
        };
        let black = vec![0u8; FB_SIZE];
        let _ = fb_file.write_all(&black);
    }

    // Create empty key input file
    if File::create(&key_path).is_err() {
        let _ = fs::remove_file(&fb_path);
        return;
    }

    // Spawn DOOM
    let mut child = match Command::new("/doom")
        .env("DOOM_FB_PATH", &fb_path)
        .env("DOOM_INPUT_PATH", &key_path)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to spawn doom: {e}");
            let _ = fs::remove_file(&fb_path);
            let _ = fs::remove_file(&key_path);
            return;
        }
    };

    eprintln!("DOOM instance {id} started (pid={})", child.id());

    // Reader thread: receive key events from browser, write to key file
    let mut read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => {
            let _ = child.kill();
            let _ = fs::remove_file(&fb_path);
            let _ = fs::remove_file(&key_path);
            return;
        }
    };
    let key_path_clone = key_path.clone();
    let reader_handle = thread::spawn(move || {
        let mut key_file = match OpenOptions::new().append(true).open(&key_path_clone) {
            Ok(f) => f,
            Err(_) => return,
        };
        loop {
            match read_ws_frame(&mut read_stream) {
                Some(data) if data.len() == 2 => {
                    let _ = key_file.write_all(&data);
                }
                Some(_) => {} // ignore unexpected messages
                None => break,
            }
        }
    });

    // Writer loop: read framebuffer file, convert BGRA→RGBA, send via WebSocket
    let mut fb = match File::open(&fb_path) {
        Ok(f) => f,
        Err(_) => {
            let _ = child.kill();
            let _ = fs::remove_file(&fb_path);
            let _ = fs::remove_file(&key_path);
            return;
        }
    };
    let mut buf = vec![0u8; FB_SIZE];

    loop {
        if fb.seek(SeekFrom::Start(0)).is_err() {
            break;
        }
        if fb.read_exact(&mut buf).is_err() {
            // File might not be fully written yet, send what we have as black
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        // Convert BGRA → RGBA and set alpha to 255
        for chunk in buf.chunks_exact_mut(4) {
            chunk.swap(0, 2);
            chunk[3] = 255;
        }

        if send_ws_frame(&mut stream, &buf).is_err() {
            break;
        }

        thread::sleep(Duration::from_millis(33));
    }

    // Cleanup
    eprintln!("DOOM instance {id} shutting down");
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader_handle.join();
    let _ = fs::remove_file(&fb_path);
    let _ = fs::remove_file(&key_path);
}

// --- Request parsing and routing ---

fn find_header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    for line in request.lines() {
        if let Some(value) = line.strip_prefix(name) {
            if let Some(value) = value.strip_prefix(": ") {
                return Some(value.trim());
            }
            if let Some(value) = value.strip_prefix(':') {
                return Some(value.trim());
            }
        }
    }
    None
}

fn handle_connection(mut stream: TcpStream) {
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };

    let request = String::from_utf8_lossy(&buf[..n]);

    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    let (method, path) = if parts.len() >= 2 {
        (parts[0], parts[1])
    } else {
        ("", "")
    };

    // Check for WebSocket upgrade
    let is_upgrade = find_header(&request, "Upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    if is_upgrade && method == "GET" {
        if let Some(key) = find_header(&request, "Sec-WebSocket-Key") {
            let key = key.to_string();
            handle_websocket(stream, &key);
            return;
        }
    }

    match (method, path) {
        ("GET", "/") => serve_static(stream, INDEX_HTML, "text/html"),
        ("GET", "/doom") => serve_static(stream, DOOM_HTML, "text/html"),
        _ => serve_error(stream, "404 Not Found"),
    }
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
