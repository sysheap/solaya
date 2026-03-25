use std::{io::Write, os::fd::AsRawFd};

unsafe extern "C" {
    fn pread(fd: i32, buf: *mut u8, count: usize, offset: i64) -> isize;
    fn pwrite(fd: i32, buf: *const u8, count: usize, offset: i64) -> isize;
    fn lseek(fd: i32, offset: i64, whence: i32) -> i64;
}

fn main() {
    let mut f = std::fs::File::create("/tmp/pread-test").unwrap();
    f.write_all(b"hello world").unwrap();
    drop(f);

    let f = std::fs::File::open("/tmp/pread-test").unwrap();
    let fd = f.as_raw_fd();

    let mut buf = [0u8; 5];
    let n = unsafe { pread(fd, buf.as_mut_ptr(), 5, 6) };
    assert_eq!(n, 5);
    assert_eq!(&buf, b"world");

    let pos = unsafe { lseek(fd, 0, 1) };
    assert_eq!(pos, 0);
    drop(f);

    let f = std::fs::OpenOptions::new()
        .write(true)
        .open("/tmp/pread-test")
        .unwrap();
    let fd = f.as_raw_fd();
    let n = unsafe { pwrite(fd, b"ABCDE".as_ptr(), 5, 3) };
    assert_eq!(n, 5);

    let pos = unsafe { lseek(fd, 0, 1) };
    assert_eq!(pos, 0);
    drop(f);

    let data = std::fs::read_to_string("/tmp/pread-test").unwrap();
    assert_eq!(data, "helABCDErld");

    println!("pread_pwrite: OK");
}
