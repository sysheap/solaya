unsafe extern "C" {
    fn getrlimit(resource: i32, rlim: *mut headers::sysinfo_types::rlimit) -> i32;
}

fn main() {
    let mut rlim = headers::sysinfo_types::rlimit::default();

    let ret = unsafe { getrlimit(headers::sysinfo_types::RLIMIT_NOFILE as i32, &mut rlim) };
    assert_eq!(ret, 0);
    assert_eq!(rlim.rlim_cur, 1024, "soft limit should be 1024");
    assert_eq!(rlim.rlim_max, 4096, "hard limit should be 4096");

    let ret = unsafe { getrlimit(headers::sysinfo_types::RLIMIT_STACK as i32, &mut rlim) };
    assert_eq!(ret, 0);
    assert_eq!(
        rlim.rlim_cur,
        8 * 1024 * 1024,
        "stack soft limit should be 8MiB"
    );
    assert_eq!(
        rlim.rlim_max,
        u64::MAX,
        "stack hard limit should be unlimited"
    );

    let ret = unsafe { getrlimit(headers::sysinfo_types::RLIMIT_CORE as i32, &mut rlim) };
    assert_eq!(ret, 0);
    assert_eq!(rlim.rlim_cur, 0, "core soft limit should be 0");
    assert_eq!(rlim.rlim_max, 0, "core hard limit should be 0");

    println!("rlimit: OK");
}
