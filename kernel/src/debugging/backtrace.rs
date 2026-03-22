use super::eh_frame_parser;
use crate::{
    cpu::KERNEL_STACK_SIZE,
    debugging::{
        self,
        eh_frame_parser::EhFrameParser,
        unwinder::{RegisterRule, Unwinder},
    },
    info,
    klibc::{runtime_initialized::RuntimeInitializedData, util::UsizeExt},
    memory::{VirtAddr, linker_information::LinkerInformation},
};
use alloc::vec::Vec;
use arch::backtrace::CalleeSavedRegs;
use sys::klibc::validated_ptr::ValidatedPtr;

#[allow(dead_code)]
#[derive(Debug)]
enum BacktraceNextError {
    RaIsZero,
    CouldNotGetFde(usize),
    RaOutsideText(usize),
}

fn is_in_text_segment(address: usize) -> bool {
    LinkerInformation::text_range().contains(&VirtAddr::new(address))
}

struct Backtrace<'a> {
    fdes: Vec<eh_frame_parser::ParsedFDE<'a>>,
}

static BACKTRACE: RuntimeInitializedData<Backtrace> = RuntimeInitializedData::new();

impl<'a> Backtrace<'a> {
    fn new() -> Self {
        let mut self_ = Self { fdes: Vec::new() };
        self_.init();
        self_
    }

    fn find(&self, pc: usize) -> Option<&eh_frame_parser::ParsedFDE<'a>> {
        self.fdes.iter().find(|&fde| fde.contains(pc))
    }

    fn init(&mut self) {
        assert!(self.fdes.is_empty(), "Init can only be called once.");

        let eh_frame_start = LinkerInformation::__start_eh_frame();
        let eh_frame_size = LinkerInformation::eh_frame_size();

        info!(
            "Initialize backtrace with eh_frame at {} and size {:#x}",
            eh_frame_start, eh_frame_size
        );

        let eh_frame = ValidatedPtr::<u8>::from_trusted(eh_frame_start.as_usize() as *const u8)
            .as_static_slice(eh_frame_size);

        let eh_frame_parser = EhFrameParser::new(eh_frame);
        let eh_frames = eh_frame_parser.iter(eh_frame_start.as_usize());

        for frame in eh_frames {
            self.fdes.push(frame);
        }
    }

    fn next(&self, regs: &mut CalleeSavedRegs) -> Result<usize, BacktraceNextError> {
        let ra = regs.ra();

        if ra == 0 {
            return Err(BacktraceNextError::RaIsZero);
        }

        if !is_in_text_segment(ra) {
            return Err(BacktraceNextError::RaOutsideText(ra));
        }

        let fde = self
            .find(ra - 1)
            .ok_or(BacktraceNextError::CouldNotGetFde(ra))?;

        let unwinder = Unwinder::new(fde);

        let row = unwinder.find_row_for_address(ra);

        let cfa = crate::klibc::util::wrapping_add_signed(
            regs[row.cfa_register.as_usize()],
            row.cfa_offset,
        );

        let mut new_regs = regs.clone();
        new_regs.set_sp(cfa);
        new_regs.set_ra(0);

        for (reg_index, rule) in row.register_rules.iter().enumerate() {
            let value = match rule {
                RegisterRule::None => {
                    continue;
                }
                RegisterRule::Offset(offset) => {
                    let addr = crate::klibc::util::wrapping_add_signed(cfa, *offset);
                    ValidatedPtr::<usize>::from_trusted(addr as *const usize).read()
                }
            };
            new_regs[reg_index] = value;
        }

        *regs = new_regs;

        Ok(ra)
    }
}

pub fn init() {
    BACKTRACE.initialize(Backtrace::new());
}

pub fn print() {
    CalleeSavedRegs::with_context(|regs| {
        let mut counter = 0u64;
        let mut last_sp = regs.sp();
        loop {
            match BACKTRACE.next(regs) {
                Ok(address) => {
                    print_stacktrace_frame(counter, address);
                    counter += 1;
                    last_sp = regs.sp();
                }
                Err(BacktraceNextError::RaIsZero) => {
                    info!("{counter}: 0x0");
                    break;
                }
                Err(BacktraceNextError::CouldNotGetFde(address)) => {
                    print_stacktrace_frame(counter, address);
                    counter += 1;
                    info!("  --- DWARF unwinding lost, scanning stack ---");
                    scan_stack_for_return_addresses(last_sp, &mut counter);
                    break;
                }
                Err(BacktraceNextError::RaOutsideText(address)) => {
                    info!("  RA {address:#x} outside text segment, scanning stack");
                    info!("  --- DWARF unwinding lost, scanning stack ---");
                    scan_stack_for_return_addresses(last_sp, &mut counter);
                    break;
                }
            }
        }
    });
}

