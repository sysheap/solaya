use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};
use driver_api::BlockDevice;
use headers::errno::Errno;

use hal::spinlock::Spinlock;

static NEXT_INO: AtomicU64 = AtomicU64::new(1);

pub fn alloc_ino() -> u64 {
    NEXT_INO.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
    Symlink,
}

impl NodeType {
    pub fn stat_mode(self) -> u32 {
        match self {
            NodeType::File => headers::fs::S_IFREG | 0o644,
            NodeType::Directory => headers::fs::S_IFDIR | 0o755,
            NodeType::Symlink => headers::fs::S_IFLNK | 0o777,
        }
    }
}

pub fn stat_from_node(node: &VfsNodeRef) -> headers::fs::stat {
    let (atime_sec, atime_nsec) = node.atime();
    let (mtime_sec, mtime_nsec) = node.mtime();
    let (ctime_sec, ctime_nsec) = node.ctime();
    headers::fs::stat {
        st_ino: node.ino(),
        st_mode: node.mode(),
        st_nlink: node.nlink(),
        st_uid: node.uid(),
        st_gid: node.gid(),
        st_size: node.size() as i64,
        st_blksize: 4096,
        st_atime: atime_sec,
        st_atime_nsec: atime_nsec as u64,
        st_mtime: mtime_sec,
        st_mtime_nsec: mtime_nsec as u64,
        st_ctime: ctime_sec,
        st_ctime_nsec: ctime_nsec as u64,
        ..headers::fs::stat::default()
    }
}

pub fn statx_from_node(node: &VfsNodeRef) -> headers::fs::statx {
    let (atime_sec, atime_nsec) = node.atime();
    let (mtime_sec, mtime_nsec) = node.mtime();
    let (ctime_sec, ctime_nsec) = node.ctime();
    headers::fs::statx {
        stx_mask: 0x7ff,
        stx_blksize: 4096,
        stx_nlink: node.nlink(),
        stx_uid: node.uid(),
        stx_gid: node.gid(),
        stx_mode: node.mode() as u16,
        stx_ino: node.ino(),
        stx_size: node.size() as u64,
        stx_atime: headers::fs::statx_timestamp {
            tv_sec: atime_sec,
            tv_nsec: atime_nsec,
            __reserved: 0,
        },
        stx_mtime: headers::fs::statx_timestamp {
            tv_sec: mtime_sec,
            tv_nsec: mtime_nsec,
            __reserved: 0,
        },
        stx_ctime: headers::fs::statx_timestamp {
            tv_sec: ctime_sec,
            tv_nsec: ctime_nsec,
            __reserved: 0,
        },
        ..headers::fs::statx::default()
    }
}

#[derive(Clone)]
pub struct DirEntry {
    pub name: String,
    pub ino: u64,
    pub node_type: NodeType,
}

pub type VfsNodeRef = Arc<dyn VfsNode>;

pub trait VfsNode: Send + Sync {
    fn node_type(&self) -> NodeType;
    fn ino(&self) -> u64;
    fn size(&self) -> usize;

    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, Errno> {
        Err(Errno::EISDIR)
    }

    fn write(&self, _offset: usize, _data: &[u8]) -> Result<usize, Errno> {
        Err(Errno::EISDIR)
    }

    fn truncate(&self, _length: usize) -> Result<(), Errno> {
        Err(Errno::EISDIR)
    }

    fn mode(&self) -> u32 {
        self.node_type().stat_mode()
    }

    fn uid(&self) -> u32 {
        0
    }

    fn gid(&self) -> u32 {
        0
    }

    fn nlink(&self) -> u32 {
        1
    }

    fn set_mode(&self, _mode: u32) -> Result<(), Errno> {
        Err(Errno::EPERM)
    }

    fn set_owner(&self, _uid: u32, _gid: u32) -> Result<(), Errno> {
        Err(Errno::EPERM)
    }

    fn lookup(&self, _name: &str) -> Result<VfsNodeRef, Errno> {
        Err(Errno::ENOTDIR)
    }

    fn create(&self, _name: &str, _node_type: NodeType) -> Result<VfsNodeRef, Errno> {
        Err(Errno::ENOTDIR)
    }

