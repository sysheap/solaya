use core::{
    net::Ipv4Addr,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::{
    collections::{BTreeMap, VecDeque, btree_map::Entry},
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::{debug, klibc::Spinlock};

static SOCKET_DATA_COUNTER: AtomicU64 = AtomicU64::new(0);
static SOCKET_WAITERS: Spinlock<Vec<Waker>> = Spinlock::new(Vec::new());

pub fn wake_socket_waiters() {
    SOCKET_DATA_COUNTER.fetch_add(1, Ordering::SeqCst);
    let wakers: Vec<Waker> = SOCKET_WAITERS.lock().drain(..).collect();
    for waker in wakers {
        waker.wake();
    }
}

pub fn socket_data_counter() -> u64 {
    SOCKET_DATA_COUNTER.load(Ordering::SeqCst)
}

pub struct SocketDataWait {
    seen_counter: u64,
    registered: bool,
}

impl SocketDataWait {
    pub fn new(seen_counter: u64) -> Self {
        Self {
            seen_counter,
            registered: false,
        }
    }
}

impl Future for SocketDataWait {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current = SOCKET_DATA_COUNTER.load(Ordering::SeqCst);
        if current != self.seen_counter {
            return Poll::Ready(());
        }
        if !self.registered {
            SOCKET_WAITERS.lock().push(cx.waker().clone());
            self.registered = true;
            // Double-check after registering to prevent lost wakeups
            let current = SOCKET_DATA_COUNTER.load(Ordering::SeqCst);
            if current != self.seen_counter {
                return Poll::Ready(());
            }
        }
        Poll::Pending
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Port(u16);

impl Port {
    pub const fn new(port: u16) -> Self {
        Self(port)
    }

    pub fn as_u16(self) -> u16 {
        self.0
    }
}

impl core::fmt::Display for Port {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub type SharedAssignedSocket = Arc<Spinlock<AssignedSocket>>;
type WeakSharedAssignedSocket = Weak<Spinlock<AssignedSocket>>;

type SpinlockSocketMap = Spinlock<BTreeMap<Port, WeakSharedAssignedSocket>>;
type SharedSocketMap = Arc<SpinlockSocketMap>;
type WeakSharedSocketMap = Weak<SpinlockSocketMap>;

pub struct OpenSockets {
    sockets: SharedSocketMap,
}

impl OpenSockets {
    pub fn new() -> Self {
        Self {
            sockets: Arc::new(Spinlock::new(BTreeMap::new())),
        }
    }

    pub fn try_get_socket(&self, port: Port) -> Option<SharedAssignedSocket> {
        let mut sockets = self.sockets.lock();
        if sockets.contains_key(&port) {
            return None;
        }

        let weak_socket_map = Arc::downgrade(&self.sockets);
        let assigned_socket = AssignedSocket::new(port, weak_socket_map);

        let arc_socket = Arc::new(Spinlock::new(assigned_socket));

        assert!(
            sockets.insert(port, Arc::downgrade(&arc_socket)).is_none(),
            "There must be no value before in the socket map."
        );

        Some(arc_socket)
    }

    pub fn put_data(&self, from: Ipv4Addr, from_port: Port, to_port: Port, data: &[u8]) {
        let mut sockets = self.sockets.lock();
        match sockets.entry(to_port) {
            Entry::Vacant(_) => {
                debug!("Recived packet on {} but there is no listener.", to_port)
            }
            Entry::Occupied(mut entry) => entry
                .get_mut()
                .upgrade()
                .expect("There must an assigned socket.")
                .lock()
                .put_data(from, from_port, data),
        }
    }
}

struct Datagram {
    from: Ipv4Addr,
    from_port: Port,
    data: Vec<u8>,
}

pub struct AssignedSocket {
    datagrams: VecDeque<Datagram>,
    port: Port,
    open_sockets: WeakSharedSocketMap,
}

impl AssignedSocket {
    fn new(port: Port, open_sockets: WeakSharedSocketMap) -> Self {
        Self {
            datagrams: VecDeque::new(),
            port,
            open_sockets,
        }
    }

    pub fn get_port(&self) -> Port {
        self.port
    }

    fn put_data(&mut self, from: Ipv4Addr, from_port: Port, data: &[u8]) {
        self.datagrams.push_back(Datagram {
            from,
            from_port,
            data: data.to_vec(),
        });
    }

    pub fn get_datagram(&mut self, out_buffer: &mut [u8]) -> Option<(usize, Ipv4Addr, Port)> {
        let datagram = self.datagrams.pop_front()?;
        let len = usize::min(datagram.data.len(), out_buffer.len());
        out_buffer[..len].copy_from_slice(&datagram.data[..len]);
        Some((len, datagram.from, datagram.from_port))
    }
}

impl Drop for AssignedSocket {
    fn drop(&mut self) {
        let sockets = self
            .open_sockets
            .upgrade()
            .expect("The original map must exist.");
        let mut sockets = sockets.lock();
        assert!(
            sockets.remove(&self.port).is_some(),
            "There must be a value to remove in the map."
        );
    }
}

#[cfg(test)]
mod tests {
    use core::net::Ipv4Addr;

    use super::{OpenSockets, Port};

    impl super::AssignedSocket {
        pub fn has_data(&self) -> bool {
            !self.datagrams.is_empty()
        }
    }

    const PORT1: Port = Port::new(1234);
    const FROM1: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);

    const PORT2: Port = Port::new(4444);
    const FROM2: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 2);

    #[test_case]
    fn duplicate_ports() {
        let open_sockets = OpenSockets::new();

        let _assigned_socket = open_sockets
            .try_get_socket(PORT1)
            .expect("There must be a free port.");

        assert!(
            open_sockets.try_get_socket(PORT1).is_none(),
            "Ports must not handed out twice."
        );
    }

    #[test_case]
    fn data_delivery() {
        let open_sockets = OpenSockets::new();

        let assigned_port1 = open_sockets
            .try_get_socket(PORT1)
            .expect("Port must be free");

        let assigned_port2 = open_sockets
            .try_get_socket(PORT2)
            .expect("Port must be free");

        assert!(
            !assigned_port1.lock().has_data(),
            "Buffer must be empty intially"
        );
        assert!(
            !assigned_port2.lock().has_data(),
            "Buffer must be empty intially"
        );

        let port1_data = [1, 2, 3];
        let port2_data = [3, 2, 1];

        open_sockets.put_data(FROM1, PORT1, PORT1, &port1_data);

        assert!(assigned_port1.lock().has_data(), "Data must be delivered.");
        assert!(
            !assigned_port2.lock().has_data(),
            "Buffer must be still empty."
        );

        open_sockets.put_data(FROM2, PORT2, PORT2, &port2_data);

        let mut buf1 = [0; 10];
        let mut buf2 = [0; 10];

        let (len1, from1, from_port1) = assigned_port1
            .lock()
            .get_datagram(&mut buf1)
            .expect("Must have datagram");
        let (len2, from2, from_port2) = assigned_port2
            .lock()
            .get_datagram(&mut buf2)
            .expect("Must have datagram");

        assert_eq!(len1, 3, "Data must be copied completely.");
        assert_eq!(len2, 3, "Data must be copied completely.");
        assert_eq!(from1, FROM1);
        assert_eq!(from2, FROM2);
        assert_eq!(from_port1, PORT1);
        assert_eq!(from_port2, PORT2);

        assert_eq!(buf1[0..3], port1_data, "Data must be the same.");
        assert_eq!(buf2[0..3], port2_data, "Data must be the same.");

        assert!(
            !assigned_port1.lock().has_data(),
            "Buffer must be empty again"
        );
        assert!(
            !assigned_port2.lock().has_data(),
            "Buffer must be empty again"
        );
    }

    #[test_case]
    fn datagram_truncation() {
        let open_sockets = OpenSockets::new();

        let socket = open_sockets
            .try_get_socket(PORT1)
            .expect("Socket must be free");

        socket
            .lock()
            .put_data(Ipv4Addr::UNSPECIFIED, PORT1, &[1, 2, 3, 4, 5]);

        let mut small_buffer = [0; 2];
        let (len, _, _) = socket
            .lock()
            .get_datagram(&mut small_buffer)
            .expect("Must have datagram");
        assert_eq!(len, 2, "Only 2 bytes must be copied");
        assert_eq!(small_buffer, [1, 2]);

        assert!(
            !socket.lock().has_data(),
            "Remainder of datagram must be discarded (UDP semantics)"
        );
    }

    #[test_case]
    fn multiple_datagrams_preserve_sender() {
        let open_sockets = OpenSockets::new();

        let socket = open_sockets
            .try_get_socket(PORT1)
            .expect("There must be a free socket.");

        assert!(!socket.lock().has_data(), "Must be initially empty.");

        open_sockets.put_data(FROM1, PORT1, PORT1, &[1, 2, 3]);
        open_sockets.put_data(FROM2, PORT2, PORT1, &[4, 5, 6]);

        let mut buf = [0; 10];

        let (len, from, from_port) = socket
            .lock()
            .get_datagram(&mut buf)
            .expect("Must have first datagram");
        assert_eq!(len, 3);
        assert_eq!(from, FROM1);
        assert_eq!(from_port, PORT1);
        assert_eq!(buf[..3], [1, 2, 3]);

        let (len, from, from_port) = socket
            .lock()
            .get_datagram(&mut buf)
            .expect("Must have second datagram");
        assert_eq!(len, 3);
        assert_eq!(from, FROM2);
        assert_eq!(from_port, PORT2);
        assert_eq!(buf[..3], [4, 5, 6]);
    }

    #[test_case]
    fn drop_must_work_correctly() {
        let open_sockets = OpenSockets::new();

        let assigned_socket = open_sockets
            .try_get_socket(PORT1)
            .expect("There must be a free port.");

        assert!(
            open_sockets.sockets.lock().contains_key(&PORT1),
            "Open sockets must contain the port."
        );

        drop(assigned_socket);

        assert!(
            !open_sockets.sockets.lock().contains_key(&PORT1),
            "Open sockets must not contain port."
        );

        assert!(
            open_sockets.try_get_socket(PORT1).is_some(),
            "Port must be reusable after drop."
        );
    }
}
