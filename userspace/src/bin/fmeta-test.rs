use std::{io::Write, os::unix::io::AsRawFd};

unsafe extern "C" {
    fn fchmod(fd: i32, mode: u32) -> i32;
    fn fchown(fd: i32, owner: u32, group: u32) -> i32;
    fn ftruncate(fd: i32, length: i64) -> i32;
    fn fstat(fd: i32, buf: *mut u8) -> i32;
}

#[repr(C)]
#[derive(Default)]
struct Stat {
    st_dev: u64,
    st_ino: u64,
    st_mode: u32,
    st_nlink: u32,
    st_uid: u32,
    st_gid: u32,
    st_rdev: u64,
    _pad1: u64,
    st_size: i64,
    st_blksize: i32,
    _pad2: i32,
    st_blocks: i64,
    st_atime: i64,
    st_atime_nsec: i64,
    st_mtime: i64,
    st_mtime_nsec: i64,
    st_ctime: i64,
    st_ctime_nsec: i64,
    _unused: [i32; 2],
}

fn do_fstat(fd: i32) -> Stat {
    let mut st = Stat::default();
    let ret = unsafe { fstat(fd, &mut st as *mut Stat as *mut u8) };
    assert!(ret == 0, "fstat failed");
    st
}

fn main() {
    let path = "/tmp/fmeta-test";
    let mut f = std::fs::File::create(path).expect("create failed");
    f.write_all(b"hello world").expect("write failed");
    let fd = f.as_raw_fd();

    // Test ftruncate: grow
    let ret = unsafe { ftruncate(fd, 20) };
    assert!(ret == 0, "ftruncate grow failed");
    let st = do_fstat(fd);
    assert!(st.st_size == 20, "size after grow should be 20");
    println!("OK ftruncate_grow");

    // Test ftruncate: shrink
    let ret = unsafe { ftruncate(fd, 5) };
    assert!(ret == 0, "ftruncate shrink failed");
    let st = do_fstat(fd);
    assert!(st.st_size == 5, "size after shrink should be 5");
    println!("OK ftruncate_shrink");

    // Test fchmod
    let ret = unsafe { fchmod(fd, 0o755) };
    assert!(ret == 0, "fchmod failed");
    let st = do_fstat(fd);
    assert!(
        st.st_mode & 0o7777 == 0o755,
        "mode should be 0755 after fchmod"
    );
    println!("OK fchmod");

    // Test fchown
    let ret = unsafe { fchown(fd, 1000, 1000) };
    assert!(ret == 0, "fchown failed");
    let st = do_fstat(fd);
    assert!(st.st_uid == 1000, "uid should be 1000");
    assert!(st.st_gid == 1000, "gid should be 1000");
    println!("OK fchown");

    // Test fchown with -1 (don't change)
    let ret = unsafe { fchown(fd, u32::MAX, 2000) };
    assert!(ret == 0, "fchown partial failed");
    let st = do_fstat(fd);
    assert!(st.st_uid == 1000, "uid should still be 1000");
    assert!(st.st_gid == 2000, "gid should be 2000");
    println!("OK fchown_partial");
}
