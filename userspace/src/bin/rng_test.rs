use std::io::Read;

use headers::syscall_types::AT_RANDOM;

unsafe extern "C" {
    fn getauxval(typ: u64) -> usize;
}

fn main() {
    // Test 1: Read from /dev/random
    {
        let mut f = std::fs::File::open("/dev/random").expect("open /dev/random failed");
        let mut buf = [0u8; 32];
        let n = f.read(&mut buf).expect("read from /dev/random failed");
        assert_eq!(n, 32, "/dev/random should return 32 bytes");
        assert!(
            buf.iter().any(|&b| b != 0),
            "/dev/random should return non-zero bytes"
        );
        print!("random bytes:");
        for b in &buf {
            print!(" {:02x}", b);
        }
        println!();
    }
    println!("OK dev_random");

    // Test 2: AT_RANDOM from auxiliary vector
    {
        let ptr = unsafe { getauxval(AT_RANDOM.into()) };
        assert!(ptr != 0, "AT_RANDOM should be non-zero");
        let random = unsafe { core::slice::from_raw_parts(ptr as *const u8, 16) };
        assert!(
            random.iter().any(|&b| b != 0),
            "AT_RANDOM should contain non-zero bytes"
        );
        print!("at_random bytes:");
        for b in random {
            print!(" {:02x}", b);
        }
        println!();
    }
    println!("OK at_random");
}
