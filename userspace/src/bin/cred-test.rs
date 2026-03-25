unsafe extern "C" {
    fn getuid() -> u32;
    fn geteuid() -> u32;
    fn getgid() -> u32;
    fn getegid() -> u32;
    fn setuid(uid: u32) -> i32;
    fn setgid(gid: u32) -> i32;
    fn getresuid(ruid: *mut u32, euid: *mut u32, suid: *mut u32) -> i32;
    fn getresgid(rgid: *mut u32, egid: *mut u32, sgid: *mut u32) -> i32;
    fn setresuid(ruid: u32, euid: u32, suid: u32) -> i32;
    fn getgroups(size: i32, list: *mut u32) -> i32;
    fn setgroups(size: usize, list: *const u32) -> i32;
    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
    fn fork() -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}

fn main() {
    assert_eq!(unsafe { getuid() }, 0);
    assert_eq!(unsafe { geteuid() }, 0);
    assert_eq!(unsafe { getgid() }, 0);
    assert_eq!(unsafe { getegid() }, 0);

    assert_eq!(unsafe { setuid(1000) }, 0);
    assert_eq!(unsafe { getuid() }, 1000);
    assert_eq!(unsafe { geteuid() }, 1000);

    assert_eq!(unsafe { setgid(500) }, 0);
    assert_eq!(unsafe { getgid() }, 500);
    assert_eq!(unsafe { getegid() }, 500);

    let (mut r, mut e, mut s) = (0u32, 0u32, 0u32);
    assert_eq!(unsafe { getresuid(&mut r, &mut e, &mut s) }, 0);
    assert_eq!(r, 1000);
    assert_eq!(e, 1000);
    assert_eq!(s, 1000);

    let (mut rg, mut eg, mut sg) = (0u32, 0u32, 0u32);
    assert_eq!(unsafe { getresgid(&mut rg, &mut eg, &mut sg) }, 0);
    assert_eq!(rg, 500);
    assert_eq!(eg, 500);
    assert_eq!(sg, 500);

    assert_eq!(unsafe { setresuid(2000, 2001, 2002) }, 0);
    let (mut r2, mut e2, mut s2) = (0u32, 0u32, 0u32);
    assert_eq!(unsafe { getresuid(&mut r2, &mut e2, &mut s2) }, 0);
    assert_eq!(r2, 2000);
    assert_eq!(e2, 2001);
    assert_eq!(s2, 2002);

    // Restore uid to 1000 for fork test
    assert_eq!(unsafe { setuid(1000) }, 0);

    let groups = [100u32, 200, 300];
    assert_eq!(unsafe { setgroups(3, groups.as_ptr()) }, 0);
    let count = unsafe { getgroups(0, core::ptr::null_mut()) };
    assert_eq!(count, 3);

    let mut retrieved = [0u32; 3];
    assert_eq!(unsafe { getgroups(3, retrieved.as_mut_ptr()) }, 3);
    assert_eq!(retrieved, [100, 200, 300]);

    let name = b"mythread\0\0\0\0\0\0\0\0";
    assert_eq!(unsafe { prctl(15, name.as_ptr() as u64, 0, 0, 0) }, 0);
    let mut buf = [0u8; 16];
    assert_eq!(unsafe { prctl(16, buf.as_mut_ptr() as u64, 0, 0, 0) }, 0);
    assert_eq!(&buf[..8], b"mythread");

    let pid = unsafe { fork() };
    if pid == 0 {
        assert_eq!(unsafe { getuid() }, 1000);
        assert_eq!(unsafe { getgid() }, 500);
        println!("child-ok");
        std::process::exit(0);
    } else {
        let mut status = 0;
        unsafe { waitpid(pid, &mut status, 0) };
    }

    println!("cred_test: OK");
}
