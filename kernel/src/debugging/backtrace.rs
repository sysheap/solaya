#![allow(unsafe_code)]
use super::eh_frame_parser;
use crate::{
    assert::static_assert_size,
    cpu::KERNEL_STACK_SIZE,
    debugging::{
        self,
        eh_frame_parser::EhFrameParser,
        unwinder::{RegisterRule, Unwinder},
    },
    info,
    klibc::{runtime_initialized::RuntimeInitializedData, util::UsizeExt},
    memory::{address::VirtAddr, linker_information::LinkerInformation},
};
use alloc::vec::Vec;
// Needed for the native backtrace impl for debugging purposes
// use core::ffi::c_void;
// use unwinding::abi::{
//     UnwindContext, UnwindReasonCode, _Unwind_Backtrace, _Unwind_GetIP, with_context,
// };

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

/// We keep the already parsed information in a Vec
/// even though we might not even need to produce a backtrace
/// But we want to avoid heap allocation while backtracing
/// in case of memory corruption.
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

        let eh_frame = sys::memory::linker_region_as_slice(
            sys::memory::VirtAddr::new(eh_frame_start.as_usize()),
            eh_frame_size,
        );

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

        // RA points to the next instruction. Move it back one byte such
        // that it points into the previous instruction.
        // This case must be handled different as soon as we have
        // signal trampolines.
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
                    let ptr = crate::klibc::util::wrapping_add_signed(cfa, *offset) as *const usize;
                    // SAFETY: ptr is CFA + offset, which points to the saved
                    // register value on the stack frame.
                    unsafe { ptr.read() }
                }
            };
            new_regs[reg_index] = value;
        }

        *regs = new_regs;

        Ok(ra)
    }
}

// We leave that here for debugging purposes
// I'm not entirely sure if my own backtrace implementation
// is fault free. But we will see that in the future.
// After multiple months of implementing this I'm done and want to move forward
// to something else.
// fn print_native() {
//     #[derive(Default)]
//     struct CallbackData {
//         counter: usize,
//     }

//     extern "C" fn callback(unwind_ctx: &UnwindContext<'_>, arg: *mut c_void) -> UnwindReasonCode {
//         let data = unsafe { &mut *(arg as *mut CallbackData) };
//         data.counter += 1;
//         info!("{}: {:#x}", data.counter, _Unwind_GetIP(unwind_ctx));
//         UnwindReasonCode::NO_REASON
//     }

//     let mut data = CallbackData::default();

//     _Unwind_Backtrace(callback, &mut data as *mut _ as _);
// }

/// You ask where I got the registers from? This is a good question.
/// I just looked what registers were mentioned in the eh_frame and added those.
/// Maybe there will be more in the future, then we have to add them.
/// I tried to generate the following code via a macro. However this is not possible,
/// because they won't allow to concatenate x$num_reg as a identifier and I need the
/// literal number to access it via an index.
#[derive(Debug, Clone, Default)]
struct CalleeSavedRegs {
    x1: usize,
    x2: usize,
    x8: usize,
    x9: usize,
    x18: usize,
    x19: usize,
    x20: usize,
    x21: usize,
    x22: usize,
    x23: usize,
    x24: usize,
    x25: usize,
    x26: usize,
    x27: usize,
}

impl core::fmt::Display for CalleeSavedRegs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        macro_rules! print_reg {
            ($reg:ident) => {
                writeln!(f, "{}: {:#x}", stringify!($reg), self.$reg)?
            };
        }

        print_reg!(x1);
        print_reg!(x2);
        print_reg!(x8);
        print_reg!(x9);
        print_reg!(x18);
        print_reg!(x19);
        print_reg!(x20);
        print_reg!(x21);
        print_reg!(x22);
        print_reg!(x23);
        print_reg!(x24);
        print_reg!(x25);
        print_reg!(x26);
        print_reg!(x27);

        Ok(())
    }
}

impl core::ops::Index<usize> for CalleeSavedRegs {
    type Output = usize;

    fn index(&self, index: usize) -> &Self::Output {
        match index {
            1 => &self.x1,
            2 => &self.x2,
            8 => &self.x8,
            9 => &self.x9,
            18 => &self.x18,
            19 => &self.x19,
            20 => &self.x20,
            21 => &self.x21,
            22 => &self.x22,
            23 => &self.x23,
            24 => &self.x24,
            25 => &self.x25,
            26 => &self.x26,
            27 => &self.x27,
            _ => panic!("Invalid index"),
        }
    }
}

