pub mod devfs;
pub mod ext2;
pub mod open_file;
mod procfs;
pub(crate) mod tmpfs;
pub mod vfs;

pub use open_file::VfsOpenFile;
pub use vfs::{
    resolve_parent, resolve_path, resolve_path_nofollow, resolve_relative, stat_from_node,
    statx_from_node,
};

pub fn init() {
    vfs::mount("/", vfs::RootDir::new());
    vfs::mount("/tmp", tmpfs::TmpfsDir::new());
    vfs::mount("/proc", procfs::new());
    vfs::mount("/dev", devfs::new());
}
