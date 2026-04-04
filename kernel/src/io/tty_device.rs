use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use common::pid::Tid;
use core::{cmp::min, task::Waker};
#[cfg(target_arch = "riscv64")]
use core::{
    pin::Pin,
    task::{Context, Poll},
};
#[cfg(target_arch = "riscv64")]
use headers::errno::Errno;
#[cfg(target_arch = "riscv64")]
use headers::syscall_types::SIGTTIN;
use headers::syscall_types::{
    CLOCAL, CREAD, CS8, ECHO, ECHOE, ICANON, ICRNL, ISIG, NCCS, ONLCR, OPOST, VEOF, VERASE, VINTR,
    VKILL, VMIN, VQUIT, VSUSP, VTIME, termios,
};

use crate::klibc::{Spinlock, array_vec::ArrayVec, runtime_initialized::RuntimeInitializedData};
#[cfg(target_arch = "riscv64")]
use crate::processes::{process::ProcessRef, process_table};

pub static CONSOLE_TTY: RuntimeInitializedData<TtyDevice> = RuntimeInitializedData::new();

pub fn console_tty() -> &'static TtyDevice {
    &CONSOLE_TTY
}

pub type TtyDevice = Arc<Spinlock<TtyDeviceInner>>;

pub struct InputResult {
    pub action: InputAction,
    pub echo: ArrayVec<u8, 192>,
}

pub enum InputAction {
    Consumed,
    Signal(u32),
}

pub struct TtyDeviceInner {
    settings: termios,
    line_buf: VecDeque<u8>,
    input_buf: VecDeque<u8>,
    wakeup_queue: Vec<Waker>,
    fg_pgid: Tid,
    eof_pending: bool,
    last_tty_signal: Option<(u32, Tid)>,
}

impl TtyDeviceInner {
    pub fn new() -> Self {
        let mut c_cc = [0u8; NCCS as usize];
        c_cc[VINTR as usize] = 3; // Ctrl-C
        c_cc[VQUIT as usize] = 28; // Ctrl-backslash
        c_cc[VERASE as usize] = 127; // DEL
        c_cc[VKILL as usize] = 21; // Ctrl-U
        c_cc[VEOF as usize] = 4; // Ctrl-D
        c_cc[VSUSP as usize] = 26; // Ctrl-Z
        c_cc[VMIN as usize] = 1;
        c_cc[VTIME as usize] = 0;

        Self {
            settings: termios {
                c_iflag: ICRNL,
                c_oflag: OPOST | ONLCR,
                c_cflag: CS8 | CREAD | CLOCAL,
                c_lflag: ISIG | ICANON | ECHO | ECHOE,
                c_line: 0,
                c_cc,
            },
            line_buf: VecDeque::new(),
            input_buf: VecDeque::new(),
            wakeup_queue: Vec::new(),
            fg_pgid: Tid::new(1), // init process group by default
            eof_pending: false,
            last_tty_signal: None,
        }
    }

    pub fn get_termios(&self) -> termios {
        self.settings
    }

    pub fn set_termios(&mut self, new: termios) {
        self.settings = new;
    }

    pub fn fg_pgid(&self) -> Tid {
        self.fg_pgid
    }

    pub fn set_fg_pgid(&mut self, pgid: Tid) {
        self.fg_pgid = pgid;
    }

    pub fn record_tty_signal(&mut self, sig: u32, target_pgid: Tid) {
        self.last_tty_signal = Some((sig, target_pgid));
    }

    pub fn take_signal_for_new_fg(&mut self, caller_pgid: Tid) -> Option<u32> {
        let (sig, _sent_to) = self.last_tty_signal.take()?;
        // Forward only when the caller (shell) was the old foreground group.
        // This means the shell is switching from itself to a job, and the
        // signal may have been sent to the shell instead of the job.
        if self.fg_pgid == caller_pgid {
            Some(sig)
        } else {
            None
        }
    }

    fn has_lflag(&self, flag: u32) -> bool {
        self.settings.c_lflag & flag != 0
    }

    fn has_iflag(&self, flag: u32) -> bool {
        self.settings.c_iflag & flag != 0
    }

    fn has_oflag(&self, flag: u32) -> bool {
        self.settings.c_oflag & flag != 0
    }

    fn echo_newline(&self, echo: &mut ArrayVec<u8, 192>) {
        if self.has_oflag(OPOST) && self.has_oflag(ONLCR) {
            let _ = echo.push(b'\r');
        }
        let _ = echo.push(b'\n');
    }

