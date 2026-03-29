use crate::{
    cpu::Cpu,
    debugging::{
        eh_frame_parser::EhFrameParser,
        unwinder::{RegisterRule, Unwinder},
    },
    fs::vfs,
    klibc::{elf::ElfFile, util::UsizeExt},
    println,
    processes::userspace_ptr::UserspacePtr,
    syscalls::{
        linux::{LinuxSyscallHandler, LinuxSyscalls, SYSCALL_METADATA},
        trace_config::TRACED_PROCESSES,
    },
};
use alloc::vec::Vec;
use common::syscalls::trap_frame::{Register, TrapFrame};
use core::ffi::{c_int, c_uint, c_ulong};
use headers::errno::Errno;

#[derive(Clone, Copy)]
pub enum ArgFormat {
    SignedDec,
    Hex,
    Pointer,
}

pub struct SyscallMetadata {
    pub name: &'static str,
    pub args: &'static [(&'static str, ArgFormat)],
}

pub trait SyscallArgFormat {
    const FORMAT: ArgFormat;
}

impl SyscallArgFormat for c_int {
    const FORMAT: ArgFormat = ArgFormat::SignedDec;
}
impl SyscallArgFormat for c_uint {
    const FORMAT: ArgFormat = ArgFormat::Hex;
}
impl SyscallArgFormat for c_ulong {
    const FORMAT: ArgFormat = ArgFormat::Hex;
}
impl SyscallArgFormat for usize {
    const FORMAT: ArgFormat = ArgFormat::Hex;
}
impl SyscallArgFormat for isize {
    const FORMAT: ArgFormat = ArgFormat::SignedDec;
}
impl<T> SyscallArgFormat for *const T {
    const FORMAT: ArgFormat = ArgFormat::Pointer;
}
impl<T> SyscallArgFormat for *mut T {
    const FORMAT: ArgFormat = ArgFormat::Pointer;
}
impl<T> SyscallArgFormat for Option<*const T> {
    const FORMAT: ArgFormat = ArgFormat::Pointer;
}
impl<T> SyscallArgFormat for Option<*mut T> {
    const FORMAT: ArgFormat = ArgFormat::Pointer;
}

fn should_trace() -> bool {
    if TRACED_PROCESSES.is_empty() {
        return false;
    }
    Cpu::with_current_process(|p| TRACED_PROCESSES.contains(&p.get_name()))
}

fn find_metadata(nr: usize) -> Option<&'static SyscallMetadata> {
    SYSCALL_METADATA
        .iter()
        .find(|(n, _)| *n == nr)
        .map(|(_, m)| m)
}

fn format_arg(raw: usize, fmt: ArgFormat) -> alloc::string::String {
    match fmt {
        ArgFormat::SignedDec => alloc::format!("{}", raw as isize),
        ArgFormat::Hex => alloc::format!("{:#x}", raw),
        ArgFormat::Pointer if raw == 0 => alloc::string::String::from("NULL"),
        ArgFormat::Pointer => alloc::format!("{:#x}", raw),
    }
}

fn log_enter(trap_frame: &TrapFrame, tid: common::pid::Tid) {
    let nr = trap_frame[Register::a7];
    let args = [
        trap_frame[Register::a0],
        trap_frame[Register::a1],
        trap_frame[Register::a2],
        trap_frame[Register::a3],
        trap_frame[Register::a4],
        trap_frame[Register::a5],
    ];

    let Some(meta) = find_metadata(nr) else {
        println!("[SYSCALL ENTER] tid={tid} syscall_{nr}(...)");
        return;
    };

    let mut arg_strs = alloc::string::String::new();
    for (i, (name, fmt)) in meta.args.iter().enumerate() {
        if i > 0 {
            arg_strs.push_str(", ");
        }
        arg_strs.push_str(name);
        arg_strs.push_str(": ");
        arg_strs.push_str(&format_arg(args[i], *fmt));
    }

    println!("[SYSCALL ENTER] tid={tid} {}({arg_strs})", meta.name);
}

fn log_exit(trap_frame: &TrapFrame, tid: common::pid::Tid, result: &Result<isize, Errno>) {
    let nr = trap_frame[Register::a7];
    let name = find_metadata(nr).map(|m| m.name).unwrap_or("unknown");

    match result {
        Ok(val) => println!("[SYSCALL EXIT]  tid={tid} {name} = {val}"),
        Err(e) => println!(
            "[SYSCALL EXIT]  tid={tid} {name} = -{} ({e:?})",
            *e as isize
        ),
    }
}

pub async fn trace_syscall(
    trap_frame: &TrapFrame,
    handler: &mut LinuxSyscallHandler,
) -> Result<isize, Errno> {
    let tracing = should_trace();
    let tid = if tracing {
        let tid = Cpu::with_scheduler(|s| s.get_current_thread().lock().get_tid());
        log_enter(trap_frame, tid);
        Some(tid)
    } else {
        None
    };
    let result = handler.handle(trap_frame).await;
    if let Some(tid) = tid {
        log_exit(trap_frame, tid, &result);
    }
    result
}

