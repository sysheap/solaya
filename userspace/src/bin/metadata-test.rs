use std::os::raw::c_int;

unsafe extern "C" {
    fn open(path: *const u8, flags: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn fstat(fd: c_int, buf: *mut u8) -> c_int;
    fn statfs(path: *const u8, buf: *mut u8) -> c_int;
}

fn main() {
    // Create a file
    let fd = unsafe { open(c"/tmp/meta-test".as_ptr().cast(), 0x42, 0o666) }; // O_CREAT|O_RDWR
    assert!(fd >= 0, "open failed: {fd}");

    // fstat: check mode = S_IFREG | 0o644 = 0o100644
    let mut stat_buf = [0u8; 128];
    assert_eq!(unsafe { fstat(fd, stat_buf.as_mut_ptr()) }, 0);
    // st_mode is at offset 16 (after st_dev(8) + st_ino(8)), size u32
    let st_mode = u32::from_le_bytes([stat_buf[16], stat_buf[17], stat_buf[18], stat_buf[19]]);
    assert_eq!(
        st_mode, 0o100644,
        "mode should be 0o100644, got {st_mode:#o}"
    );
    // st_nlink at offset 20, size u32
    let st_nlink = u32::from_le_bytes([stat_buf[20], stat_buf[21], stat_buf[22], stat_buf[23]]);
    assert_eq!(st_nlink, 1, "nlink should be 1, got {st_nlink}");
    unsafe { close(fd) };

    // O_EXCL on existing file should fail
    let fd = unsafe { open(c"/tmp/meta-test".as_ptr().cast(), 0xC2, 0o666) }; // O_CREAT|O_RDWR|O_EXCL
    assert_eq!(fd, -1, "O_EXCL on existing file should fail");

    // statfs on /tmp
    let mut statfs_buf = [0u8; 256];
    assert_eq!(
        unsafe { statfs(c"/tmp".as_ptr().cast(), statfs_buf.as_mut_ptr()) },
        0
    );
    // f_type at offset 0, size i64 (c_long on 64-bit)
    let f_type = i64::from_le_bytes(statfs_buf[0..8].try_into().unwrap());
    assert_eq!(
        f_type, 0x01021994,
        "f_type should be TMPFS_MAGIC, got {f_type:#x}"
    );

    println!("metadata: OK");
}
