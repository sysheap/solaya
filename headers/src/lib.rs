#![no_std]
#![allow(non_camel_case_types)]

pub mod syscalls {
    include!(concat!(env!("OUT_DIR"), "/syscalls.rs"));
}

pub mod syscall_types {
    include!(concat!(env!("OUT_DIR"), "/syscall_types.rs"));
}

pub mod errno {
    include!(concat!(env!("OUT_DIR"), "/errno.rs"));
}

pub mod socket {
    include!(concat!(env!("OUT_DIR"), "/socket_types.rs"));
}

pub mod fs {
    include!(concat!(env!("OUT_DIR"), "/fs_types.rs"));
}

pub mod sysinfo_types {
    include!(concat!(env!("OUT_DIR"), "/sysinfo_types.rs"));
}
