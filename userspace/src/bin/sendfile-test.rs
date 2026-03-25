use std::{io::Write, os::fd::AsRawFd};

unsafe extern "C" {
    fn sendfile(out_fd: i32, in_fd: i32, offset: *mut i64, count: usize) -> isize;
}

fn main() {
    {
        let mut f = std::fs::File::create("/tmp/sf-src").unwrap();
        f.write_all(b"hello sendfile world").unwrap();
    }

    // Test 1: sendfile without offset (uses file position)
    let dest = std::fs::File::create("/tmp/sf-dst").unwrap();
    let src = std::fs::File::open("/tmp/sf-src").unwrap();
    let n = unsafe { sendfile(dest.as_raw_fd(), src.as_raw_fd(), core::ptr::null_mut(), 20) };
    assert_eq!(n, 20);
    drop(dest);
    drop(src);
    let data = std::fs::read_to_string("/tmp/sf-dst").unwrap();
    assert_eq!(data, "hello sendfile world");

    // Test 2: sendfile with offset
    let dest2 = std::fs::File::create("/tmp/sf-dst2").unwrap();
    let src2 = std::fs::File::open("/tmp/sf-src").unwrap();
    let mut off: i64 = 6;
    let n = unsafe { sendfile(dest2.as_raw_fd(), src2.as_raw_fd(), &mut off, 8) };
    assert_eq!(n, 8);
    assert_eq!(off, 14);
    drop(dest2);
    let data = std::fs::read_to_string("/tmp/sf-dst2").unwrap();
    assert_eq!(data, "sendfile");

    println!("sendfile: OK");
}