    pub fn process_output(&self, data: &[u8]) -> Vec<u8> {
        if !(self.has_oflag(OPOST) && self.has_oflag(ONLCR)) {
            return data.to_vec();
        }
        let mut out = Vec::with_capacity(data.len());
        for &b in data {
            if b == b'\n' {
                out.push(b'\r');
            }
            out.push(b);
        }
        out
    }

    pub fn process_input_byte(&mut self, mut byte: u8) -> InputResult {
        self.last_tty_signal = None;
        let mut echo = ArrayVec::new();

        if self.has_iflag(ICRNL) && byte == b'\r' {
            byte = b'\n';
        }

        if self.has_lflag(ISIG) {
            if byte == self.settings.c_cc[VINTR as usize] {
                if self.has_lflag(ECHO) {
                    let _ = echo.push(b'^');
                    let _ = echo.push(b'C');
                    self.echo_newline(&mut echo);
                }
                self.line_buf.clear();
                self.input_buf.clear();
                self.eof_pending = false;
                return InputResult {
                    action: InputAction::Signal(headers::syscall_types::SIGINT),
                    echo,
                };
            }
            if byte == self.settings.c_cc[VSUSP as usize] {
                if self.has_lflag(ECHO) {
                    let _ = echo.push(b'^');
                    let _ = echo.push(b'Z');
                    self.echo_newline(&mut echo);
                }
                self.line_buf.clear();
                self.input_buf.clear();
                self.eof_pending = false;
                return InputResult {
                    action: InputAction::Signal(headers::syscall_types::SIGTSTP),
                    echo,
                };
            }
            if byte == self.settings.c_cc[VQUIT as usize] {
                if self.has_lflag(ECHO) {
                    let _ = echo.push(b'^');
                    let _ = echo.push(b'\\');
                    self.echo_newline(&mut echo);
                }
                self.line_buf.clear();
                self.input_buf.clear();
                self.eof_pending = false;
                return InputResult {
                    action: InputAction::Signal(headers::syscall_types::SIGQUIT),
                    echo,
                };
            }
        }

        if self.has_lflag(ICANON) {
            if byte == b'\n' {
                self.line_buf.push_back(byte);
                if self.has_lflag(ECHO) {
                    self.echo_newline(&mut echo);
                }
                self.flush_line_buf();
            } else if byte == self.settings.c_cc[VERASE as usize] {
                if self.line_buf.pop_back().is_some() && self.has_lflag(ECHOE) {
                    let _ = echo.push(b'\x08');
                    let _ = echo.push(b' ');
                    let _ = echo.push(b'\x08');
                }
            } else if byte == self.settings.c_cc[VEOF as usize] {
                if self.line_buf.is_empty() {
                    self.eof_pending = true;
                    self.wake_all();
                } else {
                    self.flush_line_buf();
                }
            } else if byte == self.settings.c_cc[VKILL as usize] {
                if self.has_lflag(ECHOE) {
                    for _ in 0..self.line_buf.len() {
                        let _ = echo.push(b'\x08');
                        let _ = echo.push(b' ');
                        let _ = echo.push(b'\x08');
                    }
                }
                self.line_buf.clear();
            } else {
                self.line_buf.push_back(byte);
                if self.has_lflag(ECHO) {
                    let _ = echo.push(byte);
                }
            }
        } else {
            self.push_input(byte);
            if self.has_lflag(ECHO) {
                let _ = echo.push(byte);
            }
        }

        InputResult {
            action: InputAction::Consumed,
            echo,
        }
    }

    fn flush_line_buf(&mut self) {
        self.input_buf.extend(self.line_buf.drain(..));
        self.eof_pending = false;
        self.wake_all();
    }

    fn push_input(&mut self, byte: u8) {
        self.input_buf.push_back(byte);
        self.wake_all();
    }

    fn wake_all(&mut self) {
        while let Some(waker) = self.wakeup_queue.pop() {
            waker.wake();
        }
    }

    pub fn get_input(&mut self, count: usize) -> Vec<u8> {
        let actual_count = min(self.input_buf.len(), count);
        self.input_buf.drain(..actual_count).collect()
    }

    pub fn is_input_empty(&self) -> bool {
        self.input_buf.is_empty()
    }

    fn register_wakeup(&mut self, waker: Waker) {
        self.wakeup_queue.push(waker);
    }

    fn vmin(&self) -> usize {
        self.settings.c_cc[VMIN as usize] as usize
    }
}

