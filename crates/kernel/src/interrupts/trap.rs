use crate::{
    cpu::Cpu,
    info,
    interrupts::{
        plic::{self, PLIC},
        syscall_dispatch,
    },
    memory::VirtAddr,
    processes::timer,
};
use abi::syscalls::trap_frame::TrapFrame;
use core::panic;
use hal::trap_cause::{
    InterruptCause,
    exception::{ENVIRONMENT_CALL_FROM_U_MODE, STORE_AMO_PAGE_FAULT},
    interrupt,
};
use headers::syscall_types::SIGSEGV;

pub extern "C" fn get_process_satp_value() -> usize {
    Cpu::with_current_process(|p| p.get_satp_value())
}

fn assert_sepc_not_in_trap_handler() {
    let sepc = hal::cpu::read_sepc();
    let trap_base = hal::linker_symbols::asm_handle_trap_addr();
    assert!(
        !(trap_base..trap_base + 64).contains(&sepc),
        "BUG: sepc {sepc:#x} points into trap handler {trap_base:#x}"
    );
}

pub extern "C" fn handle_trap() {
    let cause = InterruptCause::from_scause();
    if cause.is_interrupt() {
        match cause.get_exception_code() {
            interrupt::SUPERVISOR_TIMER_INTERRUPT => handle_timer_interrupt(),
            interrupt::SUPERVISOR_SOFTWARE_INTERRUPT => handle_supervisor_software_interrupt(),
            interrupt::SUPERVISOR_EXTERNAL_INTERRUPT => handle_external_interrupt(),
            _ => handle_unimplemented(),
        }
    } else {
        handle_exception();
    }
}

fn handle_timer_interrupt() {
    timer::wakeup_wakers();
    Cpu::with_scheduler(|mut s| s.schedule());
    assert_sepc_not_in_trap_handler();
}

fn handle_external_interrupt() {
    let mut plic_guard = PLIC.lock();
    let irq = match plic_guard.claim() {
        Some(i) => i,
        None => return,
    };
    drop(plic_guard);
    plic::dispatch_interrupt(irq);
    PLIC.lock().complete(irq);
}

fn handle_syscall() {
    let trap_frame: TrapFrame = Cpu::read_trap_frame();
    syscall_dispatch::dispatch(trap_frame);
}

fn handle_unhandled_exception() {
    let cause = InterruptCause::from_scause();
    let stval = hal::cpu::read_stval();
    let sepc = hal::cpu::read_sepc();
    info!(
        "handle_unhandled_exception: cause={} sepc={:#x} stval={:#x}",
        cause.get_reason(),
        sepc,
        stval
    );
    let cpu = Cpu::current();
    let mut scheduler = cpu.scheduler().lock();
    let (message, from_userspace) = scheduler.get_current_process().with_lock(|p| {
        let from_userspace =
            p.get_page_table().is_userspace_address(VirtAddr::new(sepc));
        (format!(
            "Unhandled exception!\nName: {}\nException code: {}\nstval: 0x{:x}\nsepc: 0x{:x}\nFrom Userspace: {}\nProcess name: {}\n{:?}",
            cause.get_reason(),
            cause.get_exception_code(),
            stval,
            sepc,
            from_userspace,
            p.get_name(),
            Cpu::read_trap_frame()
        ), from_userspace)
    });
    if from_userspace {
        info!("{}", message);
        scheduler.kill_current_process(crate::processes::signal::ExitStatus::Signaled(
            u8::try_from(SIGSEGV).expect("signal number fits in u8"),
        ));
        scheduler.schedule();
        return;
    }
    panic!("{}", message);
}

fn handle_store_page_fault() {
    let stval = hal::cpu::read_stval();
    let process = Cpu::with_scheduler(|s| s.get_current_process());
    let resolved = process.with_lock(|mut p| p.resolve_cow_page(VirtAddr::new(stval)));
    if !resolved {
        handle_unhandled_exception();
    }
}

fn handle_exception() {
    let cause = InterruptCause::from_scause();
    let code = cause.get_exception_code();
    match code {
        ENVIRONMENT_CALL_FROM_U_MODE => handle_syscall(),
        STORE_AMO_PAGE_FAULT => handle_store_page_fault(),
        _ => handle_unhandled_exception(),
    }

    assert_sepc_not_in_trap_handler();
}

fn handle_supervisor_software_interrupt() {
    let sleep_requested = crate::processes::kernel_tasks::take_sleep_request();

    Cpu::with_scheduler(|mut s| {
        if sleep_requested {
            let is_worker = s
                .get_current_thread()
                .with_lock(|t| crate::processes::kernel_tasks::is_current_worker_tid(t.get_tid()));
            if is_worker {
                s.get_current_thread().with_lock(|mut t| {
                    t.set_register_state(Cpu::read_trap_frame());
                    t.set_program_counter(VirtAddr::new(hal::cpu::read_sepc()));
                    t.suspend_unless_wakeup_pending();
                });
            }
        }
        s.schedule();
    });

    hal::cpu::clear_supervisor_software_interrupt();
    assert_sepc_not_in_trap_handler();
}

fn handle_unimplemented() {
    let sepc = hal::cpu::read_sepc();
    let cause = InterruptCause::from_scause();
    panic!(
        "Unimplemented trap occurred! (sepc: {:x?}) (cause: {:?})",
        sepc,
        cause.get_reason(),
    );
}
