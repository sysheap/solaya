//! Standalone bindgen driver for the Solaya headers crate.
//!
//! Produces the same `syscalls.rs`, `syscall_types.rs`, `errno.rs`,
//! `socket_types.rs`, `fs_types.rs`, and `sysinfo_types.rs` outputs as
//! `headers/build.rs`, but parameterised via CLI args instead of relying on
//! nix-provided symlinks in the source tree. Invoked by `cmake/bindgen.cmake`
//! during CMake configure/build.
//!
//! Usage:
//!     bindgen-driver \
//!         --out-dir        <DIR> \
//!         --linux-headers  <DIR>   (contains asm/, asm-generic/, linux/)
//!         --musl-headers   <DIR>   (contains sys/, netinet/, dirent.h, ...)
//!
//! The new cross-toolchain lays linux UAPI and musl headers under the same
//! `<sysroot>/usr/include/` directory, so callers often pass the same path for
//! --linux-headers and --musl-headers. The args are kept separate because the
//! legacy nix flow symlinks them into two distinct dirs (headers/linux_headers
//! and headers/musl_headers).

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

#[derive(Debug, Default)]
struct SocketConstantCallback;

impl ParseCallbacks for SocketConstantCallback {
    fn int_macro(&self, _name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        Some(bindgen::callbacks::IntKind::I32)
    }
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

struct Args {
    out_dir: PathBuf,
    linux_headers: PathBuf,
    musl_headers: PathBuf,
}

fn parse_args() -> Args {
    let mut it = env::args().skip(1);
    let mut out_dir: Option<PathBuf> = None;
    let mut linux: Option<PathBuf> = None;
    let mut musl: Option<PathBuf> = None;

    while let Some(flag) = it.next() {
        let val = it
            .next()
            .unwrap_or_else(|| panic!("missing value for {flag}"));
        match flag.as_str() {
            "--out-dir" => out_dir = Some(PathBuf::from(val)),
            "--linux-headers" => linux = Some(PathBuf::from(val)),
            "--musl-headers" => musl = Some(PathBuf::from(val)),
            other => panic!("unknown flag: {other}"),
        }
    }

    Args {
        out_dir: out_dir.expect("--out-dir required"),
        linux_headers: linux.expect("--linux-headers required"),
        musl_headers: musl.expect("--musl-headers required"),
    }
}

fn default_bindgen_builder(linux_headers: &Path) -> bindgen::Builder {
    bindgen::Builder::default()
        .clang_arg(format!("-I{}", linux_headers.display()))
        .use_core()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();
    std::fs::create_dir_all(&args.out_dir)?;

    generate_syscall_nr_file(&args)?;
    generate_syscall_types(&args)?;
    generate_error_types(&args)?;
    generate_socket_types(&args)?;
    generate_fs_types(&args)?;
    generate_sysinfo_types(&args)?;

    Ok(())
}

fn linux_h(args: &Args, rel: &str) -> String {
    args.linux_headers.join(rel).to_string_lossy().into_owned()
}

fn musl_h(args: &Args, rel: &str) -> String {
    args.musl_headers.join(rel).to_string_lossy().into_owned()
}

fn generate_syscall_types(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let bindings = default_bindgen_builder(&args.linux_headers)
        .header(linux_h(args, "asm-generic/fcntl.h"))
        .header(linux_h(args, "asm-generic/ioctls.h"))
        .header(linux_h(args, "asm-generic/poll.h"))
        .header(linux_h(args, "asm-generic/signal.h"))
        .header(linux_h(args, "asm-generic/termbits.h"))
        .header(linux_h(args, "linux/auxvec.h"))
        .header(linux_h(args, "linux/mman.h"))
        .header(linux_h(args, "linux/sched.h"))
        .header(linux_h(args, "linux/futex.h"))
        .header(linux_h(args, "linux/time.h"))
        .header(linux_h(args, "linux/uio.h"))
        .header(linux_h(args, "linux/wait.h"))
        .header(linux_h(args, "linux/prctl.h"))
        .generate()?;
    bindings.write_to_file(args.out_dir.join("syscall_types.rs"))?;
    Ok(())
}

fn generate_error_types(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let errno_callback = ErrnoCallback::default();
    let _ = default_bindgen_builder(&args.linux_headers)
        .header(linux_h(args, "asm-generic/errno.h"))
        .parse_callbacks(Box::new(errno_callback.clone()))
        .generate()?;

    let errno_path = args.out_dir.join("errno.rs");
    let mut errno_file = File::options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&errno_path)?;

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

fn generate_syscall_nr_file(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let syscall_type_changer = SyscallReaderCallback::default();
    let bindings = default_bindgen_builder(&args.linux_headers)
        .header(linux_h(args, "asm/unistd.h"))
        .parse_callbacks(Box::new(syscall_type_changer.clone()))
        .generate()?;

    let syscall_file_path = args.out_dir.join("syscalls.rs");
    bindings.write_to_file(&syscall_file_path)?;

    let mut syscall_names_file = File::options().append(true).open(&syscall_file_path)?;

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

fn generate_socket_types(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let bindings = bindgen::Builder::default()
        .clang_arg(format!("-I{}", args.musl_headers.display()))
        .header(musl_h(args, "sys/socket.h"))
        .header(musl_h(args, "netinet/in.h"))
        .header(musl_h(args, "netinet/tcp.h"))
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
    bindings.write_to_file(args.out_dir.join("socket_types.rs"))?;
    Ok(())
}

fn generate_fs_types(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let fs_path = args.out_dir.join("fs_types.rs");

    let bindings = default_bindgen_builder(&args.linux_headers)
        .header(linux_h(args, "asm-generic/stat.h"))
        .header(linux_h(args, "asm-generic/statfs.h"))
        .header(linux_h(args, "linux/stat.h"))
        .header(linux_h(args, "linux/fcntl.h"))
        .header(linux_h(args, "linux/fs.h"))
        .parse_callbacks(Box::new(FsConstantCallback))
        .allowlist_type("^stat$")
        .allowlist_type("^statfs$")
        .allowlist_type("statx$")
        .allowlist_type("statx_timestamp")
        .allowlist_var("S_IF.*")
        .allowlist_var("AT_FDCWD")
        .allowlist_var("AT_REMOVEDIR")
        .allowlist_var("AT_EMPTY_PATH")
        .allowlist_var("AT_SYMLINK_NOFOLLOW")
        .allowlist_var("SEEK_SET")
        .allowlist_var("SEEK_CUR")
        .allowlist_var("SEEK_END")
        .derive_copy(true)
        .derive_default(true)
        .generate()?;
    bindings.write_to_file(&fs_path)?;

    let dt_bindings = bindgen::Builder::default()
        .clang_arg(format!("-I{}", args.musl_headers.display()))
        .header(musl_h(args, "dirent.h"))
        .clang_arg("-D_GNU_SOURCE")
        .parse_callbacks(Box::new(DtConstantCallback))
        .use_core()
        .allowlist_var("DT_.*")
        .generate()?;

    let mut fs_file = File::options().append(true).open(&fs_path)?;
    write!(fs_file, "{dt_bindings}")?;

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

fn generate_sysinfo_types(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let bindings = bindgen::Builder::default()
        .clang_arg(format!("-I{}", args.musl_headers.display()))
        .clang_arg("-D_GNU_SOURCE")
        .header(musl_h(args, "sys/utsname.h"))
        .header(musl_h(args, "sys/sysinfo.h"))
        .header(musl_h(args, "sys/resource.h"))
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
    bindings.write_to_file(args.out_dir.join("sysinfo_types.rs"))?;
    Ok(())
}

fn format_file(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    // Invoke rustfmt directly rather than via `cargo fmt --`, since this
    // driver runs outside any Cargo workspace (CMake invokes it with just
    // --out-dir/--linux-headers/--musl-headers).
    Command::new("rustfmt").arg(path).spawn()?.wait()?;
    Ok(())
}
