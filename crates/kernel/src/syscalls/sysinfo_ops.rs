use alloc::vec;
use core::{
    ffi::{c_int, c_uint},
    sync::atomic::{AtomicU64, Ordering},
};
use headers::errno::Errno;

use crate::{
    cpu::Cpu,
    drivers::virtio::rng,
    klibc::util::ByteInterpretable,
    memory::{self, PAGE_SIZE},
    processes::{process_table, timer},
    syscalls::linux_validator::LinuxUserspaceArg,
};

use super::linux::LinuxSyscallHandler;

trait CStrField {
    fn copy_from(&mut self, src: &str);
}

impl CStrField for [core::ffi::c_char] {
    fn copy_from(&mut self, src: &str) {
        for (dst, &b) in self.iter_mut().zip(src.as_bytes()) {
            *dst = b as core::ffi::c_char;
        }
        let nul_pos = src.len().min(self.len() - 1);
        self[nul_pos] = 0;
    }
}

impl LinuxSyscallHandler {
    pub(super) fn do_uname(&self, buf: LinuxUserspaceArg<*mut u8>) -> Result<isize, Errno> {
        let mut uts = headers::sysinfo_types::utsname::default();
        uts.sysname.copy_from("Linux");
        uts.nodename.copy_from("solaya");
        uts.release.copy_from("6.1.0");
        uts.version.copy_from("#1");
        uts.machine.copy_from("riscv64");
        uts.domainname.copy_from("");
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

    pub(super) fn do_getrlimit(
        &self,
        resource: c_uint,
        rlim: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let (soft, hard) = get_default_rlimit(resource);
        let rl = headers::sysinfo_types::rlimit {
            rlim_cur: soft,
            rlim_max: hard,
        };
        rlim.write_slice(rl.as_slice())?;
        Ok(0)
    }

    pub(super) fn do_prlimit64(
        &self,
        pid: c_int,
        resource: c_uint,
        old_limit: LinuxUserspaceArg<Option<*mut u8>>,
    ) -> Result<isize, Errno> {
        if pid != 0 {
            let my_pid = self.current_process.with_lock(|p| p.main_tid());
            if pid != my_pid.as_isize() as c_int {
                return Err(Errno::ESRCH);
            }
        }
        if old_limit.arg_nonzero() {
            let (soft, hard) = get_default_rlimit(resource);
            let rl = headers::sysinfo_types::rlimit {
                rlim_cur: soft,
                rlim_max: hard,
            };
            let ptr = LinuxUserspaceArg::<*mut u8>::new(
                old_limit.raw_arg(),
                self.current_process.clone(),
            );
            ptr.write_slice(rl.as_slice())?;
        }
        Ok(0)
    }

    pub(super) fn do_sched_getaffinity(
        &self,
        _pid: c_int,
        cpusetsize: usize,
        mask: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let num_cpus = Cpu::current().number_cpus();
        let bytes_needed = num_cpus.div_ceil(8);
        if cpusetsize < bytes_needed {
            return Err(Errno::EINVAL);
        }
        let mut buf = vec![0u8; bytes_needed];
        for i in 0..num_cpus {
            buf[i / 8] |= 1 << (i % 8);
        }
        mask.write_slice(&buf)?;
        Ok(bytes_needed as isize)
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

fn get_default_rlimit(resource: u32) -> (u64, u64) {
    let unlimited = u64::MAX;
    match resource {
        headers::sysinfo_types::RLIMIT_STACK => (8 * 1024 * 1024, unlimited),
        headers::sysinfo_types::RLIMIT_NOFILE => (1024, 4096),
        headers::sysinfo_types::RLIMIT_CORE => (0, 0),
        _ => (unlimited, unlimited),
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
