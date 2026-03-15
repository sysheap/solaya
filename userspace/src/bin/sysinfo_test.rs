use std::arch::asm;

const SYS_UNAME: usize = 160;
const SYS_SYSINFO: usize = 179;
const SYS_GETRUSAGE: usize = 165;
const SYS_GETRANDOM: usize = 278;

unsafe fn syscall1(nr: usize, arg0: usize) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a7") nr,
            in("a0") arg0,
            lateout("a0") ret,
        );
    }
    ret
}

unsafe fn syscall3(nr: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a7") nr,
            in("a0") arg0,
            in("a1") arg1,
            in("a2") arg2,
            lateout("a0") ret,
        );
    }
    ret
}

unsafe fn syscall2(nr: usize, arg0: usize, arg1: usize) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "ecall",
            in("a7") nr,
            in("a0") arg0,
            in("a1") arg1,
            lateout("a0") ret,
        );
    }
    ret
}

#[repr(C)]
struct Utsname {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

#[repr(C)]
struct Sysinfo {
    uptime: u64,
    loads: [u64; 3],
    totalram: u64,
    freeram: u64,
    sharedram: u64,
    bufferram: u64,
    totalswap: u64,
    freeswap: u64,
    procs: u16,
    pad: u16,
    _pad2: u32,
    totalhigh: u64,
    freehigh: u64,
    mem_unit: u32,
    _reserved: [u8; 256],
}

fn cstr(buf: &[u8]) -> &str {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    core::str::from_utf8(&buf[..len]).unwrap_or("???")
}

fn main() {
    // SAFETY: These are C-style structs that are valid when zero-initialized.
    let mut uts: Utsname = unsafe { core::mem::zeroed() };
    let ret = unsafe { syscall1(SYS_UNAME, &mut uts as *mut Utsname as usize) };
    assert!(ret == 0, "uname failed");
    println!(
        "uname: {} {} {} {}",
        cstr(&uts.sysname),
        cstr(&uts.nodename),
        cstr(&uts.release),
        cstr(&uts.machine)
    );
    println!("OK uname");

    let mut si: Sysinfo = unsafe { core::mem::zeroed() };
    let ret = unsafe { syscall1(SYS_SYSINFO, &mut si as *mut Sysinfo as usize) };
    assert!(ret == 0, "sysinfo failed");
    println!(
        "sysinfo: totalram={} freeram={} procs={}",
        si.totalram, si.freeram, si.procs
    );
    assert!(si.totalram > 0, "totalram should be > 0");
    assert!(si.freeram > 0, "freeram should be > 0");
    assert!(si.procs > 0, "procs should be > 0");
    println!("OK sysinfo");

    let mut buf1 = [0u8; 16];
    let mut buf2 = [0u8; 16];
    let ret = unsafe { syscall3(SYS_GETRANDOM, buf1.as_mut_ptr() as usize, buf1.len(), 0) };
    assert!(ret == 16, "getrandom failed");
    let ret = unsafe { syscall3(SYS_GETRANDOM, buf2.as_mut_ptr() as usize, buf2.len(), 0) };
    assert!(ret == 16, "getrandom failed");
    print!("random1:");
    for b in &buf1 {
        print!(" {:02x}", b);
    }
    println!();
    print!("random2:");
    for b in &buf2 {
        print!(" {:02x}", b);
    }
    println!();
    assert!(buf1 != buf2, "two getrandom calls should differ");
    println!("OK getrandom");

    let mut ru = [0u8; 512];
    let ret = unsafe { syscall2(SYS_GETRUSAGE, 0, ru.as_mut_ptr() as usize) };
    assert!(ret == 0, "getrusage failed");
    println!("OK getrusage");
}