impl core::ops::IndexMut<usize> for CalleeSavedRegs {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index {
            1 => &mut self.x1,
            2 => &mut self.x2,
            8 => &mut self.x8,
            9 => &mut self.x9,
            18 => &mut self.x18,
            19 => &mut self.x19,
            20 => &mut self.x20,
            21 => &mut self.x21,
            22 => &mut self.x22,
            23 => &mut self.x23,
            24 => &mut self.x24,
            25 => &mut self.x25,
            26 => &mut self.x26,
            27 => &mut self.x27,
            _ => panic!("Invalid index"),
        }
    }
}

// This value is referenced in the assembly of extern "C-unwind" fn dispatch
static_assert_size!(CalleeSavedRegs, 0x70);

impl CalleeSavedRegs {
    fn ra(&self) -> usize {
        self.x1
    }

    fn set_ra(&mut self, value: usize) {
        self.x1 = value;
    }

    fn sp(&self) -> usize {
        self.x2
    }

    fn set_sp(&mut self, value: usize) {
        self.x2 = value;
    }

    fn with_context<F: FnMut(&mut CalleeSavedRegs)>(f: F) {
        // Inspired by the unwinder crate
        // https://github.com/nbdd0121/unwinding/

        // We cannot call a closure directly from assembly
        // because we're missing some compiler magic.
        // Convert the closure to a fn pointer by having a
        // intermediate function closure_to_fn_pointer.

        // Not the prettiest code but very cool and also
        // very convenient for the caller side.

        #[repr(C)]
        struct ClosureWrapper<F: FnMut(&mut CalleeSavedRegs)>(F);

        let mut closure = ClosureWrapper(f);

        dispatch(
            &mut CalleeSavedRegs::default(),
            &mut closure,
            closure_to_fn_pointer,
        );

        extern "C" fn closure_to_fn_pointer<F: FnMut(&mut CalleeSavedRegs)>(
            regs: &mut CalleeSavedRegs,
            f_data: &mut ClosureWrapper<F>,
        ) {
            (f_data.0)(regs);
        }

        // SAFETY: Naked function that captures callee-saved registers (s0-s11,
        // ra, sp) into the CalleeSavedRegs struct, then calls the closure.
        // No prologue is generated so we get the true register state.
        #[unsafe(naked)]
        extern "C-unwind" fn dispatch<F: FnMut(&mut CalleeSavedRegs)>(
            regs: &mut CalleeSavedRegs,
            f_data: &mut ClosureWrapper<F>,
            f: extern "C" fn(&mut CalleeSavedRegs, &mut ClosureWrapper<F>),
        ) {
            core::arch::naked_asm!(
                "
                     # regs is in a0
                     # f to call in a2
                     sd x1, 0x00(a0)   
                     sd x2, 0x08(a0)
                     sd x8, 0x10(a0)
                     sd x9, 0x18(a0)
                     sd x18, 0x20(a0)
                     sd x19, 0x28(a0)
                     sd x20, 0x30(a0)
                     sd x21, 0x38(a0)
                     sd x22, 0x40(a0)
                     sd x23, 0x48(a0)
                     sd x24, 0x50(a0)
                     sd x25, 0x58(a0)
                     sd x26, 0x60(a0)
                     sd x27, 0x68(a0)
                     # Save return address on stack
                     # It is important to change the stack
                     # pointer after the previous instructions
                     # Otherwise the wrong sp is saved (x2 == sp)
                     addi sp, sp, -0x08
                     sd ra, 0x00(sp)
                     jalr a2
                     ld ra, 0x00(sp)
                     addi sp, sp, 0x08
                     ret
                    "
            )
        }
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
    // Per-CPU kernel stacks are mapped at the top of the address space
    let stack_bottom = 0usize.wrapping_sub(KERNEL_STACK_SIZE);
    // Validate SP is within the per-CPU kernel stack before scanning
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
        // SAFETY: slot_addr is within the per-CPU kernel stack (validated above).
        let value = unsafe { (slot_addr as *const usize).read() };
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
    use crate::debugging::backtrace::{Backtrace, BacktraceNextError, CalleeSavedRegs};
    use alloc::collections::VecDeque;
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

            // Skip some items because they are inside the unwind functions itself
            data.addresses.pop_front();
            data.addresses.pop_front();
            own_addr.pop_front();

            assert_eq!(own_addr, data.addresses);
        });
    }
}
