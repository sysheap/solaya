pub mod devfs;
#[cfg(all(target_arch = "riscv64", feature = "ext2"))]
pub mod ext2;
pub mod open_file;
mod procfs;
mod tmpfs;
pub mod vfs;

pub use open_file::VfsOpenFile;
pub use vfs::{resolve_parent, resolve_path, resolve_relative, stat_from_node, statx_from_node};

pub fn init() {
    vfs::mount("/", vfs::RootDir::new());
    vfs::mount("/tmp", tmpfs::TmpfsDir::new());
    vfs::mount("/proc", procfs::new());
    vfs::mount("/dev", devfs::new());
}
