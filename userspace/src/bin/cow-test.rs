unsafe extern "C" {
    fn fork() -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}

fn main() {
    let mut stack_value = 42;
    let pid = unsafe { fork() };
    if pid == 0 {
        // Child: modify the variable (triggers CoW)
        stack_value = 99;
        assert_eq!(stack_value, 99);
        println!("child: value={stack_value}");
    } else if pid > 0 {
        let mut status: i32 = 0;
        unsafe { waitpid(pid, &mut status, 0) };
        // Parent: value must be unchanged (CoW isolation)
        assert_eq!(stack_value, 42);
        println!("parent: value={stack_value}");
    } else {
        println!("fork failed");
    }
}