const MAX_STACK_SCAN_SLOTS: usize = 512;

fn scan_stack_for_return_addresses(sp: usize, counter: &mut u64) {
    let stack_bottom = 0usize.wrapping_sub(KERNEL_STACK_SIZE);
    if sp < stack_bottom {
        info!("  SP {sp:#x} outside kernel stack, cannot scan");
        return;
    }
    let remaining_bytes = 0usize.wrapping_sub(sp);
    let remaining_slots = remaining_bytes / size_of::<usize>();
    let slots_to_scan = remaining_slots.min(MAX_STACK_SCAN_SLOTS);

    let text_range = LinkerInformation::text_range();
    for i in 0..slots_to_scan {
        let slot_addr = sp.wrapping_add(i * size_of::<usize>());
        let value = ValidatedPtr::<usize>::from_trusted(slot_addr as *const usize).read();
        if text_range.contains(&VirtAddr::new(value)) {
            print_uncertain_stacktrace_frame(*counter, value);
            *counter += 1;
        }
    }
}

fn print_stacktrace_frame(counter: u64, address: usize) {
    print_stacktrace_frame_inner(counter, address, "");
}

fn print_uncertain_stacktrace_frame(counter: u64, address: usize) {
    print_stacktrace_frame_inner(counter, address, "[?] ");
}

fn print_stacktrace_frame_inner(counter: u64, address: usize, prefix: &str) {
    let symbol = debugging::symbols::get_symbol(address);
    if let Some(symbol) = symbol {
        let offset = address - symbol.address;
        if let Some(file) = symbol.file {
            info!(
                "{counter}: {prefix}{address:#x} <{}+{}>\n\t\t{}\n",
                symbol.symbol, offset, file
            );
        } else {
            info!(
                "{counter}: {prefix}{address:#x} <{}+{}>\n",
                symbol.symbol, offset
            );
        }
    } else {
        info!("{counter}: {prefix}{address:#x}\n");
    }
}

#[cfg(not(miri))]
#[cfg(test)]
mod tests {
    #![allow(unsafe_code)]
    use crate::debugging::backtrace::{Backtrace, BacktraceNextError};
    use alloc::collections::VecDeque;
    use arch::backtrace::CalleeSavedRegs;
    use core::ffi::c_void;
    use unwinding::abi::{_Unwind_Backtrace, _Unwind_GetIP, UnwindContext, UnwindReasonCode};

    #[test_case]
    fn backtrace() {
        #[derive(Default)]
        struct CallbackData {
            addresses: VecDeque<usize>,
        }

        extern "C" fn callback(
            unwind_ctx: &UnwindContext<'_>,
            arg: *mut c_void,
        ) -> UnwindReasonCode {
            // SAFETY: arg was cast from &mut CallbackData in the caller below;
            // the callback is invoked synchronously so the reference is valid.
            let data = unsafe { &mut *arg.cast::<CallbackData>() };
            data.addresses.push_back(_Unwind_GetIP(unwind_ctx));
            UnwindReasonCode::NO_REASON
        }

        let mut data = CallbackData::default();

        _Unwind_Backtrace(callback, (&mut data as *mut CallbackData).cast());
        CalleeSavedRegs::with_context(|regs| {
            let backtrace = Backtrace::new();
            let mut own_addr = VecDeque::new();

            loop {
                match backtrace.next(regs) {
                    Ok(address) => {
                        own_addr.push_back(address);
                    }
                    Err(BacktraceNextError::RaIsZero) => {
                        own_addr.push_back(0);
                        break;
                    }
                    Err(BacktraceNextError::CouldNotGetFde(address))
                    | Err(BacktraceNextError::RaOutsideText(address)) => {
                        own_addr.push_back(address);
                        break;
                    }
                }
            }

            data.addresses.pop_front();
            data.addresses.pop_front();
            own_addr.pop_front();

            assert_eq!(own_addr, data.addresses);
        });
    }
}
