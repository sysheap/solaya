// Direct regression test for prctl(PR_GET_AUXV). Kept independent of libc
// auxv consumers (rustix, getauxval, …) so a failure here points at the
// kernel path rather than at whichever library happens to wrap it.

unsafe extern "C" {
    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
    fn strlen(s: *const u8) -> usize;
}

const PR_GET_AUXV: i32 = 0x41555856;
const AT_NULL: usize = 0;
const AT_PHDR: usize = 3;
const AT_PAGESZ: usize = 6;
const AT_RANDOM: usize = 25;
const AT_EXECFN: usize = 31;

fn get_auxv(buf: &mut [u64]) -> i32 {
    let ptr = buf.as_mut_ptr() as u64;
    let len = core::mem::size_of_val(buf) as u64;
    unsafe { prctl(PR_GET_AUXV, ptr, len, 0, 0) }
}

fn main() {
    // Large enough for the current Solaya auxv (six entries → 112 bytes) plus
    // headroom if new tags are added.
    let mut buf = [0u64; 64];
    let size = get_auxv(&mut buf);
    assert!(size > 0, "PR_GET_AUXV returned {size}");
    let size = size as usize;
    assert!(
        size <= core::mem::size_of_val(&buf),
        "auxv size {size} exceeds buffer"
    );
    assert!(
        size.is_multiple_of(16),
        "auxv size {size} not a tag/value multiple"
    );

    let pairs = size / 16;
    let mut saw_null = false;
    let mut pagesz = 0u64;
    let mut phdr = 0u64;
    let mut random = 0u64;
    let mut execfn = 0u64;
    for i in 0..pairs {
        let tag = buf[i * 2] as usize;
        let val = buf[i * 2 + 1];
        match tag {
            AT_PAGESZ => pagesz = val,
            AT_PHDR => phdr = val,
            AT_RANDOM => random = val,
            AT_EXECFN => execfn = val,
            AT_NULL => {
                assert_eq!(val, 0, "AT_NULL must carry value 0");
                saw_null = true;
                assert_eq!(
                    i + 1,
                    pairs,
                    "AT_NULL must be the last pair (saw it at {i}/{pairs})"
                );
            }
            _ => {}
        }
    }
    assert!(saw_null, "auxv must be AT_NULL-terminated");
    assert_eq!(pagesz, 4096, "AT_PAGESZ should be 4096, got {pagesz}");
    assert_ne!(phdr, 0, "AT_PHDR should be a non-null program-header VA");
    assert_ne!(random, 0, "AT_RANDOM should point at the 16 random bytes");

    // AT_EXECFN must round-trip to the program name pushed onto the stack.
    assert_ne!(execfn, 0, "AT_EXECFN should be a non-null string pointer");
    let name_len = unsafe { strlen(execfn as *const u8) };
    let name = unsafe { core::slice::from_raw_parts(execfn as *const u8, name_len) };
    assert_eq!(name, b"auxv-test", "AT_EXECFN resolves to {name:?}");

    // Undersized buffer: the kernel must still report the full auxv size so
    // callers (e.g. rustix's pr_get_auxv_dynamic) can retry with a larger
    // allocation. rustix relies on this contract.
    let mut tiny = [0u64; 2];
    let full_size = get_auxv(&mut tiny);
    assert_eq!(
        full_size as usize, size,
        "PR_GET_AUXV with 16-byte buffer should return full auxv size {size}"
    );

    println!("auxv: OK");
}
