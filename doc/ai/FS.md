# Filesystem (VFS)

## Overview

The VFS layer lives in `kernel/src/fs/`. All filesystems implement the `VfsNode` trait and are mounted into a global mount table. Filesystems are in-memory by default (tmpfs, procfs, devfs); ext2 is the one exception and sits on a block device.

## Files

| File | Purpose |
|------|---------|
| `vfs.rs` | `VfsNode` trait, mount table, path resolution (including `..` and relative symlinks), `StaticDir` |
| `tmpfs.rs` | Read-write in-memory filesystem (`TmpfsDir`, `TmpfsFile`, `TmpfsSymlink`) |
| `procfs.rs` | `/proc` — `ProcVersionFile`, builder via `StaticDir` |
| `devfs.rs` | `/dev` — `DevNull`, `DevZero`, builder via `StaticDir` |
| `open_file.rs` | `VfsOpenFile` — per-fd state (offset, flags), read/write/seek |
| `mod.rs` | `init()` — mounts `/`, `/tmp`, `/proc`, `/dev` |

## Mount Layout

```
/         TmpfsDir (populated at boot by initramfs::extract from the
                    buildroot cpio — /bin, /sbin, /etc, /usr/...)
/tmp      TmpfsDir (read-write, supports create/unlink)
/proc     StaticDir { "version" -> ProcVersionFile }
/dev      StaticDir { "null" -> DevNull, "zero" -> DevZero }
```

## VfsNode Trait

Every node implements `node_type()`, `ino()`, and `size()`. Other methods have default implementations that return appropriate errors:

- **Files** override: `read`, `write`, `truncate`
- **Directories** override: `lookup`, `readdir`, and optionally `create`/`unlink` (only tmpfs supports mutation)

Default errors: files return `ENOTDIR` for directory ops, directories return `EISDIR` for file ops.

## Key Types

- `VfsNodeRef` = `Arc<dyn VfsNode>` — shared reference-counted node
- `VfsOpenFile` = `Arc<Spinlock<VfsOpenFileInner>>` — per-fd open file with offset tracking
- `DirEntry` — returned by `readdir()`, contains name, ino, node_type
- `NodeType` — enum: `File` or `Directory`

## Inode Allocation

`alloc_ino()` in `vfs.rs` uses a global `AtomicU64` counter. Every node gets a unique inode at construction time.

## Path Resolution

`resolve_path(path)` finds the longest-matching mount point, then walks remaining components via `lookup()`. `resolve_parent(path)` splits off the last component and resolves the parent directory. `resolve_relative(base, path)` walks from an existing node (used for dirfd-relative operations like `openat`).

## StaticDir

A reusable read-only directory for filesystems with fixed children (devfs, procfs). Constructed via `StaticDir::new(vec![("name", node), ...])`. Implements `lookup` and `readdir` over a `BTreeMap`.

## Adding a New Filesystem Entry

### New device in `/dev`
Add entry to the vec in `devfs::new()`:
```rust
pub(super) fn new() -> Arc<StaticDir> {
    StaticDir::new(vec![
        ("null", Arc::new(DevNull { ino: alloc_ino() })),
        ("zero", Arc::new(DevZero { ino: alloc_ino() })),
        ("mydev", Arc::new(MyDev { ino: alloc_ino() })),  // new
    ])
}
```

### New file in `/proc`
Same pattern in `procfs::new()`.

### New mount point
Add `vfs::mount("/path", node)` in `fs::init()`.

## Related Syscalls

Filesystem syscalls in `kernel/src/syscalls/linux.rs`:

| Syscall | Purpose |
|---------|---------|
| `openat` | Open/create file, returns fd |
| `close` | Close fd |
| `read` / `write` / `writev` | File I/O |
| `lseek` | Reposition file offset |
| `fstat` / `fstatat` | Stat a file |
| `getdents64` | Read directory entries |
| `mkdirat` | Create directory (tmpfs only) |
| `unlinkat` | Remove file/directory (tmpfs only) |
| `chdir` / `getcwd` | Change/get working directory |
| `dup3` | Duplicate fd |
| `pipe2` | Create pipe (separate from VFS) |
| `fcntl` | fd flags |
| `readlinkat` | Read symbolic link (returns EINVAL — no symlinks yet) |
