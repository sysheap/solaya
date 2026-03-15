unsafe extern "C" {
    fn uname(buf: *mut headers::sysinfo_types::utsname) -> i32;
    fn sysinfo(info: *mut headers::sysinfo_types::sysinfo) -> i32;
    fn getrusage(who: i32, usage: *mut headers::sysinfo_types::rusage) -> i32;
    fn getrandom(buf: *mut u8, buflen: usize, flags: u32) -> isize;
}

fn cstr(buf: &[core::ffi::c_char]) -> &str {
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
        .to_str()
        .unwrap_or("???")
}

fn main() {
    let mut uts = headers::sysinfo_types::utsname::default();
    let ret = unsafe { uname(&mut uts) };
    assert!(ret == 0, "uname failed");
    println!(
        "uname: {} {} {} {}",
        cstr(&uts.sysname),
        cstr(&uts.nodename),
        cstr(&uts.release),
        cstr(&uts.machine)
    );
    println!("OK uname");

    let mut si = headers::sysinfo_types::sysinfo::default();
    let ret = unsafe { sysinfo(&mut si) };
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
    let ret = unsafe { getrandom(buf1.as_mut_ptr(), buf1.len(), 0) };
    assert!(ret == 16, "getrandom failed");
    let ret = unsafe { getrandom(buf2.as_mut_ptr(), buf2.len(), 0) };
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

    let mut ru = headers::sysinfo_types::rusage::default();
    let ret = unsafe { getrusage(0, &mut ru) };
    assert!(ret == 0, "getrusage failed");
    println!("OK getrusage");
}
