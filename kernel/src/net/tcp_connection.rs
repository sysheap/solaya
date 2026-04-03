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
        tcp::{FLAG_ACK, FLAG_FIN, FLAG_RST, FLAG_SYN, TcpHeader, TcpOptions, build_syn_options},
    },
    processes::kernel_tasks,
};

use super::ipv4::IpV4Header;

use headers::syscall_types::timespec;

struct TcpStats {
    bytes_sent: u64,
    bytes_received: u64,
    packets_sent: u64,
    packets_received: u64,
    flushes: u64,
    segments_flushed: u64,
    start_time: timespec,
}

impl TcpStats {
    fn new() -> Self {
        Self {
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            flushes: 0,
            segments_flushed: 0,
            start_time: crate::processes::timer::current_time(),
        }
    }
}

const MSS: usize = 1460;
const MAX_RETRANSMITS: usize = 5;
const WINDOW_SCALE_SHIFT: u8 = 7;

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
    window_size: u16,
    options: TcpOptions,
    data: Vec<u8>,
}

const MAX_SEND_BUFFER: usize = 512 * 1024;
const MAX_RECV_BUFFER: usize = 512 * 1024;
const MAX_SEGMENTS_PER_FLUSH: usize = 128;
const RETRANSMIT_TIMEOUT_MS: i64 = 200;

pub struct TcpConnection {
    id: ConnectionId,
    remote_mac: MacAddress,
    /// Next sequence number to send (covers sent-but-unacked + unsent boundary)
    send_nxt: u32,
    /// Oldest unacknowledged sequence number
    send_una: u32,
    recv_ack: u32,
    recv_buffer: VecDeque<u8>,
    recv_waker: Option<Waker>,
    segment_mailbox: VecDeque<ReceivedSegment>,
    segment_waker: Option<Waker>,
    send_space_waker: Option<Waker>,
    established: bool,
    closed: bool,
    user_close_requested: bool,
    /// Holds ALL unacked + unsent data. [0..nxt_offset) = sent-but-unacked, [nxt_offset..) = unsent.
    /// nxt_offset = send_nxt - send_una. ACKs drain from front; new data appended at end.
    send_buffer: VecDeque<u8>,
    remote_window: u32,
    send_window_scale: u8,
    recv_window_scale: u8,
    window_update_needed: bool,
    stats: TcpStats,
}

pub type SharedTcpConnection = Arc<Spinlock<TcpConnection>>;

impl TcpConnection {
    fn new(id: ConnectionId, remote_mac: MacAddress, initial_seq: u32) -> Self {
        Self {
            id,
            remote_mac,
            send_nxt: initial_seq,
            send_una: initial_seq,
            recv_ack: 0,
            recv_buffer: VecDeque::new(),
            recv_waker: None,
            segment_mailbox: VecDeque::new(),
            segment_waker: None,
            send_space_waker: None,
            established: false,
            closed: false,
            user_close_requested: false,
            send_buffer: VecDeque::new(),
            remote_window: 65535,
            send_window_scale: 0,
            recv_window_scale: 0,
            window_update_needed: false,
            stats: TcpStats::new(),
        }
    }