#[cfg(target_arch = "riscv64")]
pub struct ReadTty {
    device: TtyDevice,
    max_count: usize,
    process: ProcessRef,
    tid: Tid,
}

#[cfg(target_arch = "riscv64")]
impl ReadTty {
    pub fn new(device: TtyDevice, max_count: usize, process: ProcessRef, tid: Tid) -> Self {
        Self {
            device,
            max_count,
            process,
            tid,
        }
    }
}

#[cfg(target_arch = "riscv64")]
impl Future for ReadTty {
    type Output = Result<Vec<u8>, Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // SIGTTIN: background processes must not read from the controlling TTY
        let fg_pgid = this.device.lock().fg_pgid();
        let pgid = this.process.lock().pgid();

        if pgid != fg_pgid {
            let should_eio = process_table::THE.with_lock(|pt| {
                let Some(thread) = pt.get_thread(this.tid) else {
                    return true;
                };
                thread.with_lock(|t| {
                    let is_blocked = t.get_sigmask() & (1u64 << SIGTTIN) != 0;
                    let sa = t.get_sigaction_raw(SIGTTIN);
                    let is_ignored = matches!(sa.sa_handler, Some(f) if f as usize == 1);
                    is_blocked || is_ignored
                })
            });

            if should_eio {
                return Poll::Ready(Err(Errno::EIO));
            }

            process_table::THE.with_lock(|mut pt| {
                pt.send_signal_to_pgid(pgid, SIGTTIN);
            });

            return Poll::Pending;
        }

        let mut dev = this.device.lock();
        let is_canonical = dev.has_lflag(ICANON);
        let vmin = dev.vmin();

        if !dev.is_input_empty() {
            let min_needed = if is_canonical { 1 } else { vmin.max(1) };
            if dev.input_buf.len() >= min_needed || dev.input_buf.len() >= this.max_count {
                return Poll::Ready(Ok(dev.get_input(this.max_count)));
            }
        }

        if is_canonical && dev.eof_pending {
            dev.eof_pending = false;
            return Poll::Ready(Ok(Vec::new()));
        }

        if !is_canonical && vmin == 0 {
            return Poll::Ready(Ok(dev.get_input(this.max_count)));
        }

        dev.register_wakeup(cx.waker().clone());
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn echo_regular_chars() {
        let mut dev = TtyDeviceInner::new();
        let r = dev.process_input_byte(b'a');
        assert!(matches!(r.action, InputAction::Consumed));
        assert_eq!(&*r.echo, b"a");
        assert!(dev.is_input_empty());
    }

    #[test_case]
    fn newline_flushes_line() {
        let mut dev = TtyDeviceInner::new();
        dev.process_input_byte(b'h');
        dev.process_input_byte(b'i');
        let r = dev.process_input_byte(b'\n');
        assert_eq!(&*r.echo, b"\r\n");
        assert_eq!(dev.get_input(1024), b"hi\n");
    }

    #[test_case]
    fn cr_mapped_to_nl() {
        let mut dev = TtyDeviceInner::new();
        dev.process_input_byte(b'x');
        let r = dev.process_input_byte(b'\r');
        assert_eq!(&*r.echo, b"\r\n");
        assert_eq!(dev.get_input(1024), b"x\n");
    }

    #[test_case]
    fn backspace_erases() {
        let mut dev = TtyDeviceInner::new();
        dev.process_input_byte(b'a');
        dev.process_input_byte(b'b');
        let r = dev.process_input_byte(127);
        assert_eq!(&*r.echo, b"\x08 \x08");
        dev.process_input_byte(b'\n');
        assert_eq!(dev.get_input(1024), b"a\n");
    }

    #[test_case]
    fn backspace_on_empty_does_nothing() {
        let mut dev = TtyDeviceInner::new();
        let r = dev.process_input_byte(127);
        assert!(r.echo.is_empty());
    }

    #[test_case]
    fn ctrl_c_generates_sigint() {
        let mut dev = TtyDeviceInner::new();
        dev.process_input_byte(b'x');
        let r = dev.process_input_byte(3);
        assert!(
            matches!(r.action, InputAction::Signal(sig) if sig == headers::syscall_types::SIGINT)
        );
        dev.process_input_byte(b'\n');
        assert_eq!(dev.get_input(1024), b"\n");
    }

    #[test_case]
    fn ctrl_z_generates_sigtstp() {
        let mut dev = TtyDeviceInner::new();
        let r = dev.process_input_byte(26);
        assert!(
            matches!(r.action, InputAction::Signal(sig) if sig == headers::syscall_types::SIGTSTP)
        );
        assert_eq!(&*r.echo, b"^Z\r\n");
    }