    fn create_symlink(&self, _name: &str, _target: &str) -> Result<VfsNodeRef, Errno> {
        Err(Errno::ENOTDIR)
    }

    fn unlink(&self, _name: &str) -> Result<(), Errno> {
        Err(Errno::ENOTDIR)
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, Errno> {
        Err(Errno::ENOTDIR)
    }

    fn readlink(&self) -> Result<String, Errno> {
        Err(Errno::EINVAL)
    }

    fn link(&self, _name: &str, _node: VfsNodeRef) -> Result<(), Errno> {
        Err(Errno::ENOTDIR)
    }

    fn remove_child(&self, _name: &str) -> Result<VfsNodeRef, Errno> {
        Err(Errno::ENOTDIR)
    }

    fn inc_nlink(&self) {}
    #[allow(dead_code)]
    fn dec_nlink(&self) {}

    /// If this node is backed by a block device, return an `Arc<dyn BlockDevice>`
    /// so callers can bypass in-memory caching and do direct I/O.
    fn block_device(&self) -> Option<Arc<dyn BlockDevice>> {
        None
    }

    fn atime(&self) -> (i64, u32) {
        (0, 0)
    }

    fn mtime(&self) -> (i64, u32) {
        (0, 0)
    }

    fn ctime(&self) -> (i64, u32) {
        (0, 0)
    }
}

static MOUNT_TABLE: Spinlock<BTreeMap<String, VfsNodeRef>> = Spinlock::new(BTreeMap::new());

pub fn mount(path: &str, root: VfsNodeRef) {
    MOUNT_TABLE.lock().insert(path.to_string(), root);
}

pub fn resolve_path(path: &str) -> Result<VfsNodeRef, Errno> {
    resolve_path_with_depth(path, 0)
}

fn resolve_path_with_depth(path: &str, depth: u32) -> Result<VfsNodeRef, Errno> {
    if path == "." || path == "/" {
        let table = MOUNT_TABLE.lock();
        let (_, node) = find_mount(&table, "/")?;
        return Ok(node);
    }
    let absolute = if path.starts_with('/') {
        canonicalize_path(path)
    } else {
        canonicalize_path(&alloc::format!("/{path}"))
    };
    if absolute == "/" {
        let table = MOUNT_TABLE.lock();
        let (_, node) = find_mount(&table, "/")?;
        return Ok(node);
    }
    let table = MOUNT_TABLE.lock();
    let (mount_path, node) = find_mount(&table, &absolute)?;
    let remainder = &absolute[mount_path.len()..];
    let base_abs = String::from(mount_path);
    drop(table);
    walk_with_depth(node, base_abs, remainder, depth)
}

/// Resolve `.` and `..` at the string level.  An absolute path in,
/// absolute path out; `..` above `/` stays at `/`.
pub fn canonicalize_path(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            _ => stack.push(component),
        }
    }
    if stack.is_empty() {
        String::from("/")
    } else {
        let mut out = String::new();
        for c in &stack {
            out.push('/');
            out.push_str(c);
        }
        out
    }
}

pub fn resolve_path_nofollow(path: &str) -> Result<VfsNodeRef, Errno> {
    if path == "." || path == "/" || path == ".." {
        return resolve_path(path);
    }
    if !path.starts_with('/') {
        let abs = alloc::format!("/{path}");
        return resolve_path_nofollow(&abs);
    }
    let (parent, name) = resolve_parent(path)?;
    if name == "." || name == ".." {
        return resolve_path(path);
    }
    parent.lookup(name)
}

pub fn resolve_parent(path: &str) -> Result<(VfsNodeRef, &str), Errno> {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return Err(Errno::EINVAL);
    }
    let last_slash = path.rfind('/').ok_or(Errno::EINVAL)?;
    let parent_path = if last_slash == 0 {
        "/"
    } else {
        &path[..last_slash]
    };
    let name = &path[last_slash + 1..];
    if name.is_empty() {
        return Err(Errno::EINVAL);
    }
    let parent = resolve_path(parent_path)?;
    Ok((parent, name))
}

