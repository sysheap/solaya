// Regression test for resolve_relative `..` / relative-symlink handling.
//
// Pre-fix, openat(dirfd, "..", ...) walked `..` against `/` (base_abs
// was hard-coded to "/") instead of against the dirfd's real directory,
// and relative symlinks resolved from the same wrong base.

use std::{ffi::CString, io::Write};

unsafe extern "C" {
    fn open(path: *const u8, flags: i32) -> i32;
    fn openat(dirfd: i32, path: *const u8, flags: i32) -> i32;
    fn close(fd: i32) -> i32;
    fn symlink(target: *const u8, linkpath: *const u8) -> i32;
    fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
}

const O_RDONLY: i32 = 0;
const O_DIRECTORY: i32 = 0o200000;

fn main() {
    // Setup: /tmp/dirfd-test-{dir,target}
    std::fs::create_dir_all("/tmp/dirfd-test-dir").expect("mkdir dir");
    let target_path = "/tmp/dirfd-test-target";
    {
        let mut f = std::fs::File::create(target_path).expect("create target");
        f.write_all(b"TARGET\n").expect("write target");
    }

    // link: /tmp/dirfd-test-dir/link -> ../dirfd-test-target
    let link = CString::new("/tmp/dirfd-test-dir/link").unwrap();
    let rel = CString::new("../dirfd-test-target").unwrap();
    let _ = std::fs::remove_file(link.to_str().unwrap());
    let rc = unsafe { symlink(rel.as_ptr().cast(), link.as_ptr().cast()) };
    assert_eq!(
        rc,
        0,
        "symlink failed (errno may be {})",
        std::io::Error::last_os_error()
    );

    // Open the dir to get a dirfd.
    let dir = CString::new("/tmp/dirfd-test-dir").unwrap();
    let dirfd = unsafe { open(dir.as_ptr().cast(), O_RDONLY | O_DIRECTORY) };
    assert!(dirfd >= 0, "open /tmp/dirfd-test-dir failed");

    // Test 1: openat(dirfd, "..", O_DIRECTORY) must succeed and refer to /tmp.
    // Pre-fix, `..` popped from hard-coded "/", which kept us at "/" — the
    // lookup then walked *past* the dirfd instead of back to /tmp.
    let dotdot = CString::new("..").unwrap();
    let parent_fd = unsafe { openat(dirfd, dotdot.as_ptr().cast(), O_RDONLY | O_DIRECTORY) };
    assert!(parent_fd >= 0, "openat(dirfd, \"..\") failed");
    unsafe { close(parent_fd) };
    println!("OK dotdot");

    // Test 2: openat(dirfd, "link", O_RDONLY) must follow the relative
    // symlink "../dirfd-test-target" relative to dirfd (i.e. to
    // /tmp/dirfd-test-target) and return the 7-byte target file.
    let link_rel = CString::new("link").unwrap();
    let fd = unsafe { openat(dirfd, link_rel.as_ptr().cast(), O_RDONLY) };
    assert!(fd >= 0, "openat(dirfd, \"link\") failed");
    let mut buf = [0u8; 16];
    let n = unsafe { read(fd, buf.as_mut_ptr(), buf.len()) };
    unsafe { close(fd) };
    assert!(n > 0, "read from symlink target returned {n}");
    let got = core::str::from_utf8(&buf[..n as usize]).unwrap();
    assert_eq!(got, "TARGET\n", "symlink target content mismatch: {got:?}");
    println!("OK relative_symlink");

    unsafe { close(dirfd) };

    // Cleanup
    let _ = std::fs::remove_file(link.to_str().unwrap());
    let _ = std::fs::remove_file(target_path);
    let _ = std::fs::remove_dir("/tmp/dirfd-test-dir");
}
