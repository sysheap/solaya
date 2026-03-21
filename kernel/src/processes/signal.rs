#![allow(unsafe_code)]
use core::arch::global_asm;

use crate::{
    debug,
    klibc::util::ByteInterpretable,
    memory::{PAGE_SIZE, PhysAddr, VirtAddr},
    processes::{thread::Thread, userspace_ptr::UserspacePtr},
};
use common::syscalls::trap_frame::Register;
use headers::syscall_types::{
    SA_NODEFER, SA_RESETHAND, SIGABRT, SIGALRM, SIGBUS, SIGCHLD, SIGCONT, SIGFPE, SIGHUP, SIGILL,
    SIGINT, SIGIO, SIGKILL, SIGPIPE, SIGPROF, SIGPWR, SIGQUIT, SIGSEGV, SIGSTKFLT, SIGSTOP, SIGSYS,
    SIGTERM, SIGTRAP, SIGTSTP, SIGTTIN, SIGTTOU, SIGURG, SIGUSR1, SIGUSR2, SIGVTALRM, SIGWINCH,
    SIGXCPU, SIGXFSZ, sigset_t,
};

pub const TRAMPOLINE_VADDR: VirtAddr = VirtAddr::new(0x1000);

#[cfg(not(miri))]
global_asm!(
    ".pushsection .text",
    ".balign {PAGE_SIZE}",
    "__signal_trampoline:",
    "li a7, {NR_RT_SIGRETURN}",
    "ecall",
    ".skip {PAGE_SIZE} - (. - __signal_trampoline)",
    ".popsection",
    PAGE_SIZE = const PAGE_SIZE,
    NR_RT_SIGRETURN = const headers::syscalls::SYSCALL_NR_RT_SIGRETURN,
);

#[cfg(not(miri))]
pub fn trampoline_phys_addr() -> PhysAddr {
    unsafe extern "C" {
        static __signal_trampoline: u8;
    }
    PhysAddr::new(core::ptr::addr_of!(__signal_trampoline) as usize)
}

#[cfg(miri)]
pub fn trampoline_phys_addr() -> PhysAddr {
    PhysAddr::new(0x1000)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Exited(u8),
    Signaled(u8),
}