    fn advertised_window(&self) -> u16 {
        let available = MAX_RECV_BUFFER.saturating_sub(self.recv_buffer.len());
        let scaled = available >> self.recv_window_scale;
        scaled.min(65535) as u16
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

    pub fn recv_data(&mut self, count: usize) -> (Vec<u8>, Option<Waker>) {
        let n = count.min(self.recv_buffer.len());
        let data: Vec<u8> = self.recv_buffer.drain(..n).collect();
        let waker = if n > 0 && self.recv_buffer.len() < MAX_RECV_BUFFER / 2 {
            self.window_update_needed = true;
            self.segment_waker.take()
        } else {
            None
        };
        (data, waker)
    }

    pub fn has_recv_data(&self) -> bool {
        !self.recv_buffer.is_empty()
    }

    pub fn register_recv_waker(&mut self, waker: Waker) {
        self.recv_waker = Some(waker);
    }

    pub fn queue_send_data(&mut self, data: &[u8]) -> Option<Waker> {
        assert!(
            self.send_buffer.len() + data.len() <= MAX_SEND_BUFFER,
            "Send buffer overflow: caller must check send_buffer_space()"
        );
        self.send_buffer.extend(data);
        self.segment_waker.take()
    }

    fn apply_ack(&mut self, seg: &ReceivedSegment) {
        let acked_to = seg.ack;
        let una = self.send_una;
        let bytes_acked = acked_to.wrapping_sub(una);
        if bytes_acked > 0 && bytes_acked <= self.send_nxt.wrapping_sub(una) {
            let drain_count = bytes_acked as usize;
            self.send_buffer.drain(..drain_count);
            self.send_una = acked_to;
        }
        self.remote_window = (seg.window_size as u32) << self.send_window_scale;
    }

    pub fn send_buffer_space(&self) -> usize {
        MAX_SEND_BUFFER.saturating_sub(self.send_buffer.len())
    }

    pub fn register_send_space_waker(&mut self, waker: Waker) {
        self.send_space_waker = Some(waker);
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
    let (tcp_header, options, payload) = match TcpHeader::process(data, ip_header) {
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
        window_size: tcp_header.window_size(),
        options,
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
        &[],
    );
    super::send_packet(packet);
}

enum TcpEvent {
    Segment(ReceivedSegment),
    Timeout,
    ReadyToSend,
    WindowUpdate,
    UserClose,
}

struct WaitForEvent {
    conn: SharedTcpConnection,
    timeout: crate::processes::timer::Sleep,
    done: bool,
}

impl Future for WaitForEvent {
    type Output = TcpEvent;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<TcpEvent> {
        let this = self.get_mut();
        if this.done {
            return Poll::Pending;
        }
        let mut conn = this.conn.lock();
        if let Some(seg) = conn.segment_mailbox.pop_front() {
            this.done = true;
            return Poll::Ready(TcpEvent::Segment(seg));
        }
        if conn.user_close_requested {
            this.done = true;
            return Poll::Ready(TcpEvent::UserClose);
        }
        if conn.window_update_needed {
            conn.window_update_needed = false;
            this.done = true;
            return Poll::Ready(TcpEvent::WindowUpdate);
        }
        let nxt_offset = conn.send_nxt.wrapping_sub(conn.send_una) as usize;
        let has_unsent = conn.send_buffer.len() > nxt_offset;
        if has_unsent {
            if nxt_offset < conn.remote_window as usize {
                this.done = true;
                return Poll::Ready(TcpEvent::ReadyToSend);
            }
            if conn.remote_window == 0 {
                this.done = true;
                return Poll::Ready(TcpEvent::ReadyToSend);
            }
        }
        conn.segment_waker = Some(cx.waker().clone());
        drop(conn);
        if Pin::new(&mut this.timeout).poll(cx).is_ready() {
            this.done = true;
            return Poll::Ready(TcpEvent::Timeout);
        }
        Poll::Pending
    }
}

fn wait_for_event(conn: &SharedTcpConnection) -> WaitForEvent {
    let timeout = crate::processes::timer::sleep(&timespec {
        tv_sec: 0,
        tv_nsec: RETRANSMIT_TIMEOUT_MS * 1_000_000,
    })
    .expect("timer must work");
    WaitForEvent {
        conn: conn.clone(),
        timeout,
        done: false,
    }
}

struct WaitForMailboxOnly {
    conn: SharedTcpConnection,
}

impl Future for WaitForMailboxOnly {
    type Output = ReceivedSegment;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut conn = self.conn.lock();
        if let Some(seg) = conn.segment_mailbox.pop_front() {
            return Poll::Ready(seg);
        }
        conn.segment_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

async fn wait_for_mailbox_or_timeout(
    conn: &SharedTcpConnection,
    seconds: i64,
) -> Option<ReceivedSegment> {
    let timeout = crate::processes::timer::sleep(&timespec {
        tv_sec: seconds,
        tv_nsec: 0,
    })
    .expect("timer must work");

    MailboxOrTimeout {
        mailbox: WaitForMailboxOnly { conn: conn.clone() },
        timeout,
        done: false,
    }
    .await
}

struct MailboxOrTimeout {
    mailbox: WaitForMailboxOnly,
    timeout: crate::processes::timer::Sleep,
    done: bool,
}

impl Future for MailboxOrTimeout {
    type Output = Option<ReceivedSegment>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        if let Poll::Ready(seg) = Pin::new(&mut this.mailbox).poll(cx) {
            this.done = true;
            return Poll::Ready(Some(seg));
        }
        if let Poll::Ready(()) = Pin::new(&mut this.timeout).poll(cx) {
            this.done = true;
            return Poll::Ready(None);
        }
        Poll::Pending
    }
}

// wait_for_segment_or_timeout: used by handshake and FIN-ACK code.
// Delegates to WaitForMailboxOnly which checks segment_mailbox only.
async fn wait_for_segment_or_timeout(
    conn: &SharedTcpConnection,
    seconds: i64,
) -> Option<ReceivedSegment> {
    wait_for_mailbox_or_timeout(conn, seconds).await
}

async fn server_connection_task(
    conn: SharedTcpConnection,
    initial_syn: ReceivedSegment,
    listener: SharedTcpListener,
) {
    let syn_options = build_syn_options(MSS as u16, WINDOW_SCALE_SHIFT);
    let (conn_id, iss, recv_ack) = {
        let mut c = conn.lock();
        c.recv_ack = initial_syn.seq.wrapping_add(1);
        if let Some(shift) = initial_syn.options.window_scale {
            c.send_window_scale = shift.min(14);
            c.recv_window_scale = WINDOW_SCALE_SHIFT;
        }
        let iss = c.send_nxt;
        let recv_ack = c.recv_ack;
        (c.id, iss, recv_ack)
    };
    send_syn_packet(&conn, FLAG_SYN | FLAG_ACK, iss, recv_ack, &syn_options);

    // Wait for ACK to complete handshake
    let mut retransmits = 0;
    loop {
        match wait_for_segment_or_timeout(&conn, 1).await {
            Some(seg) => {
                if seg.flags & FLAG_ACK != 0 && seg.ack == iss.wrapping_add(1) {
                    let mut c = conn.lock();
                    c.send_nxt = iss.wrapping_add(1);
                    c.send_una = iss.wrapping_add(1);
                    c.remote_window = (seg.window_size as u32) << c.send_window_scale;
                    drop(c);
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
                send_syn_packet(&conn, FLAG_SYN | FLAG_ACK, iss, recv_ack, &syn_options);
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
    log_connection_stats(&conn);
    cleanup_connection(conn_id);
}

struct SegmentToSend {
    seq: u32,
    ack: u32,
    data: Vec<u8>,
}

fn flush_send_buffer(conn: &SharedTcpConnection) {
    let (segments, remote_ip, remote_mac, local_port, remote_port, window_adv) = {
        let mut c = conn.lock();
        let mut segments = Vec::new();
        let nxt_offset = c.send_nxt.wrapping_sub(c.send_una) as usize;
        c.send_buffer.make_contiguous();
        let buf_len = c.send_buffer.len();
        let window = c.remote_window as usize;
        let send_una = c.send_una;
        let recv_ack = c.recv_ack;
        let mut cursor = nxt_offset;

        while segments.len() < MAX_SEGMENTS_PER_FLUSH {
            let unsent = buf_len.saturating_sub(cursor);
            if unsent == 0 {
                break;
            }
            if cursor >= window {
                break;
            }
            let allowed = (window - cursor).min(MSS).min(unsent);
            if allowed == 0 {
                break;
            }
            let (front, _) = c.send_buffer.as_slices();
            let data = front[cursor..cursor + allowed].to_vec();
            let seq = send_una.wrapping_add(cursor as u32);
            segments.push(SegmentToSend {
                seq,
                ack: recv_ack,
                data,
            });
            cursor += allowed;
        }

        let new_bytes = cursor - nxt_offset;
        c.send_nxt = c.send_nxt.wrapping_add(new_bytes as u32);

        let window_adv = c.advertised_window();
        (
            segments,
            c.id.remote_ip,
            c.remote_mac,
            c.id.local_port,
            c.id.remote_port,
            window_adv,
        )
    };

    if segments.is_empty() {
        return;
    }

    let segment_count = segments.len() as u64;
    let total_bytes: u64 = segments.iter().map(|s| s.data.len() as u64).sum();

    let packets: Vec<Vec<u8>> = segments
        .into_iter()
        .map(|seg| {
            TcpHeader::create_tcp_packet(
                remote_ip,
                remote_mac,
                local_port,
                remote_port,
                seg.seq,
                seg.ack,
                FLAG_ACK,
                window_adv,
                &seg.data,
                &[],
            )
        })
        .collect();

    super::send_packets(packets);

    let mut c = conn.lock();
    c.stats.flushes += 1;
    c.stats.segments_flushed += segment_count;
    c.stats.packets_sent += segment_count;
    c.stats.bytes_sent += total_bytes;
}

fn send_zero_window_probe(conn: &SharedTcpConnection) {
    let mut c = conn.lock();
    let nxt_offset = c.send_nxt.wrapping_sub(c.send_una) as usize;
    if nxt_offset >= c.send_buffer.len() {
        return; // nothing unsent
    }
    let probe_byte = c.send_buffer[nxt_offset];
    let seq = c.send_nxt;
    let ack = c.recv_ack;
    c.send_nxt = seq.wrapping_add(1);
    let window_adv = c.advertised_window();
    let packet = TcpHeader::create_tcp_packet(
        c.id.remote_ip,
        c.remote_mac,
        c.id.local_port,
        c.id.remote_port,
        seq,
        ack,
        FLAG_ACK,
        window_adv,
        &[probe_byte],
        &[],
    );
    c.stats.packets_sent += 1;
    c.stats.bytes_sent += 1;
    drop(c);
    super::send_packet(packet);
}

fn retransmit(conn: &SharedTcpConnection) {
    let (segments, remote_ip, remote_mac, local_port, remote_port, window_adv) = {
        let mut c = conn.lock();
        let unacked_bytes = c.send_nxt.wrapping_sub(c.send_una) as usize;
        if unacked_bytes == 0 {
            return;
        }
        c.send_buffer.make_contiguous();
        let (front, _) = c.send_buffer.as_slices();
        let send_una = c.send_una;
        let recv_ack = c.recv_ack;
        let mut segments = Vec::new();
        let mut offset = 0;
        while offset < unacked_bytes && segments.len() < MAX_SEGMENTS_PER_FLUSH {
            let len = MSS.min(unacked_bytes - offset);
            let data = front[offset..offset + len].to_vec();
            let seq = send_una.wrapping_add(offset as u32);
            segments.push(SegmentToSend {
                seq,
                ack: recv_ack,
                data,
            });
            offset += len;
        }
        let window_adv = c.advertised_window();
        (
            segments,
            c.id.remote_ip,
            c.remote_mac,
            c.id.local_port,
            c.id.remote_port,
            window_adv,
        )
    };

    if segments.is_empty() {
        return;
    }

    let packets: Vec<Vec<u8>> = segments
        .into_iter()
        .map(|seg| {
            TcpHeader::create_tcp_packet(
                remote_ip,
                remote_mac,
                local_port,
                remote_port,
                seg.seq,
                seg.ack,
                FLAG_ACK,
                window_adv,
                &seg.data,
                &[],
            )
        })
        .collect();

    super::send_packets(packets);
}

async fn drain_and_close(conn: &SharedTcpConnection) {
    loop {
        flush_send_buffer(conn);
        let all_acked = {
            let c = conn.lock();
            c.send_buffer.is_empty() && c.send_nxt == c.send_una
        };
        if all_acked {
            break;
        }

        super::receive_and_process_packets();

        loop {
            let seg = conn.lock().segment_mailbox.pop_front();
            let Some(seg) = seg else { break };
            if seg.flags & FLAG_RST != 0 {
                conn.lock().closed = true;
                return;
            }
            if seg.flags & FLAG_ACK != 0 {
                conn.lock().apply_ack(&seg);
            }
        }

        let (remote_window, has_unacked) = {
            let c = conn.lock();
            (c.remote_window, c.send_nxt != c.send_una)
        };
        if remote_window == 0 {
            send_zero_window_probe(conn);
        }

        match wait_for_mailbox_or_timeout(conn, 1).await {
            Some(seg) => {
                if seg.flags & FLAG_RST != 0 {
                    conn.lock().closed = true;
                    return;
                }
                if seg.flags & FLAG_ACK != 0 {
                    conn.lock().apply_ack(&seg);
                }
            }
            None => {
                if has_unacked {
                    retransmit(conn);
                } else {
                    // No unacked data, but there may be unsent data
                    // waiting for window to open. Only give up if
                    // the buffer is truly empty.
                    let buf_empty = conn.lock().send_buffer.is_empty();
                    if buf_empty {
                        break;
                    }
                    send_zero_window_probe(conn);
                }
            }
        }
    }
    send_fin(conn);
    wait_for_fin_ack(conn).await;
    conn.lock().closed = true;
}

fn send_fin(conn: &SharedTcpConnection) {
    let mut c = conn.lock();
    let seq = c.send_nxt;
    let ack = c.recv_ack;
    c.send_nxt = seq.wrapping_add(1);
    drop(c);
    send_data_packet(conn, FLAG_FIN | FLAG_ACK, seq, ack, &[]);
}

struct SegmentResult {
    need_ack: bool,
    waker: Option<Waker>,
    send_space_waker: Option<Waker>,
    is_fin: bool,
    is_rst: bool,
    is_user_close: bool,
}

fn process_one_segment(conn: &SharedTcpConnection, seg: &ReceivedSegment) -> SegmentResult {
    let mut c = conn.lock();

    if seg.flags & FLAG_RST != 0 {
        c.closed = true;
        return SegmentResult {
            need_ack: false,
            waker: c.recv_waker.take(),
            send_space_waker: None,
            is_fin: false,
            is_rst: true,
            is_user_close: false,
        };
    }

    if seg.flags & FLAG_ACK != 0 {
        c.apply_ack(seg);
    }

    // Wake writer if ACK freed buffer space
    let send_space_waker = if c.send_buffer_space() > 0 {
        c.send_space_waker.take()
    } else {
        None
    };

    let mut need_ack = false;
    let mut waker = None;

    if !seg.data.is_empty() {
        if seg.seq == c.recv_ack {
            c.stats.bytes_received += seg.data.len() as u64;
            c.stats.packets_received += 1;
            c.recv_ack = c.recv_ack.wrapping_add(len_as_seq(seg.data.len()));
            c.recv_buffer.extend(&seg.data);
            waker = c.recv_waker.take();
            need_ack = true;
        } else {
            need_ack = true;
        }
    }

    if seg.flags & FLAG_FIN != 0 {
        c.recv_ack = c.recv_ack.wrapping_add(1);
        c.closed = true;
        waker = waker.or_else(|| c.recv_waker.take());
        return SegmentResult {
            need_ack: true,
            waker,
            send_space_waker,
            is_fin: true,
            is_rst: false,
            is_user_close: false,
        };
    }

    SegmentResult {
        need_ack,
        waker,
        send_space_waker,
        is_fin: false,
        is_rst: false,
        is_user_close: c.user_close_requested,
    }
}

async fn established_loop(conn: &SharedTcpConnection) {
    loop {
        flush_send_buffer(conn);

        match wait_for_event(conn).await {
            TcpEvent::Segment(first_seg) => {
                let mut need_ack = false;
                let mut do_fin = false;
                let mut do_rst = false;
                let mut do_user_close = false;
                let mut wakers: Vec<Waker> = Vec::new();

                let r = process_one_segment(conn, &first_seg);
                need_ack |= r.need_ack;
                do_fin |= r.is_fin;
                do_rst |= r.is_rst;
                do_user_close |= r.is_user_close;
                if let Some(w) = r.waker {
                    wakers.push(w);
                }
                if let Some(w) = r.send_space_waker {
                    wakers.push(w);
                }

                if !do_rst && !do_fin {
                    loop {
                        let next = conn.lock().segment_mailbox.pop_front();
                        let Some(seg) = next else { break };
                        let r = process_one_segment(conn, &seg);
                        need_ack |= r.need_ack;
                        do_fin |= r.is_fin;
                        do_rst |= r.is_rst;
                        do_user_close |= r.is_user_close;
                        if let Some(w) = r.waker {
                            wakers.push(w);
                        }
                        if let Some(w) = r.send_space_waker {
                            wakers.push(w);
                        }
                        if do_rst || do_fin {
                            break;
                        }
                    }
                }

                for w in wakers {
                    w.wake();
                }
                if do_rst {
                    return;
                }
                if need_ack {
                    let (seq, ack) = {
                        let c = conn.lock();
                        (c.send_nxt, c.recv_ack)
                    };
                    send_data_packet(conn, FLAG_ACK, seq, ack, &[]);
                }
                if do_fin {
                    drain_and_close(conn).await;
                    return;
                }
                if do_user_close {
                    drain_and_close(conn).await;
                    return;
                }
            }
            TcpEvent::Timeout => {
                let has_unacked = {
                    let c = conn.lock();
                    c.send_nxt != c.send_una
                };
                if has_unacked {
                    retransmit(conn);
                }
                super::receive_and_process_packets();
            }
            TcpEvent::ReadyToSend => {
                let needs_zwp = {
                    let c = conn.lock();
                    let nxt_offset = c.send_nxt.wrapping_sub(c.send_una) as usize;
                    c.send_buffer.len() > nxt_offset && c.remote_window == 0
                };
                if needs_zwp {
                    send_zero_window_probe(conn);
                }
                // flush happens at top of loop
            }
            TcpEvent::WindowUpdate => {
                let (seq, ack) = {
                    let c = conn.lock();
                    (c.send_nxt, c.recv_ack)
                };
                send_data_packet(conn, FLAG_ACK, seq, ack, &[]);
            }
            TcpEvent::UserClose => {
                drain_and_close(conn).await;
                return;
            }
        }
    }
}

fn send_data_packet(conn: &SharedTcpConnection, flags: u16, seq: u32, ack: u32, data: &[u8]) {
    let c = conn.lock();
    let window = c.advertised_window();
    let packet = TcpHeader::create_tcp_packet(
        c.id.remote_ip,
        c.remote_mac,
        c.id.local_port,
        c.id.remote_port,
        seq,
        ack,
        flags,
        window,
        data,
        &[],
    );
    drop(c);
    super::send_packet(packet);
}

fn send_syn_packet(conn: &SharedTcpConnection, flags: u16, seq: u32, ack: u32, options: &[u8]) {
    let c = conn.lock();
    let window = c.advertised_window();
    let packet = TcpHeader::create_tcp_packet(
        c.id.remote_ip,
        c.remote_mac,
        c.id.local_port,
        c.id.remote_port,
        seq,
        ack,
        flags,
        window,
        &[],
        options,
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
                            (c.send_nxt, c.recv_ack)
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
                    (c.send_nxt.wrapping_sub(1), c.recv_ack)
                };
                send_data_packet(conn, FLAG_FIN | FLAG_ACK, seq, ack, &[]);
            }
        }
    }
}

fn log_connection_stats(conn: &SharedTcpConnection) {
    let c = conn.lock();
    let now = crate::processes::timer::current_time();
    let elapsed_ms = (now.tv_sec - c.stats.start_time.tv_sec) * 1000
        + (now.tv_nsec - c.stats.start_time.tv_nsec) / 1_000_000;
    let avg_seg_per_flush = c
        .stats
        .segments_flushed
        .checked_div(c.stats.flushes)
        .unwrap_or(0);
    let elapsed_ms_u64 = u64::try_from(elapsed_ms).unwrap_or(0);
    let throughput_kbps = (c.stats.bytes_sent * 8)
        .checked_div(elapsed_ms_u64)
        .unwrap_or(0);
    info!(
        "TCP {:?} closed: sent={}B/{}pkts recv={}B/{}pkts flushes={} avg_seg/flush={} {}ms {}kbit/s",
        c.id,
        c.stats.bytes_sent,
        c.stats.packets_sent,
        c.stats.bytes_received,
        c.stats.packets_received,
        c.stats.flushes,
        avg_seg_per_flush,
        elapsed_ms,
        throughput_kbps,
    );
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
    let syn_options = build_syn_options(MSS as u16, WINDOW_SCALE_SHIFT);

    let conn_id = ConnectionId {
        local_port,
        remote_ip: dest_ip,
        remote_port: dest_port,
    };

    let conn = Arc::new(Spinlock::new(TcpConnection::new(conn_id, dest_mac, iss)));
    TCP_CONNECTIONS.lock().insert(conn_id, conn.clone());

    // Send SYN with MSS and window scale options
    send_syn_packet(&conn, FLAG_SYN, iss, 0, &syn_options);

    // Wait for SYN-ACK
    let mut retransmits = 0;
    loop {
        match wait_for_segment_or_timeout(&conn, 1).await {
            Some(seg) => {
                if seg.flags & FLAG_SYN != 0 && seg.flags & FLAG_ACK != 0 {
                    let (seq, ack) = {
                        let mut c = conn.lock();
                        c.recv_ack = seg.seq.wrapping_add(1);
                        c.send_nxt = iss.wrapping_add(1);
                        c.send_una = iss.wrapping_add(1);
                        c.established = true;
                        if let Some(shift) = seg.options.window_scale {
                            c.send_window_scale = shift.min(14);
                            c.recv_window_scale = WINDOW_SCALE_SHIFT;
                        }
                        c.remote_window = (seg.window_size as u32) << c.send_window_scale;
                        (c.send_nxt, c.recv_ack)
                    };
                    send_data_packet(&conn, FLAG_ACK, seq, ack, &[]);
                    info!("TCP connection established (client) {:?}", conn_id);
                    let conn_for_task = conn.clone();
                    kernel_tasks::spawn(async move {
                        established_loop(&conn_for_task).await;
                        log_connection_stats(&conn_for_task);
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
                send_syn_packet(&conn, FLAG_SYN, iss, 0, &syn_options);
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
            let (data, waker) = c.recv_data(count);
            drop(c);
            if let Some(w) = waker {
                w.wake();
            }
            return Poll::Ready(data);
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

pub struct WaitForSendSpace {
    conn: SharedTcpConnection,
}

impl Future for WaitForSendSpace {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut c = self.conn.lock();
        if c.send_buffer_space() > 0 || c.is_closed() {
            return Poll::Ready(());
        }
        c.register_send_space_waker(cx.waker().clone());
        Poll::Pending
    }
}

pub fn wait_for_send_space(conn: &SharedTcpConnection) -> WaitForSendSpace {
    WaitForSendSpace { conn: conn.clone() }
}
