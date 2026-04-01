use std::{net::UdpSocket, os::fd::AsRawFd};

use common::ioctl::{get_mac_address, set_ip_address};

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;

const BOOTREQUEST: u8 = 1;
const BOOTREPLY: u8 = 2;
const HTYPE_ETHERNET: u8 = 1;
const HLEN_ETHERNET: u8 = 6;

const DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

const OPT_SUBNET_MASK: u8 = 1;
const OPT_ROUTER: u8 = 3;
const OPT_MESSAGE_TYPE: u8 = 53;
const OPT_SERVER_ID: u8 = 54;
const OPT_REQUESTED_IP: u8 = 50;
const OPT_PARAM_REQUEST: u8 = 55;
const OPT_END: u8 = 255;

const DHCPDISCOVER: u8 = 1;
const DHCPOFFER: u8 = 2;
const DHCPREQUEST: u8 = 3;
const DHCPACK: u8 = 5;

const DHCP_HEADER_SIZE: usize = 236;

fn build_discover(mac: &[u8; 6], xid: u32) -> Vec<u8> {
    let mut pkt = vec![0u8; DHCP_HEADER_SIZE];
    pkt[0] = BOOTREQUEST;
    pkt[1] = HTYPE_ETHERNET;
    pkt[2] = HLEN_ETHERNET;
    // hops = 0
    pkt[4..8].copy_from_slice(&xid.to_be_bytes());
    // flags: broadcast
    pkt[10] = 0x80;
    // chaddr
    pkt[28..34].copy_from_slice(mac);

    // Options
    pkt.extend_from_slice(&DHCP_MAGIC_COOKIE);
    // Message type = DISCOVER
    pkt.extend_from_slice(&[OPT_MESSAGE_TYPE, 1, DHCPDISCOVER]);
    // Parameter request list
    pkt.extend_from_slice(&[OPT_PARAM_REQUEST, 2, OPT_SUBNET_MASK, OPT_ROUTER]);
    // End
    pkt.push(OPT_END);

    pkt
}

fn build_request(mac: &[u8; 6], xid: u32, offered_ip: [u8; 4], server_id: [u8; 4]) -> Vec<u8> {
    let mut pkt = vec![0u8; DHCP_HEADER_SIZE];
    pkt[0] = BOOTREQUEST;
    pkt[1] = HTYPE_ETHERNET;
    pkt[2] = HLEN_ETHERNET;
    pkt[4..8].copy_from_slice(&xid.to_be_bytes());
    // flags: broadcast
    pkt[10] = 0x80;
    // chaddr
    pkt[28..34].copy_from_slice(mac);

    // Options
    pkt.extend_from_slice(&DHCP_MAGIC_COOKIE);
    // Message type = REQUEST
    pkt.extend_from_slice(&[OPT_MESSAGE_TYPE, 1, DHCPREQUEST]);
    // Requested IP
    pkt.extend_from_slice(&[OPT_REQUESTED_IP, 4]);
    pkt.extend_from_slice(&offered_ip);
    // Server ID
    pkt.extend_from_slice(&[OPT_SERVER_ID, 4]);
    pkt.extend_from_slice(&server_id);
    // End
    pkt.push(OPT_END);

    pkt
}

struct DhcpResponse {
    msg_type: u8,
    yiaddr: [u8; 4],
    server_id: [u8; 4],
}

fn parse_response(data: &[u8], expected_xid: u32) -> Option<DhcpResponse> {
    if data.len() < DHCP_HEADER_SIZE + 4 {
        return None;
    }
    if data[0] != BOOTREPLY {
        return None;
    }
    let xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if xid != expected_xid {
        return None;
    }

    let mut yiaddr = [0u8; 4];
    yiaddr.copy_from_slice(&data[16..20]);

    // Parse options after magic cookie
    let opts_start = DHCP_HEADER_SIZE + 4;
    if data[DHCP_HEADER_SIZE..opts_start] != DHCP_MAGIC_COOKIE {
        return None;
    }

    let mut msg_type = 0u8;
    let mut server_id = [0u8; 4];
    let mut i = opts_start;
    while i < data.len() {
        let opt = data[i];
        if opt == OPT_END {
            break;
        }
        if opt == 0 {
            // Padding
            i += 1;
            continue;
        }
        if i + 1 >= data.len() {
            break;
        }
        let len = data[i + 1] as usize;
        let val_start = i + 2;
        if val_start + len > data.len() {
            break;
        }
        match opt {
            OPT_MESSAGE_TYPE if len == 1 => msg_type = data[val_start],
            OPT_SERVER_ID if len == 4 => {
                server_id.copy_from_slice(&data[val_start..val_start + 4]);
            }
            _ => {}
        }
        i = val_start + len;
    }

    Some(DhcpResponse {
        msg_type,
        yiaddr,
        server_id,
    })
}

fn main() {
    let socket = match UdpSocket::bind(format!("0.0.0.0:{DHCP_CLIENT_PORT}")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dhcpd: bind failed: {e}");
            std::process::exit(2);
        }
    };

    let mac = match get_mac_address(socket.as_raw_fd()) {
        Some(m) => m,
        None => {
            eprintln!("dhcpd: no network device");
            std::process::exit(2);
        }
    };
    let xid: u32 = 0x12345678;

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(3));
        eprintln!("dhcpd: no response within 3s");
        std::process::exit(1);
    });

    // Send DISCOVER
    let discover = build_discover(&mac, xid);
    if let Err(e) = socket.send_to(&discover, format!("255.255.255.255:{DHCP_SERVER_PORT}")) {
        eprintln!("dhcpd: send failed: {e}");
        std::process::exit(1);
    }

    // Receive OFFER
    let mut buf = [0u8; 1024];
    let (n, _) = socket
        .recv_from(&mut buf)
        .expect("dhcpd: recv offer failed");
    let offer = parse_response(&buf[..n], xid).expect("dhcpd: invalid offer");
    assert!(offer.msg_type == DHCPOFFER, "dhcpd: expected OFFER");

    // Send REQUEST
    let request = build_request(&mac, xid, offer.yiaddr, offer.server_id);
    socket
        .send_to(&request, format!("255.255.255.255:{DHCP_SERVER_PORT}"))
        .expect("dhcpd: send request failed");

    // Receive ACK
    let (n, _) = socket.recv_from(&mut buf).expect("dhcpd: recv ack failed");
    let ack = parse_response(&buf[..n], xid).expect("dhcpd: invalid ack");
    assert!(ack.msg_type == DHCPACK, "dhcpd: expected ACK");

    // Configure IP
    set_ip_address(socket.as_raw_fd(), ack.yiaddr);

    let ip = ack.yiaddr;
    println!(
        "dhcpd: configured ip {}.{}.{}.{}",
        ip[0], ip[1], ip[2], ip[3]
    );
    std::process::exit(0);
}