pub fn log_unimplemented_and_kill(trap_frame: &TrapFrame) {
    let nr = trap_frame[Register::a7];
    let args = [
        trap_frame[Register::a0],
        trap_frame[Register::a1],
        trap_frame[Register::a2],
        trap_frame[Register::a3],
        trap_frame[Register::a4],
        trap_frame[Register::a5],
    ];
    let pc = arch::cpu::read_sepc();

    let name = headers::syscalls::SYSCALL_NAMES
        .iter()
        .find_map(|(n, name)| if *n == nr { Some(*name) } else { None })
        .unwrap_or("unknown");

    let (process_name, tid) = Cpu::with_scheduler(|s| {
        let name = alloc::string::String::from(s.get_current_process().lock().get_name());
        let tid = s.get_current_thread().lock().get_tid();
        (name, tid)
    });

    println!(
        "[UNIMPLEMENTED SYSCALL] process={} tid={} {}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) nr={} pc={:#x}",
        process_name, tid, name, args[0], args[1], args[2], args[3], args[4], args[5], nr, pc
    );

    print_userspace_backtrace(trap_frame, &process_name);

    Cpu::with_scheduler(|s| {
        s.get_current_thread()
            .lock()
            .raise_signal(headers::syscall_types::SIGKILL);
    });
}

fn read_userspace_usize(addr: usize) -> Option<usize> {
    Cpu::with_current_process(|p| {
        let ptr = UserspacePtr::new(core::ptr::without_provenance::<usize>(addr));
        p.read_userspace_ptr(&ptr).ok()
    })
}

fn print_userspace_backtrace(trap_frame: &TrapFrame, process_name: &str) {
    let elf_buf = Cpu::with_current_process(|p| -> Option<sys::klibc::util::AlignedBuffer> {
        let path = p.binary_path()?;
        let node = vfs::resolve_path(path).ok()?;
        let size = node.size();
        let mut buf = sys::klibc::util::AlignedBuffer::new(size);
        node.read(0, buf.as_bytes_mut()).ok()?;
        Some(buf)
    });
    let elf = elf_buf
        .as_ref()
        .and_then(|buf| ElfFile::parse(buf.as_bytes()).ok());

    let pc = arch::cpu::read_sepc();

    println!("[BACKTRACE] userspace backtrace for {process_name}:");
    print_frame(0, pc, &elf);

    let fdes: Option<Vec<_>> = elf.as_ref().and_then(|e| {
        let (eh_frame_data, base_addr) = e.find_section_data_by_name(".eh_frame")?;
        Some(
            EhFrameParser::new(eh_frame_data)
                .iter(base_addr)
                .collect::<Vec<_>>(),
        )
    });

    let Some(fdes) = fdes else {
        let ra = trap_frame[Register::ra];
        if ra != 0 {
            print_frame(1, ra, &elf);
        }
        return;
    };

    let gp = trap_frame.gp_registers();
    let mut regs = [0usize; 32];
    regs.copy_from_slice(gp);

    const MAX_FRAMES: u32 = 32;
    for frame_num in 1..MAX_FRAMES {
        let ra = regs[1];
        if ra == 0 {
            break;
        }

        print_frame(frame_num, ra, &elf);

        let lookup_addr = ra - 1;
        let Some(fde) = fdes.iter().find(|f| f.contains(lookup_addr)) else {
            break;
        };

        let unwinder = Unwinder::new(fde);
        let row = unwinder.find_row_for_address(lookup_addr);

        let cfa = crate::klibc::util::wrapping_add_signed(
            regs[row.cfa_register.as_usize()],
            row.cfa_offset,
        );

        let mut new_regs = regs;
        new_regs[2] = cfa; // sp = CFA
        new_regs[1] = 0; // reset ra before applying rules

        for (reg_index, rule) in row.register_rules.iter().enumerate() {
            match rule {
                RegisterRule::None => continue,
                RegisterRule::Offset(offset) => {
                    let ptr_addr = crate::klibc::util::wrapping_add_signed(cfa, *offset);
                    let Some(value) = read_userspace_usize(ptr_addr) else {
                        println!("  [backtrace: failed to read userspace memory at {ptr_addr:#x}]");
                        return;
                    };
                    new_regs[reg_index] = value;
                }
            }
        }

        regs = new_regs;
    }
}

fn print_frame(num: u32, addr: usize, elf: &Option<ElfFile<'_>>) {
    if let Some(elf) = elf
        && let Some((name, offset)) = elf.find_symbol(addr)
    {
        println!("  {num}: {addr:#x} <{name}+{offset}>");
        return;
    }
    println!("  {num}: {addr:#x}");
}