fn find_mount<'a>(
    table: &'a BTreeMap<String, VfsNodeRef>,
    path: &str,
) -> Result<(&'a str, VfsNodeRef), Errno> {
    let mut best: Option<(&str, &VfsNodeRef)> = None;
    for (mount_path, node) in table.iter() {
        let matches = path == mount_path
            || (mount_path == "/" && path.starts_with('/'))
            || (path.starts_with(mount_path.as_str())
                && path.as_bytes().get(mount_path.len()) == Some(&b'/'));
        if matches
            && (best.is_none() || mount_path.len() > best.as_ref().map(|b| b.0.len()).unwrap_or(0))
        {
            best = Some((mount_path.as_str(), node));
        }
    }
    let (mp, node) = best.ok_or(Errno::ENOENT)?;
    Ok((mp, node.clone()))
}

/// Walk `path` relative to `base`, whose absolute path (in
/// canonicalized form, e.g. `/foo/bar`) is `base_abs`. The caller
/// MUST supply the real absolute path for `base` — otherwise `..` and
/// relative symlinks will resolve from `base_abs` instead of the
/// actual dirfd location.
pub fn resolve_relative(base: VfsNodeRef, base_abs: &str, path: &str) -> Result<VfsNodeRef, Errno> {
    walk_with_depth(base, String::from(base_abs), path, 0)
}

const MAX_SYMLINK_DEPTH: u32 = 8;

/// Walk `path` relative to `node`, whose absolute path is `base_abs`.
/// `base_abs` is updated as we descend so relative symlinks (including
/// ones that contain `..`) can be rewritten to absolute and re-resolved
/// via [`resolve_path_with_depth`] without losing track of where we are.
fn walk_with_depth(
    mut node: VfsNodeRef,
    mut base_abs: String,
    path: &str,
    mut depth: u32,
) -> Result<VfsNodeRef, Errno> {
    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                // Walk up: pop the last segment from base_abs + re-resolve.
                base_abs = pop_segment(&base_abs);
                node = resolve_path_with_depth(&base_abs, depth)?;
                continue;
            }
            _ => {}
        }
        let parent = node.clone();
        node = parent.lookup(component)?;
        if base_abs.ends_with('/') {
            base_abs.push_str(component);
        } else {
            base_abs.push('/');
            base_abs.push_str(component);
        }
        if node.node_type() == NodeType::Symlink {
            if depth >= MAX_SYMLINK_DEPTH {
                return Err(Errno::ELOOP);
            }
            depth += 1;
            let target = node.readlink()?;
            let abs_target = if target.starts_with('/') {
                canonicalize_path(&target)
            } else {
                // target is relative to the symlink's parent dir.
                let parent_abs = pop_segment(&base_abs);
                canonicalize_path(&alloc::format!("{parent_abs}/{target}"))
            };
            node = resolve_path_with_depth(&abs_target, depth)?;
            base_abs = abs_target;
        }
    }
    Ok(node)
}

fn pop_segment(path: &str) -> String {
    if path == "/" {
        return String::from("/");
    }
    match path.rfind('/') {
        Some(0) => String::from("/"),
        Some(pos) => String::from(&path[..pos]),
        None => String::from("/"),
    }
}

pub(super) struct StaticDir {
    ino: u64,
    entries: BTreeMap<String, VfsNodeRef>,
}

impl StaticDir {
    pub fn new(entries: Vec<(&str, VfsNodeRef)>) -> Arc<Self> {
        Arc::new(Self {
            ino: alloc_ino(),
            entries: entries
                .into_iter()
                .map(|(n, v)| (String::from(n), v))
                .collect(),
        })
    }
}

impl VfsNode for StaticDir {
    fn node_type(&self) -> NodeType {
        NodeType::Directory
    }

    fn ino(&self) -> u64 {
        self.ino
    }

    fn size(&self) -> usize {
        0
    }

    fn lookup(&self, name: &str) -> Result<VfsNodeRef, Errno> {
        self.entries.get(name).cloned().ok_or(Errno::ENOENT)
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, Errno> {
        Ok(self
            .entries
            .iter()
            .map(|(name, node)| DirEntry {
                name: name.clone(),
                ino: node.ino(),
                node_type: node.node_type(),
            })
            .collect())
    }
}
