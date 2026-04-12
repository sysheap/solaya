use core::{
    cell::LazyCell,
    net::Ipv4Addr,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{sync::Arc, vec::Vec};
use driver_api::NetDevice;

use crate::{
    debug,
    net::{ipv4::IpV4Header, udp::UdpHeader},
};
use hal::spinlock::Spinlock;

use self::{
    ethernet::EthernetHeader,
    ipv4::{PROTOCOL_TCP, PROTOCOL_UDP},
    mac::MacAddress,
    sockets::OpenSockets,
};

pub mod arp;
mod checksum;
mod ethernet;
mod ipv4;
pub mod mac;
pub mod sockets;
pub mod tcp;
pub mod tcp_connection;
pub mod udp;

/// Bytes reserved at the start of packet buffers for the driver-level header
/// (e.g., virtio_net_hdr). Avoids a second allocation when the driver prepends
/// its header.
pub const DRIVER_HEADER_RESERVE: usize = 12;

struct NetworkStack {
    ip_addr: Spinlock<Ipv4Addr>,
    open_sockets: Spinlock<LazyCell<OpenSockets>>,
}

static NETWORK_STACK: NetworkStack = NetworkStack {
    ip_addr: Spinlock::new(Ipv4Addr::new(0, 0, 0, 0)),
    open_sockets: Spinlock::new(LazyCell::new(OpenSockets::new)),
};

/// The primary device used by the network stack — the first one registered
/// by the driver layer. The `NetDeviceRegistry` holds all devices, this is
/// just the one the stack binds to.
fn primary_device() -> Option<Arc<dyn NetDevice>> {
    crate::drivers::registry::<dyn NetDevice>().get(0)
}

static NETWORK_INTERRUPT_COUNTER: AtomicU64 = AtomicU64::new(0);
static NETWORK_INTERRUPT_WAKERS: Spinlock<Vec<Waker>> = Spinlock::new(Vec::new());

/// Called by per-driver IRQ handlers after they've acknowledged the device's
/// own ISR register. Bumps the RX wake counter and drains pending wakers so
/// `network_rx_task` can run.
pub fn notify_packet_arrival() {
    NETWORK_INTERRUPT_COUNTER.fetch_add(1, Ordering::SeqCst);
    let wakers: Vec<Waker> = NETWORK_INTERRUPT_WAKERS.lock().drain(..).collect();
    for waker in wakers {
        waker.wake();
    }
}

struct NetworkInterruptWait {
    seen_counter: u64,
    registered: bool,
}

impl NetworkInterruptWait {
    fn new(seen_counter: u64) -> Self {
        Self {
            seen_counter,
            registered: false,
        }
    }
}

impl Future for NetworkInterruptWait {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current = NETWORK_INTERRUPT_COUNTER.load(Ordering::SeqCst);
        if current != self.seen_counter {
            return Poll::Ready(());
        }
        if !self.registered {
            NETWORK_INTERRUPT_WAKERS.lock().push(cx.waker().clone());
            self.registered = true;
            // Double-check after registering to prevent lost wakeups
            let current = NETWORK_INTERRUPT_COUNTER.load(Ordering::SeqCst);
            if current != self.seen_counter {
                return Poll::Ready(());
            }
        }
        Poll::Pending
    }
}

pub async fn network_rx_task() {
    loop {
        let seen = NETWORK_INTERRUPT_COUNTER.load(Ordering::SeqCst);
        let count = receive_and_process_packets();
        if count > 0 {
            sockets::wake_socket_waiters();
        }
        NetworkInterruptWait::new(seen).await;
    }
}

pub fn ip_addr() -> Ipv4Addr {
    *NETWORK_STACK.ip_addr.lock()
}

pub fn set_ip_addr(addr: Ipv4Addr) {
    *NETWORK_STACK.ip_addr.lock() = addr;
}

pub fn has_network_device() -> bool {
    crate::drivers::registry::<dyn NetDevice>().len() > 0
}

pub fn open_sockets() -> &'static Spinlock<LazyCell<OpenSockets>> {
    &NETWORK_STACK.open_sockets
}

fn receive_and_process_packets() -> usize {
    let device = primary_device().expect("There must be a configured network device.");
    let packets = device.receive();
    let count = packets.len();
    for packet in packets {
        process_packet(packet);
    }
    count
}

pub fn send_packet(packet: Vec<u8>) {
    primary_device()
        .expect("There must be a configured network device.")
        .send(packet);
}

pub fn current_mac_address() -> MacAddress {
    primary_device()
        .expect("MAC address must be cached after device init")
        .mac()
}

fn process_packet(packet: Vec<u8>) {
    let (ethernet_header, rest) = match EthernetHeader::try_parse(&packet) {
        Ok(p) => p,
        Err(err) => {
            debug!("Could not parse ethernet header: {:?}", err);
            return;
        }
    };

    debug!("Received ethernet packet: {}", ethernet_header);

    let ether_type = ethernet_header.ether_type();

    match ether_type {
        ethernet::EtherTypes::Arp => arp::process_and_respond(rest),
        ethernet::EtherTypes::IPv4 => process_ipv4_packet(rest, ethernet_header.source_mac()),
    }
}

fn process_ipv4_packet(data: &[u8], source_mac: MacAddress) {
    let (ipv4_header, rest) = match IpV4Header::process(data) {
        Ok(result) => result,
        Err(err) => {
            debug!("Dropping IPv4 packet: {:?}", err);
            return;
        }
    };
    arp::cache_insert(ipv4_header.source_ip, source_mac);

    match ipv4_header.upper_protocol.get() {
        PROTOCOL_UDP => {
            let (udp_header, data) =
                UdpHeader::process(rest, ipv4_header).expect("Udp header must be valid.");
            open_sockets().lock().put_data(
                ipv4_header.source_ip,
                sockets::Port::new(udp_header.source_port()),
                sockets::Port::new(udp_header.destination_port()),
                data,
            );
        }
        PROTOCOL_TCP => {
            tcp_connection::process_tcp_packet(ipv4_header, rest, source_mac);
        }
        proto => {
            debug!("Unsupported IP protocol: {}", proto);
        }
    }
}
