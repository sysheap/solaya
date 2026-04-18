//! initramfs support: parse a cpio archive and materialize it into the
//! root filesystem at boot. The cpio archive is delivered via QEMU's
//! `-initrd` flag; its physical range is advertised through
//! `/chosen/linux,initrd-{start,end}` in the DTB.

use alloc::collections::BTreeMap;
use core::ops::Range;

use hal::validated_ptr::ValidatedPtr;
use headers::{
    errno::Errno,
    fs::{S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFMT, S_IFREG, S_IFSOCK},
};
use klib::big_endian::BigEndian;

use crate::{
    device_tree, fs,
    fs::vfs::{NodeType, VfsNodeRef},
    info, warn,
};

mod cpio;

/// Physical range of the initrd reported by the bootloader via DTB
/// `/chosen/linux,initrd-{start,end}`. Returns `None` if no initrd is
/// advertised. Must be reserved in the page allocator (the bytes live in
/// free RAM until we copy them into tmpfs).
pub fn find_initrd_range() -> Option<Range<*const u8>> {
    let chosen = device_tree::THE.root_node().find_node("chosen")?;
    let start = chosen
        .get_property("linux,initrd-start")?
        .consume_sized_type::<BigEndian<u64>>()?
        .get();
    let end = chosen
        .get_property("linux,initrd-end")?
        .consume_sized_type::<BigEndian<u64>>()?
        .get();
    if end <= start {
        return None;
    }
    Some((start as *const u8)..(end as *const u8))
}

fn find_initrd() -> Option<&'static [u8]> {
    let range = find_initrd_range()?;
    let start = range.start as usize;
    let end = range.end as usize;
    let len = end.saturating_sub(start);
    if len == 0 {
        return None;
    }
    assert!(
        device_tree::range_in_ram(start..end),
        "initrd range {:#x}..{:#x} from DTB /chosen is not contained in any /memory node — \
         bootloader advertised an initrd outside RAM",
        start,
        end,
    );
    let ptr = ValidatedPtr::<u8>::from_trusted(range.start);
    Some(ptr.as_static_slice(len))
}

pub fn extract() {
    let Some(archive) = find_initrd() else {
        info!("initramfs: no initrd in DTB /chosen; skipping extraction");
        return;
    };
    info!("initramfs: extracting {} bytes from /chosen", archive.len());
    let root = match fs::resolve_path("/") {
        Ok(r) => r,
        Err(e) => {
            warn!("initramfs: no root mounted ({e:?}); skipping extraction");
            return;
        }
    };
    match extract_into(archive, root) {
        Ok(count) => info!("initramfs: extracted {count} entries into /"),
        Err(e) => warn!("initramfs: extraction failed: {e:?}"),
    }
}

#[derive(Debug)]
#[allow(dead_code)] // variants read via Debug in warn! only
enum ExtractError {
    Cpio(cpio::CpioError),
    Vfs(Errno),
    InvalidSymlinkTarget,
}

fn extract_into(archive: &[u8], root: VfsNodeRef) -> Result<usize, ExtractError> {
    let mut by_ino: BTreeMap<u32, VfsNodeRef> = BTreeMap::new();
    let mut count = 0usize;

    for entry in cpio::iter(archive) {
        let entry = entry.map_err(ExtractError::Cpio)?;
        let path = normalize(entry.name);
        if path.is_empty() {
            continue;
        }

        let file_type = entry.mode & S_IFMT;
        if matches!(file_type, S_IFBLK | S_IFCHR | S_IFIFO | S_IFSOCK) {
            continue;
        }

        // Hardlink: subsequent references to the same inode carry no data
        // (nlink > 1, empty data); reuse the Arc we stored on first sight.
        if entry.nlink > 1
            && file_type == S_IFREG
            && entry.data.is_empty()
            && let Some(existing) = by_ino.get(&entry.ino).cloned()
        {
            let (parent_path, name) = split_parent(path);
            let parent = ensure_dir(root.clone(), parent_path)?;
            parent
                .link(name, existing.clone())
                .map_err(ExtractError::Vfs)?;
            existing.inc_nlink();
            count += 1;
            continue;
        }

        let (parent_path, name) = split_parent(path);
        let parent = ensure_dir(root.clone(), parent_path)?;
        let node = match file_type {
            S_IFDIR => match parent.lookup(name) {
                Ok(n) if n.node_type() == NodeType::Directory => n,
                _ => parent
                    .create(name, NodeType::Directory)
                    .map_err(ExtractError::Vfs)?,
            },
            S_IFLNK => {
                let target = core::str::from_utf8(entry.data)
                    .map_err(|_| ExtractError::InvalidSymlinkTarget)?;
                parent
                    .create_symlink(name, target)
                    .map_err(ExtractError::Vfs)?
            }
            S_IFREG => {
                let file = parent
                    .create(name, NodeType::File)
                    .map_err(ExtractError::Vfs)?;
                if !entry.data.is_empty() {
                    file.write(0, entry.data).map_err(ExtractError::Vfs)?;
                }
                file
            }
            _ => continue,
        };

        if let Err(e) = node.set_mode(entry.mode) {
            warn!(
                "initramfs: set_mode(0o{:o}) failed for {}: {:?}",
                entry.mode, path, e
            );
        }

        if entry.nlink > 1 && file_type == S_IFREG {
            by_ino.insert(entry.ino, node);
        }
        count += 1;
    }

    Ok(count)
}

fn normalize(name: &str) -> &str {
    let s = name.trim_start_matches('/');
    let s = s.strip_prefix("./").unwrap_or(s);
    if s == "." { "" } else { s }
}

fn split_parent(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

fn ensure_dir(root: VfsNodeRef, path: &str) -> Result<VfsNodeRef, ExtractError> {
    if path.is_empty() {
        return Ok(root);
    }
    let mut cur = root;
    for component in path.split('/') {
        if component.is_empty() {
            continue;
        }
        let next = match cur.lookup(component) {
            Ok(n) => n,
            Err(_) => cur
                .create(component, NodeType::Directory)
                .map_err(ExtractError::Vfs)?,
        };
        cur = next;
    }
    Ok(cur)
}
