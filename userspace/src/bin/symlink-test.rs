use std::io::Write;

unsafe extern "C" {
    fn symlink(target: *const u8, linkpath: *const u8) -> i32;
    fn readlink(path: *const u8, buf: *mut u8, bufsiz: usize) -> isize;
    fn link(oldpath: *const u8, newpath: *const u8) -> i32;
    fn rename(oldpath: *const u8, newpath: *const u8) -> i32;
}

fn main() {
    {
        let mut f = std::fs::File::create("/tmp/target").unwrap();
        f.write_all(b"hello symlink").unwrap();
    }

    assert_eq!(
        unsafe {
            symlink(
                c"/tmp/target".as_ptr().cast(),
                c"/tmp/mylink".as_ptr().cast(),
            )
        },
        0
    );

    let data = std::fs::read_to_string("/tmp/mylink").unwrap();
    assert_eq!(data, "hello symlink");

    let mut buf = [0u8; 256];
    let n = unsafe { readlink(c"/tmp/mylink".as_ptr().cast(), buf.as_mut_ptr(), 256) };
    assert!(n > 0);
    let target = core::str::from_utf8(&buf[..n as usize]).unwrap();
    assert_eq!(target, "/tmp/target");

    assert_eq!(
        unsafe {
            link(
                c"/tmp/target".as_ptr().cast(),
                c"/tmp/hardlink".as_ptr().cast(),
            )
        },
        0
    );

    let data2 = std::fs::read_to_string("/tmp/hardlink").unwrap();
    assert_eq!(data2, "hello symlink");

    assert_eq!(
        unsafe {
            rename(
                c"/tmp/target".as_ptr().cast(),
                c"/tmp/renamed".as_ptr().cast(),
            )
        },
        0
    );
    let data3 = std::fs::read_to_string("/tmp/renamed").unwrap();
    assert_eq!(data3, "hello symlink");
    assert!(std::fs::read_to_string("/tmp/target").is_err());

    println!("symlink_test: OK");
}
