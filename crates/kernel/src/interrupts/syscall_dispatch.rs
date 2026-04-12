//! Trap-time syscall dispatch indirection.
//!
//! `trap.rs` decodes the cause and, on `ENVIRONMENT_CALL_FROM_U_MODE`,
//! forwards the trap frame to a registered `SyscallDispatcher`. The
//! concrete implementation lives in `processes::syscall_runner`, which
//! owns the Task / waker / LinuxSyscallHandler wiring. Trap decoding
//! thus stops importing `processes::task`, `processes::waker`, and
//! `syscalls::linux::LinuxSyscallHandler`.
//!
//! Registration happens once at boot after the scheduler and process
//! table are up. Before registration, `dispatch` panics — which is the
//! right behaviour, since a userspace ecall before scheduler init would
//! already be a bug.

use abi::syscalls::trap_frame::TrapFrame;
use klib::runtime_initialized::RuntimeInitializedData;

pub trait SyscallDispatcher: Sync {
    fn dispatch(&self, trap_frame: TrapFrame);
}

static DISPATCHER: RuntimeInitializedData<&'static dyn SyscallDispatcher> =
    RuntimeInitializedData::new();

pub fn register(dispatcher: &'static dyn SyscallDispatcher) {
    DISPATCHER.initialize(dispatcher);
}

pub fn dispatch(trap_frame: TrapFrame) {
    DISPATCHER.dispatch(trap_frame);
}
