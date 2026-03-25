use core::ptr::{read_volatile, write_volatile};

unsafe extern "C" {
    fn fork() -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}

fn main() {
    // Use Box to guarantee the value lives on the heap (a CoW page).
    // Use volatile reads/writes to prevent the compiler from keeping
    // the value in a register and optimizing away the memory access.
    let mut value = Box::new(42i32);
    let pid = unsafe { fork() };
    if pid == 0 {
        unsafe { write_volatile(&mut *value, 99) };
        let v = unsafe { read_volatile(&*value) };
        assert_eq!(v, 99);
        println!("child: value={v}");
    } else if pid > 0 {
        let mut status: i32 = 0;
        unsafe { waitpid(pid, &mut status, 0) };
        let v = unsafe { read_volatile(&*value) };
        assert_eq!(v, 42);
        println!("parent: value={v}");
    } else {
        println!("fork failed");
    }
}