    #[test_case]
    fn ctrl_backslash_generates_sigquit() {
        let mut dev = TtyDeviceInner::new();
        let r = dev.process_input_byte(28);
        assert!(
            matches!(r.action, InputAction::Signal(sig) if sig == headers::syscall_types::SIGQUIT)
        );
        assert_eq!(&*r.echo, b"^\\\r\n");
    }

    #[test_case]
    fn ctrl_d_on_empty_line_sets_eof() {
        let mut dev = TtyDeviceInner::new();
        let r = dev.process_input_byte(4); // Ctrl-D
        assert!(matches!(r.action, InputAction::Consumed));
        assert!(dev.eof_pending);
        assert!(dev.is_input_empty());
    }

    #[test_case]
    fn ctrl_d_with_data_flushes_without_eof() {
        let mut dev = TtyDeviceInner::new();
        dev.process_input_byte(b'a');
        dev.process_input_byte(b'b');
        let r = dev.process_input_byte(4); // Ctrl-D
        assert!(matches!(r.action, InputAction::Consumed));
        assert!(!dev.eof_pending);
        assert_eq!(dev.get_input(1024), b"ab");
    }

    #[test_case]
    fn new_line_clears_stale_eof_pending() {
        let mut dev = TtyDeviceInner::new();
        dev.process_input_byte(4); // Ctrl-D on empty line
        assert!(dev.eof_pending);
        dev.process_input_byte(b'x');
        dev.process_input_byte(b'\n');
        assert!(!dev.eof_pending);
        assert_eq!(dev.get_input(1024), b"x\n");
    }

    #[test_case]
    fn onlcr_output_processing() {
        let dev = TtyDeviceInner::new();
        assert_eq!(dev.process_output(b"hello\nworld\n"), b"hello\r\nworld\r\n");
    }

    #[test_case]
    fn ctrl_c_flushes_input_buf() {
        let mut dev = TtyDeviceInner::new();
        dev.process_input_byte(b'f');
        dev.process_input_byte(b'g');
        dev.process_input_byte(b'\n');
        assert!(!dev.is_input_empty());
        dev.process_input_byte(3); // Ctrl-C
        assert!(dev.is_input_empty());
    }

    #[test_case]
    fn signal_clears_eof_pending() {
        let mut dev = TtyDeviceInner::new();
        dev.process_input_byte(4); // Ctrl-D on empty line
        assert!(dev.eof_pending);
        dev.process_input_byte(3); // Ctrl-C
        assert!(!dev.eof_pending);
    }

    #[test_case]
    fn no_onlcr_when_disabled() {
        let mut dev = TtyDeviceInner::new();
        dev.settings.c_oflag = 0;
        assert_eq!(dev.process_output(b"hello\nworld\n"), b"hello\nworld\n");
    }

    #[test_case]
    fn take_signal_forwards_when_caller_is_fg() {
        let mut dev = TtyDeviceInner::new();
        let shell = Tid::new(10);
        dev.set_fg_pgid(shell);
        dev.record_tty_signal(headers::syscall_types::SIGINT, shell);
        assert_eq!(
            dev.take_signal_for_new_fg(shell),
            Some(headers::syscall_types::SIGINT)
        );
    }

    #[test_case]
    fn take_signal_does_not_forward_when_caller_is_not_fg() {
        let mut dev = TtyDeviceInner::new();
        let job = Tid::new(20);
        let shell = Tid::new(10);
        dev.set_fg_pgid(job);
        dev.record_tty_signal(headers::syscall_types::SIGINT, job);
        assert_eq!(dev.take_signal_for_new_fg(shell), None);
    }

    #[test_case]
    fn take_signal_returns_none_when_no_signal() {
        let mut dev = TtyDeviceInner::new();
        let shell = Tid::new(10);
        dev.set_fg_pgid(shell);
        assert_eq!(dev.take_signal_for_new_fg(shell), None);
    }

    #[test_case]
    fn input_clears_stale_tty_signal() {
        let mut dev = TtyDeviceInner::new();
        let shell = Tid::new(10);
        dev.set_fg_pgid(shell);
        dev.record_tty_signal(headers::syscall_types::SIGINT, shell);
        dev.process_input_byte(b'f');
        assert_eq!(dev.take_signal_for_new_fg(shell), None);
    }
}
