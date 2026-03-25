use std::{
    env,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

use bindgen::callbacks::ParseCallbacks;

const SYSCALL_PREFIX: &str = "__NR_";

#[derive(Debug, Default, Clone)]
struct SyscallReaderCallback {
    syscalls: Arc<Mutex<Vec<String>>>,
}

impl ParseCallbacks for SyscallReaderCallback {
    fn int_macro(&self, name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        if name.starts_with(SYSCALL_PREFIX) {
            let mut lg = self.syscalls.lock().unwrap();
            lg.push(name.replace(SYSCALL_PREFIX, ""));
            return Some(bindgen::callbacks::IntKind::Custom {
                name: "usize",
                is_signed: false,
            });
        }
        None
    }

    fn item_name(&self, item_info: bindgen::callbacks::ItemInfo) -> Option<String> {
        if item_info.name.starts_with(SYSCALL_PREFIX) {
            return Some(format!(
                "SYSCALL_NR_{}",
                item_info.name.replace(SYSCALL_PREFIX, "").to_uppercase()
            ));
        }
        None
    }
}

#[derive(Debug, Clone, Default)]
struct ErrnoCallback {
    errnos: Arc<Mutex<Vec<(String, isize)>>>,
}

impl ParseCallbacks for ErrnoCallback {
    fn int_macro(&self, name: &str, value: i64) -> Option<bindgen::callbacks::IntKind> {
        // Ignore duplicate definitions
        if ["EWOULDBLOCK", "EDEADLOCK"].contains(&name) {
            return None;
        }
        self.errnos
            .lock()
            .unwrap()
            .push((name.into(), value as isize));
        None
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_path = PathBuf::from(env::var("OUT_DIR")?);
    generate_syscall_nr_file(&out_path)?;
    generate_syscall_types(&out_path)?;
    generate_error_types(&out_path)?;
    generate_socket_types(&out_path)?;
    generate_fs_types(&out_path)?;
    generate_sysinfo_types(&out_path)?;
    Ok(())
}

fn default_bindgen_builder() -> bindgen::Builder {
    bindgen::Builder::default()
        .clang_arg("-Ilinux_headers/include")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .use_core()
}

fn generate_syscall_types(out_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let bindings = default_bindgen_builder()
        .header("linux_headers/include/asm-generic/fcntl.h")
        .header("linux_headers/include/asm-generic/ioctls.h")
        .header("linux_headers/include/asm-generic/poll.h")
        .header("linux_headers/include/asm-generic/signal.h")
        .header("linux_headers/include/asm-generic/termbits.h")
        .header("linux_headers/include/linux/auxvec.h")
        .header("linux_headers/include/linux/mman.h")
        .header("linux_headers/include/linux/sched.h")
        .header("linux_headers/include/linux/futex.h")
        .header("linux_headers/include/linux/time.h")
        .header("linux_headers/include/linux/uio.h")
        .header("linux_headers/include/linux/wait.h")
        .generate()?;
    let syscall_file_path = out_path.join("syscall_types.rs");
    bindings.write_to_file(syscall_file_path.clone())?;
    Ok(())
}

fn generate_error_types(out_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let errno_callback = ErrnoCallback::default();
    let _ = default_bindgen_builder()
        .header("linux_headers/include/asm-generic/errno.h")
        .parse_callbacks(Box::new(errno_callback.clone()))
        .generate()?;
    let errno_path = out_path.join("errno.rs");
    let mut errno_file = File::options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(errno_path.clone())?;

    writeln!(errno_file, "#[repr(isize)]")?;
    writeln!(errno_file, "#[derive(Debug, PartialEq, Eq, Copy, Clone)]")?;
    writeln!(errno_file, "pub enum Errno {{")?;

    for (error, value) in errno_callback.errnos.lock().unwrap().iter() {
        writeln!(errno_file, "{error} = {value},")?;
    }

    writeln!(errno_file, "}}")?;

    writeln!(
        errno_file,
        "            
impl From<core::num::TryFromIntError> for Errno {{
    fn from(_value: core::num::TryFromIntError) -> Self {{
        Errno::EINVAL
    }}
}}"
    )?;

    drop(errno_file);
    format_file(errno_path)?;

    Ok(())
}

fn generate_syscall_nr_file(out_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let syscall_type_changer = SyscallReaderCallback::default();
    let bindings = default_bindgen_builder()
        .header("linux_headers/include/asm/unistd.h")
        .parse_callbacks(Box::new(syscall_type_changer.clone()))
        .generate()?;

    let syscall_file_path = out_path.join("syscalls.rs");
    bindings.write_to_file(syscall_file_path.clone())?;

    let mut syscall_names_file = File::options()
        .append(true)
        .open(syscall_file_path.clone())?;

    let lg = syscall_type_changer.syscalls.lock().unwrap();

    writeln!(
        syscall_names_file,
        "pub const SYSCALL_NAMES: [(usize, &str); {}] = [",
        lg.len()
    )?;
    for name in lg.iter() {
        writeln!(
            syscall_names_file,
            "(SYSCALL_NR_{}, \"{name}\"),",
            name.to_uppercase()
        )?;
    }
    writeln!(syscall_names_file, "];")?;

    drop(syscall_names_file);

    format_file(syscall_file_path)?;
    Ok(())
}

#[derive(Debug, Default)]
struct SocketConstantCallback;

impl ParseCallbacks for SocketConstantCallback {
    fn int_macro(&self, _name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        Some(bindgen::callbacks::IntKind::I32)
    }
}

fn generate_socket_types(out_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let bindings = bindgen::Builder::default()
        .clang_arg("-Imusl_headers")
        .header("musl_headers/sys/socket.h")
        .header("musl_headers/netinet/in.h")
        .header("musl_headers/netinet/tcp.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .parse_callbacks(Box::new(SocketConstantCallback))
        .use_core()
        .allowlist_var("AF_INET")
        .allowlist_var("SOCK_DGRAM")
        .allowlist_var("SOCK_STREAM")
        .allowlist_var("SOCK_CLOEXEC")
        .allowlist_var("IPPROTO_TCP")
        .allowlist_var("IPPROTO_UDP")
        .allowlist_var("TH_FIN")
        .allowlist_var("TH_SYN")
        .allowlist_var("TH_RST")
        .allowlist_var("TH_PUSH")
        .allowlist_var("TH_ACK")
        .allowlist_var("TH_URG")
        .allowlist_type("sockaddr_in")
        .allowlist_type("in_addr")
        .derive_copy(true)
        .generate()?;
    let socket_path = out_path.join("socket_types.rs");
    bindings.write_to_file(socket_path)?;
    Ok(())
}

#[derive(Debug, Default)]
struct FsConstantCallback;

impl ParseCallbacks for FsConstantCallback {
    fn int_macro(&self, name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        if name.starts_with("S_I") {
            Some(bindgen::callbacks::IntKind::U32)
        } else {
            Some(bindgen::callbacks::IntKind::I32)
        }
    }
}

#[derive(Debug, Default)]
struct DtConstantCallback;

impl ParseCallbacks for DtConstantCallback {
    fn int_macro(&self, _name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        Some(bindgen::callbacks::IntKind::U8)
    }
}

fn generate_fs_types(out_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let fs_path = out_path.join("fs_types.rs");

    // Invocation 1: Linux kernel headers for stat, statx, S_IF*, AT_*, SEEK_*
    let bindings = default_bindgen_builder()
        .header("linux_headers/include/asm-generic/stat.h")
        .header("linux_headers/include/linux/stat.h")
        .header("linux_headers/include/linux/fcntl.h")
        .header("linux_headers/include/linux/fs.h")
        .parse_callbacks(Box::new(FsConstantCallback))
        .allowlist_type("^stat$")
        .allowlist_type("statx$")
        .allowlist_type("statx_timestamp")
        .allowlist_var("S_IF.*")
        .allowlist_var("AT_FDCWD")
        .allowlist_var("AT_REMOVEDIR")
        .allowlist_var("AT_EMPTY_PATH")
        .allowlist_var("SEEK_SET")
        .allowlist_var("SEEK_CUR")
        .allowlist_var("SEEK_END")
        .derive_copy(true)
        .derive_default(true)
        .generate()?;
    bindings.write_to_file(fs_path.clone())?;

    // Invocation 2: musl headers for DT_* constants
    let dt_bindings = bindgen::Builder::default()
        .clang_arg("-Imusl_headers")
        .header("musl_headers/dirent.h")
        .clang_arg("-D_GNU_SOURCE")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .parse_callbacks(Box::new(DtConstantCallback))
        .use_core()
        .allowlist_var("DT_.*")
        .generate()?;

    // Append DT_* to the same file
    let mut fs_file = File::options().append(true).open(fs_path.clone())?;
    write!(fs_file, "{dt_bindings}")?;

    // Manually append linux_dirent64 (kernel-internal, not in any UAPI header)
    writeln!(fs_file)?;
    writeln!(fs_file, "#[repr(C)]")?;
    writeln!(fs_file, "pub struct linux_dirent64 {{")?;
    writeln!(fs_file, "    pub d_ino: u64,")?;
    writeln!(fs_file, "    pub d_off: i64,")?;
    writeln!(fs_file, "    pub d_reclen: u16,")?;
    writeln!(fs_file, "    pub d_type: u8,")?;
    writeln!(fs_file, "}}")?;

    drop(fs_file);
    format_file(fs_path)?;

    Ok(())
}

fn generate_sysinfo_types(out_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let bindings = bindgen::Builder::default()
        .clang_arg("-Imusl_headers")
        .clang_arg("-D_GNU_SOURCE")
        .header("musl_headers/sys/utsname.h")
        .header("musl_headers/sys/sysinfo.h")
        .header("musl_headers/sys/resource.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .use_core()
        .allowlist_type("utsname")
        .allowlist_type("sysinfo")
        .allowlist_type("rusage")
        .allowlist_type("rlimit")
        .allowlist_var("RLIMIT_.*")
        .allowlist_var("RLIM_INFINITY")
        .derive_copy(true)
        .derive_default(true)
        .generate()?;
    let sysinfo_path = out_path.join("sysinfo_types.rs");
    bindings.write_to_file(sysinfo_path)?;
    Ok(())
}

fn format_file(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    Command::new("cargo")
        .arg("fmt")
        .arg("--")
        .arg(path)
        .spawn()?
        .wait()?;
    Ok(())
}
