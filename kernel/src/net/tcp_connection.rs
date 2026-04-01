use core::{
    net::Ipv4Addr,
    pin::Pin,
    sync::atomic::{AtomicU16, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec::Vec,
};

use crate::{
    debug, info,
    klibc::Spinlock,
    net::{
        arp,
        mac::MacAddress,
        tcp::{FLAG_ACK, FLAG_FIN, FLAG_RST, FLAG_SYN, TcpHeader},
    },
    processes::kernel_tasks,
};

use super::ipv4::IpV4Header;

const WINDOW_SIZE: u16 = 8192;
const MSS: usize = 1460;
const MAX_RETRANSMITS: usize = 5;

static NEXT_EPHEMERAL_PORT: AtomicU16 = AtomicU16::new(49152);

pub fn allocate_ephemeral_port() -> u16 {
    let port = NEXT_EPHEMERAL_PORT.fetch_add(1, Ordering::Relaxed);
    assert!(port >= 49152, "Ephemeral port pool exhausted");
    port
}

fn generate_iss() -> u32 {
    arch::timer::get_current_clocks() as u32
}

fn len_as_seq(len: usize) -> u32 {
    u32::try_from(len).expect("TCP segment length must fit in u32")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ConnectionId {
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
}

struct ReceivedSegment {
    seq: u32,
    ack: u32,
    flags: u16,
    data: Vec<u8>,
}

pub struct TcpConnection {
    id: ConnectionId,
    remote_mac: MacAddress,
    send_seq: u32,
    recv_ack: u32,
    recv_buffer: VecDeque<u8>,
    recv_waker: Option<Waker>,
    segment_mailbox: VecDeque<ReceivedSegment>,
    segment_waker: Option<Waker>,
    established: bool,
    closed: bool,
    user_close_requested: bool,
    send_buffer: VecDeque<u8>,
}

pub type SharedTcpConnection = Arc<Spinlock<TcpConnection>>;

impl TcpConnection {
    fn new(id: ConnectionId, remote_mac: MacAddress, initial_seq: u32) -> Self {
        Self {
            id,
            remote_mac,
            send_seq: initial_seq,
            recv_ack: 0,
            recv_buffer: VecDeque::new(),
            recv_waker: None,
            segment_mailbox: VecDeque::new(),
            segment_waker: None,
            established: false,
            closed: false,
            user_close_requested: false,
            send_buffer: VecDeque::new(),
        }
    }

    fn deliver_segment(&mut self, segment: ReceivedSegment) -> Option<Waker> {
        self.segment_mailbox.push_back(segment);
        self.segment_waker.take()
    }

    pub fn local_port(&self) -> u16 {
        self.id.local_port
    }

    pub fn remote_ip(&self) -> Ipv4Addr {
        self.id.remote_ip
    }

    pub fn remote_port(&self) -> u16 {
        self.id.remote_port
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn recv_data(&mut self, count: usize) -> Vec<u8> {
        let n = count.min(self.recv_buffer.len());
        self.recv_buffer.drain(..n).collect()
    }

    pub fn has_recv_data(&self) -> bool {
        !self.recv_buffer.is_empty()
    }

    pub fn register_recv_waker(&mut self, waker: Waker) {
        self.recv_waker = Some(waker);
    }

    pub fn queue_send_data(&mut self, data: &[u8]) -> Option<Waker> {
        self.send_buffer.extend(data);
        self.segment_waker.take()
    }

    pub fn request_close(&mut self) -> Option<Waker> {
        self.user_close_requested = true;
        self.segment_waker.take()
    }
}

pub struct TcpListener {
    port: u16,
    backlog: VecDeque<SharedTcpConnection>,
    waker: Option<Waker>,
}

pub type SharedTcpListener = Arc<Spinlock<TcpListener>>;

impl TcpListener {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            backlog: VecDeque::new(),
            waker: None,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    fn push_connection(&mut self, conn: SharedTcpConnection) -> Option<Waker> {
        self.backlog.push_back(conn);
        self.waker.take()
    }

    pub fn accept(&mut self) -> Option<SharedTcpConnection> {
        self.backlog.pop_front()
    }

    pub fn register_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }
}

static TCP_CONNECTIONS: Spinlock<BTreeMap<ConnectionId, SharedTcpConnection>> =
    Spinlock::new(BTreeMap::new());
static TCP_LISTENERS: Spinlock<BTreeMap<u16, SharedTcpListener>> = Spinlock::new(BTreeMap::new());

pub fn register_listener(listener: SharedTcpListener) {
    let port = listener.lock().port();
    TCP_LISTENERS.lock().insert(port, listener);
}

pub fn process_tcp_packet(ip_header: &IpV4Header, data: &[u8], source_mac: MacAddress) {
    let (tcp_header, payload) = match TcpHeader::process(data, ip_header) {
        Ok(result) => result,
        Err(e) => {
            debug!("TCP parse error: {:?}", e);
            return;
        }
    };

    let conn_id = ConnectionId {
        local_port: tcp_header.destination_port(),
        remote_ip: ip_header.source_ip,
        remote_port: tcp_header.source_port(),
    };

    let segment = ReceivedSegment {
        seq: tcp_header.sequence_number(),
        ack: tcp_header.acknowledgment_number(),
        flags: tcp_header.flags(),
        data: payload.to_vec(),
    };

    // Try existing connection first
    if let Some(conn) = TCP_CONNECTIONS.lock().get(&conn_id) {
        let waker = conn.lock().deliver_segment(segment);
        if let Some(w) = waker {
            w.wake();
        }
        return;
    }

    // SYN to a listener? Spawn server connection task
    if segment.flags & FLAG_SYN != 0
        && let Some(listener) = TCP_LISTENERS.lock().get(&conn_id.local_port).cloned()
    {
        let iss = generate_iss();
        let conn = Arc::new(Spinlock::new(TcpConnection::new(conn_id, source_mac, iss)));
        TCP_CONNECTIONS.lock().insert(conn_id, conn.clone());
        kernel_tasks::spawn(server_connection_task(conn, segment, listener));
        return;
    }

    // No connection, no listener: send RST
    send_rst(
        ip_header.source_ip,
        source_mac,
        tcp_header.destination_port(),
        tcp_header.source_port(),
        tcp_header.acknowledgment_number(),
        tcp_header.sequence_number().wrapping_add(
            len_as_seq(payload.len()) + if segment.flags & FLAG_SYN != 0 { 1 } else { 0 },
        ),
    );
}

fn send_rst(
    dest_ip: Ipv4Addr,
    dest_mac: MacAddress,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
) {
    let packet = TcpHeader::create_tcp_packet(
        dest_ip,
        dest_mac,
        src_port,
        dst_port,
        seq,
        ack,
        FLAG_RST | FLAG_ACK,
        0,
        &[],
    );
    super::send_packet(packet);
}

struct WaitForSegment {
    conn: SharedTcpConnection,
}

impl Future for WaitForSegment {
    type Output = Option<ReceivedSegment>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut conn = self.conn.lock();
        if let Some(seg) = conn.segment_mailbox.pop_front() {
            return Poll::Ready(Some(seg));
        }
        if conn.user_close_requested || !conn.send_buffer.is_empty() {
            return Poll::Ready(None);
        }
        conn.segment_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

fn wait_for_segment(conn: &SharedTcpConnection) -> WaitForSegment {
    WaitForSegment { conn: conn.clone() }
}

use headers::syscall_types::timespec;

async fn wait_for_segment_or_timeout(
    conn: &SharedTcpConnection,
    seconds: i64,
) -> Option<ReceivedSegment> {
    let timeout = crate::processes::timer::sleep(&timespec {
        tv_sec: seconds,
        tv_nsec: 0,
    })
    .expect("timer must work");

    // Poll both: segment arrival and timeout
    SegmentOrTimeout {
        segment: wait_for_segment(conn),
        timeout,
        done: false,
    }
    .await
}

struct SegmentOrTimeout {
    segment: WaitForSegment,
    timeout: crate::processes::timer::Sleep,
    done: bool,
}

impl Future for SegmentOrTimeout {
    type Output = Option<ReceivedSegment>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }

        if let Poll::Ready(seg) = Pin::new(&mut this.segment).poll(cx) {
            this.done = true;
            return Poll::Ready(seg);
        }

        if let Poll::Ready(()) = Pin::new(&mut this.timeout).poll(cx) {
            this.done = true;
            return Poll::Ready(None);
        }

        Poll::Pending
    }
}

async fn server_connection_task(
    conn: SharedTcpConnection,
    initial_syn: ReceivedSegment,
    listener: SharedTcpListener,
) {
    let (conn_id, iss, recv_ack) = {
        let mut c = conn.lock();
        c.recv_ack = initial_syn.seq.wrapping_add(1);
        let iss = c.send_seq;
        let recv_ack = c.recv_ack;
        (c.id, iss, recv_ack)
    };
    send_data_packet(&conn, FLAG_SYN | FLAG_ACK, iss, recv_ack, &[]);

    // Wait for ACK to complete handshake
    let mut retransmits = 0;
    loop {
        match wait_for_segment_or_timeout(&conn, 1).await {
            Some(seg) => {
                if seg.flags & FLAG_ACK != 0 && seg.ack == iss.wrapping_add(1) {
                    conn.lock().send_seq = iss.wrapping_add(1);
                    break;
                }
                if seg.flags & FLAG_RST != 0 {
                    cleanup_connection(conn_id);
                    return;
                }
            }
            None => {
                retransmits += 1;
                if retransmits >= MAX_RETRANSMITS {
                    info!("TCP server handshake timed out for {:?}", conn_id);
                    cleanup_connection(conn_id);
                    return;
                }
                let recv_ack = conn.lock().recv_ack;
                send_data_packet(&conn, FLAG_SYN | FLAG_ACK, iss, recv_ack, &[]);
            }
        }
    }

    // Established
    let waker = {
        let mut c = conn.lock();
        c.established = true;
        c.recv_waker.take()
    };
    if let Some(w) = waker {
        w.wake();
    }
    let listener_waker = listener.lock().push_connection(conn.clone());
    if let Some(w) = listener_waker {
        w.wake();
    }
    info!("TCP connection established (server) {:?}", conn_id);

    established_loop(&conn).await;
    cleanup_connection(conn_id);
}

fn flush_send_buffer(conn: &SharedTcpConnection) {
    loop {
        let mut c = conn.lock();
        if c.send_buffer.is_empty() {
            break;
        }
        let chunk_len = c.send_buffer.len().min(MSS);
        let data: Vec<u8> = c.send_buffer.drain(..chunk_len).collect();
        let seq = c.send_seq;
        let ack = c.recv_ack;
        c.send_seq = seq.wrapping_add(len_as_seq(data.len()));
        drop(c);
        send_data_packet(conn, FLAG_ACK, seq, ack, &data);
    }
}

fn send_fin(conn: &SharedTcpConnection) {
    let mut c = conn.lock();
    let seq = c.send_seq;
    let ack = c.recv_ack;
    c.send_seq = seq.wrapping_add(1);
    drop(c);
    send_data_packet(conn, FLAG_FIN | FLAG_ACK, seq, ack, &[]);
}

async fn established_loop(conn: &SharedTcpConnection) {
    loop {
        flush_send_buffer(conn);

        match wait_for_segment(conn).await {
            Some(seg) => {
                if seg.flags & FLAG_RST != 0 {
                    let waker = {
                        let mut c = conn.lock();
                        c.closed = true;
                        c.recv_waker.take()
                    };
                    if let Some(w) = waker {
                        w.wake();
                    }
                    return;
                }

                let (send_ack, waker, do_fin_ack, do_user_close) = {
                    let mut c = conn.lock();

                    let mut need_ack = false;
                    let mut waker = None;

                    // Process incoming data (drop out-of-order per minimal TCP)
                    if !seg.data.is_empty() && seg.seq == c.recv_ack {
                        c.recv_ack = c.recv_ack.wrapping_add(len_as_seq(seg.data.len()));
                        c.recv_buffer.extend(&seg.data);
                        waker = c.recv_waker.take();
                        need_ack = true;
                    }

                    // Process FIN
                    if seg.flags & FLAG_FIN != 0 {
                        c.recv_ack = c.recv_ack.wrapping_add(1);
                        c.closed = true;
                        waker = waker.or_else(|| c.recv_waker.take());
                        let ack_info = Some((c.send_seq, c.recv_ack));
                        (ack_info, waker, true, false)
                    } else if c.user_close_requested {
                        (
                            if need_ack {
                                Some((c.send_seq, c.recv_ack))
                            } else {
                                None
                            },
                            waker,
                            false,
                            true,
                        )
                    } else {
                        (
                            if need_ack {
                                Some((c.send_seq, c.recv_ack))
                            } else {
                                None
                            },
                            waker,
                            false,
                            false,
                        )
                    }
                };

                // All waker.wake() and send_packet calls happen outside the lock
                if let Some(w) = waker {
                    w.wake();
                }
                if let Some((seq, ack)) = send_ack {
                    send_data_packet(conn, FLAG_ACK, seq, ack, &[]);
                }
                if do_fin_ack {
                    return;
                }
                if do_user_close {
                    flush_send_buffer(conn);
                    send_fin(conn);
                    wait_for_fin_ack(conn).await;
                    conn.lock().closed = true;
                    return;
                }
            }
            None => {
                if conn.lock().user_close_requested {
                    flush_send_buffer(conn);
                    send_fin(conn);
                    wait_for_fin_ack(conn).await;
                    conn.lock().closed = true;
                    return;
                }
            }
        }
    }
}

fn send_data_packet(conn: &SharedTcpConnection, flags: u16, seq: u32, ack: u32, data: &[u8]) {
    let c = conn.lock();
    let packet = TcpHeader::create_tcp_packet(
        c.id.remote_ip,
        c.remote_mac,
        c.id.local_port,
        c.id.remote_port,
        seq,
        ack,
        flags,
        WINDOW_SIZE,
        data,
    );
    drop(c);
    super::send_packet(packet);
}

async fn wait_for_fin_ack(conn: &SharedTcpConnection) {
    for _ in 0..MAX_RETRANSMITS {
        match wait_for_segment_or_timeout(conn, 1).await {
            Some(seg) => {
                if seg.flags & FLAG_ACK != 0 {
                    if seg.flags & FLAG_FIN != 0 {
                        let (seq, ack) = {
                            let mut c = conn.lock();
                            c.recv_ack = c.recv_ack.wrapping_add(1);
                            (c.send_seq, c.recv_ack)
                        };
                        send_data_packet(conn, FLAG_ACK, seq, ack, &[]);
                    }
                    return;
                }
                if seg.flags & FLAG_RST != 0 {
                    return;
                }
            }
            None => {
                let (seq, ack) = {
                    let c = conn.lock();
                    (c.send_seq.wrapping_sub(1), c.recv_ack)
                };
                send_data_packet(conn, FLAG_FIN | FLAG_ACK, seq, ack, &[]);
            }
        }
    }
}

fn cleanup_connection(id: ConnectionId) {
    TCP_CONNECTIONS.lock().remove(&id);
    debug!("TCP connection cleaned up: {:?}", id);
}

// Public interface for syscalls

pub fn create_listener(port: u16) -> SharedTcpListener {
    Arc::new(Spinlock::new(TcpListener::new(port)))
}

pub async fn initiate_connect(
    local_port: u16,
    dest_ip: Ipv4Addr,
    dest_port: u16,
) -> Option<SharedTcpConnection> {
    let dest_mac = arp::cache_lookup(&dest_ip)?;
    let iss = generate_iss();

    let conn_id = ConnectionId {
        local_port,
        remote_ip: dest_ip,
        remote_port: dest_port,
    };

    let conn = Arc::new(Spinlock::new(TcpConnection::new(conn_id, dest_mac, iss)));
    TCP_CONNECTIONS.lock().insert(conn_id, conn.clone());

    // Send SYN
    send_data_packet(&conn, FLAG_SYN, iss, 0, &[]);

    // Wait for SYN-ACK
    let mut retransmits = 0;
    loop {
        match wait_for_segment_or_timeout(&conn, 1).await {
            Some(seg) => {
                if seg.flags & FLAG_SYN != 0 && seg.flags & FLAG_ACK != 0 {
                    let (seq, ack) = {
                        let mut c = conn.lock();
                        c.recv_ack = seg.seq.wrapping_add(1);
                        c.send_seq = iss.wrapping_add(1);
                        c.established = true;
                        (c.send_seq, c.recv_ack)
                    };
                    send_data_packet(&conn, FLAG_ACK, seq, ack, &[]);
                    info!("TCP connection established (client) {:?}", conn_id);
                    let conn_for_task = conn.clone();
                    kernel_tasks::spawn(async move {
                        established_loop(&conn_for_task).await;
                        cleanup_connection(conn_id);
                    });
                    return Some(conn);
                }
                if seg.flags & FLAG_RST != 0 {
                    cleanup_connection(conn_id);
                    return None;
                }
            }
            None => {
                retransmits += 1;
                if retransmits >= MAX_RETRANSMITS {
                    cleanup_connection(conn_id);
                    return None;
                }
                send_data_packet(&conn, FLAG_SYN, iss, 0, &[]);
            }
        }
    }
}

pub struct WaitForAccept {
    listener: SharedTcpListener,
}

impl Future for WaitForAccept {
    type Output = SharedTcpConnection;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut l = self.listener.lock();
        if let Some(conn) = l.accept() {
            return Poll::Ready(conn);
        }
        l.register_waker(cx.waker().clone());
        Poll::Pending
    }
}

pub fn wait_for_accept(listener: &SharedTcpListener) -> WaitForAccept {
    WaitForAccept {
        listener: listener.clone(),
    }
}

pub struct WaitForRecvData {
    conn: SharedTcpConnection,
    count: usize,
}

impl Future for WaitForRecvData {
    type Output = Vec<u8>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let count = self.count;
        let mut c = self.conn.lock();
        if c.has_recv_data() {
            return Poll::Ready(c.recv_data(count));
        }
        if c.is_closed() {
            return Poll::Ready(Vec::new());
        }
        c.register_recv_waker(cx.waker().clone());
        Poll::Pending
    }
}

pub fn wait_for_recv_data(conn: &SharedTcpConnection, count: usize) -> WaitForRecvData {
    WaitForRecvData {
        conn: conn.clone(),
        count,
    }
}
