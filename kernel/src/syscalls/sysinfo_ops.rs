use alloc::vec;
use core::{
    ffi::c_int,
    sync::atomic::{AtomicU64, Ordering},
};
use headers::errno::Errno;

use crate::{
    drivers::virtio::rng,
    klibc::util::ByteInterpretable,
    memory::{self, PAGE_SIZE},
    processes::{process_table, timer},
    syscalls::linux_validator::LinuxUserspaceArg,
};

use super::linux::LinuxSyscallHandler;

fn write_cstr(field: &mut [core::ffi::c_char], src: &core::ffi::CStr) {
    for (dst, &b) in field.iter_mut().zip(src.to_bytes_with_nul()) {
        *dst = b as core::ffi::c_char;
    }
}

impl LinuxSyscallHandler {
    pub(super) fn do_uname(&self, buf: LinuxUserspaceArg<*mut u8>) -> Result<isize, Errno> {
        let mut uts = headers::sysinfo_types::utsname::default();
        write_cstr(&mut uts.sysname, c"Linux");
        write_cstr(&mut uts.nodename, c"solaya");
        write_cstr(&mut uts.release, c"6.1.0");
        write_cstr(&mut uts.version, c"#1");
        write_cstr(&mut uts.machine, c"riscv64");
        write_cstr(&mut uts.domainname, c"");
        buf.write_slice(uts.as_slice())?;
        Ok(0)
    }

    pub(super) fn do_sysinfo(&self, info: LinuxUserspaceArg<*mut u8>) -> Result<isize, Errno> {
        let time = timer::current_time();
        let total_pages = memory::total_heap_pages();
        let used_pages = memory::used_heap_pages();

        let si = headers::sysinfo_types::sysinfo {
            uptime: time.tv_sec.cast_unsigned(),
            totalram: (total_pages * PAGE_SIZE) as u64,
            freeram: ((total_pages - used_pages) * PAGE_SIZE) as u64,
            procs: process_table::live_thread_count() as u16,
            mem_unit: 1,
            ..headers::sysinfo_types::sysinfo::default()
        };
        info.write_slice(si.as_slice())?;
        Ok(0)
    }

    pub(super) fn do_getrusage(
        &self,
        _who: c_int,
        usage: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let ru = headers::sysinfo_types::rusage::default();
        usage.write_slice(ru.as_slice())?;
        Ok(0)
    }

    pub(super) fn do_getrandom(
        &self,
        buf: LinuxUserspaceArg<*mut u8>,
        buflen: usize,
    ) -> Result<isize, Errno> {
        let len = buflen.min(256);
        let mut data = vec![0u8; len];

        if rng::is_available() {
            rng::read_random(&mut data);
        } else {
            xorshift_fill(&mut data);
        }

        buf.write_slice(&data)?;
        Ok(len as isize)
    }
}

static PRNG_STATE: AtomicU64 = AtomicU64::new(0);

fn xorshift_fill(buf: &mut [u8]) {
    let mut state = PRNG_STATE.load(Ordering::Relaxed);
    if state == 0 {
        state = arch::timer::get_current_clocks() | 1;
    }
    for byte in buf.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
    PRNG_STATE.store(state, Ordering::Relaxed);
}