impl ExitStatus {
    pub fn to_wstatus(self) -> i32 {
        match self {
            ExitStatus::Exited(code) => i32::from(code) << 8,
            ExitStatus::Signaled(sig) => i32::from(sig) & 0x7f,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PendingSignals(u64);

impl PendingSignals {
    pub const fn new() -> Self {
        Self(0)
    }

    pub fn raise(&mut self, sig: u32) {
        assert!((1..=31).contains(&sig));
        self.0 |= 1u64 << sig;
    }

    pub fn clear(&mut self, sig: u32) {
        assert!((1..=31).contains(&sig));
        self.0 &= !(1u64 << sig);
    }

    pub fn first_unblocked(&self, mask: u64) -> Option<u32> {
        let deliverable = self.0 & !mask;
        if deliverable == 0 {
            return None;
        }
        Some(deliverable.trailing_zeros())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAction {
    Terminate,
    Ignore,
    Stop,
    Continue,
}

pub fn validate_signal(sig: i32) -> Result<Option<u32>, headers::errno::Errno> {
    let sig_u32 = u32::try_from(sig).map_err(|_| headers::errno::Errno::EINVAL)?;
    if sig_u32 >= headers::syscall_types::_NSIG {
        return Err(headers::errno::Errno::EINVAL);
    }
    if sig_u32 == 0 {
        Ok(None)
    } else {
        Ok(Some(sig_u32))
    }
}

pub fn default_action(sig: u32) -> DefaultAction {
    match sig {
        SIGHUP | SIGINT | SIGQUIT | SIGILL | SIGTRAP | SIGABRT | SIGBUS | SIGFPE | SIGKILL
        | SIGUSR1 | SIGSEGV | SIGUSR2 | SIGPIPE | SIGALRM | SIGTERM | SIGSTKFLT | SIGXCPU
        | SIGXFSZ | SIGVTALRM | SIGPROF | SIGIO | SIGPWR | SIGSYS => DefaultAction::Terminate,
        SIGCHLD | SIGURG | SIGWINCH => DefaultAction::Ignore,
        SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => DefaultAction::Stop,
        SIGCONT => DefaultAction::Continue,
        _ => DefaultAction::Terminate,
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct SignalFrame {
    saved_regs: [usize; 32],
    saved_fregs: [usize; 32],
    saved_pc: usize,
    saved_sigmask: u64,
}

const SIGNAL_FRAME_SIZE: usize = core::mem::size_of::<SignalFrame>();

impl ByteInterpretable for SignalFrame {}

pub enum SignalDeliveryResult {
    Continue,
    Terminate(ExitStatus),
    Stop(u32),
}

/// Check for pending signals and either set up a signal handler frame, return
/// an ExitStatus if the default action is to terminate, or return Stop if the
/// process should be stopped. Called before returning to userspace.
pub fn deliver_signal(thread: &mut Thread) -> SignalDeliveryResult {
    loop {
        let Some(sig) = thread.take_next_pending_signal() else {
            return SignalDeliveryResult::Continue;
        };
        let action = *thread.get_sigaction_raw(sig);
        let handler = action.sa_handler;

        match handler {
            None => {
                // SIG_DFL
                match default_action(sig) {
                    DefaultAction::Terminate => {
                        return SignalDeliveryResult::Terminate(ExitStatus::Signaled(
                            u8::try_from(sig).expect("signal number fits in u8"),
                        ));
                    }
                    DefaultAction::Stop => {
                        return SignalDeliveryResult::Stop(sig);
                    }
                    DefaultAction::Ignore | DefaultAction::Continue => {
                        continue;
                    }
                }
            }
            Some(f) if f as usize == 1 => {
                // SIG_IGN
                continue;
            }
            Some(handler_fn) => {
                if setup_signal_frame(thread, sig, handler_fn, &action) {
                    return SignalDeliveryResult::Continue;
                }
                // Frame write failed — force-kill if this was already SIGSEGV
                // to avoid infinite loop, otherwise raise SIGSEGV and retry.
                if sig == SIGSEGV {
                    return SignalDeliveryResult::Terminate(ExitStatus::Signaled(
                        u8::try_from(SIGSEGV).expect("signal number fits in u8"),
                    ));
                }
                thread.raise_signal(SIGSEGV);
                continue;
            }
        }
    }
}

fn setup_signal_frame(
    thread: &mut Thread,
    sig: u32,
    handler: unsafe extern "C" fn(core::ffi::c_int),
    action: &headers::syscall_types::sigaction,
) -> bool {
    let regs = thread.get_register_state();
    let pc = thread.get_program_counter();
    let sigmask = thread.get_sigmask();

    let frame = SignalFrame {
        saved_regs: *regs.gp_registers(),
        saved_fregs: *regs.fp_registers(),
        saved_pc: pc.as_usize(),
        saved_sigmask: sigmask,
    };

    let user_sp = regs[Register::sp];
    let frame_sp = (user_sp - SIGNAL_FRAME_SIZE) & !0xF;

    // Write the signal frame to the user stack through page tables
    let process = thread.process();
    let write_ptr: UserspacePtr<*mut u8> =
        UserspacePtr::new(core::ptr::without_provenance_mut(frame_sp));
    if process
        .lock()
        .write_userspace_slice(&write_ptr, frame.as_slice())
        .is_err()
    {
        debug!("Failed to write signal frame for sig={sig}");
        return false;
    }

    // Set up registers for the signal handler
    let trap_frame = thread.get_register_state_mut();
    trap_frame[Register::sp] = frame_sp;
    trap_frame[Register::a0] = sig as usize;
    trap_frame[Register::ra] = TRAMPOLINE_VADDR.as_usize();
    thread.set_program_counter(VirtAddr::new(handler as usize));

    // Update signal mask: block sa_mask and the signal itself (unless SA_NODEFER)
    let mut new_mask = sigmask | action.sa_mask.sig[0];
    if action.sa_flags & u64::from(SA_NODEFER) == 0 {
        new_mask |= 1u64 << sig;
    }
    thread.set_sigmask_raw(new_mask);

    // SA_RESETHAND: reset handler to SIG_DFL after first delivery
    if action.sa_flags & u64::from(SA_RESETHAND) != 0 {
        let _ = thread.set_sigaction(
            sig,
            headers::syscall_types::sigaction {
                sa_handler: None,
                sa_flags: 0,
                sa_mask: sigset_t { sig: [0] },
            },
        );
    }

    true
}

pub fn restore_signal_frame(thread: &mut Thread) -> Result<(), headers::errno::Errno> {
    let sp = thread.get_register_state()[Register::sp];
    let process = thread.process();
    let read_ptr: UserspacePtr<*const u8> = UserspacePtr::new(core::ptr::without_provenance(sp));
    let bytes = process
        .lock()
        .read_userspace_slice(&read_ptr, SIGNAL_FRAME_SIZE)?;
    assert!(bytes.len() == SIGNAL_FRAME_SIZE);
    let frame: SignalFrame = crate::klibc::util::read_from_bytes(&bytes);

    *thread.get_register_state_mut().gp_registers_mut() = frame.saved_regs;
    *thread.get_register_state_mut().fp_registers_mut() = frame.saved_fregs;
    thread.set_program_counter(VirtAddr::new(frame.saved_pc));
    thread.set_sigmask_raw(frame.saved_sigmask);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn validate_signal_returns_none_for_zero() {
        assert_eq!(validate_signal(0), Ok(None));
    }

    #[test_case]
    fn validate_signal_returns_some_for_valid() {
        assert_eq!(validate_signal(9), Ok(Some(9)));
        assert_eq!(validate_signal(1), Ok(Some(1)));
    }

    #[test_case]
    fn validate_signal_rejects_negative() {
        assert_eq!(validate_signal(-1), Err(headers::errno::Errno::EINVAL));
    }

    #[test_case]
    fn validate_signal_rejects_too_large() {
        assert_eq!(
            validate_signal(headers::syscall_types::_NSIG as i32),
            Err(headers::errno::Errno::EINVAL)
        );
    }
}
