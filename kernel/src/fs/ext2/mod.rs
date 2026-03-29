mod dir;
mod file;
mod inode;
pub mod structures;

use alloc::{collections::BTreeMap, string::String, vec};

use crate::{
    drivers::virtio::block,
    fs::{
        tmpfs::TmpfsSymlink,
        vfs::{self, VfsNodeRef, alloc_ino},
    },
    info,
    klibc::util::BufferExtension,
    processes::process_table,
    warn,
};

use dir::Ext2Dir;
use file::Ext2File;
use structures::{
    EXT2_FT_DIR, EXT2_FT_REG_FILE, EXT2_FT_SYMLINK, EXT2_MAGIC, EXT2_ROOT_INODE,
    Ext2BlockGroupDescriptor, Ext2DirEntry, Ext2Superblock,
};

/// Mount ext2 from block device synchronously during early boot.
/// Uses polling-based block reads (no interrupts needed).
pub fn mount_ext2(dev: usize) {
    info!("ext2: mounting block device {}", dev);

    let sb = read_superblock(dev);
    if sb.s_magic != EXT2_MAGIC {
        warn!(
            "ext2: block device {} is not ext2 (magic 0x{:04X}), skipping",
            dev, sb.s_magic
        );
        info!("ext2: init complete");
        return;
    }

    let block_size = sb.block_size();
    info!(
        "ext2: block_size={}, inodes={}, blocks={}",
        block_size, sb.s_inodes_count, sb.s_blocks_count
    );

    let bgds = read_block_group_descriptors(dev, &sb);

    let root = build_tree(dev, &sb, &bgds, EXT2_ROOT_INODE);
    vfs::mount("/", root.clone());
    info!("ext2: mounted at /");

    let init_node = root.lookup("init").expect("/init must exist on disk image");
    let size = init_node.size();
    let buf = read_aligned(&init_node, size);
    process_table::spawn_init(buf.as_bytes());

    info!("ext2: init complete");
}

fn read_superblock(dev: usize) -> Ext2Superblock {
    let sb_size = core::mem::size_of::<Ext2Superblock>();
    let mut buf = vec![0u8; sb_size];
    let n = block::read_sync(dev, 1024, &mut buf).expect("superblock read must succeed");
    assert!(n == sb_size, "short superblock read");

    sys::klibc::util::read_from_bytes(&buf)
}

fn read_block_group_descriptors(
    dev: usize,
    sb: &Ext2Superblock,
) -> alloc::vec::Vec<Ext2BlockGroupDescriptor> {
    let block_size = sb.block_size();
    let num_groups = sb.num_block_groups() as usize;
    let bgd_size = core::mem::size_of::<Ext2BlockGroupDescriptor>();
    let total_size = num_groups * bgd_size;

    // BGD table starts at the block after the superblock
    let bgd_offset = if block_size == 1024 { 2048 } else { block_size };

    let mut buf = vec![0u8; total_size];
    let n = block::read_sync(dev, bgd_offset, &mut buf).expect("BGD read must succeed");
    assert!(n == total_size, "short BGD read");

    let mut bgds = alloc::vec::Vec::with_capacity(num_groups);
    for i in 0..num_groups {
        bgds.push(*buf[i * bgd_size..].interpret_as::<Ext2BlockGroupDescriptor>());
    }
    bgds
}

fn build_tree(
    dev: usize,
    sb: &Ext2Superblock,
    bgds: &[Ext2BlockGroupDescriptor],
    inode_number: u32,
) -> VfsNodeRef {
    let ext2_inode = inode::read_inode_sync(dev, sb, bgds, inode_number);

    if ext2_inode.is_dir() {
        let dir_data = inode::read_inode_data_sync(dev, sb, &ext2_inode);
        let entries = parse_dir_entries(&dir_data);

        let mut children = BTreeMap::new();
        for (name, child_ino, file_type) in entries {
            let child: VfsNodeRef = if file_type == EXT2_FT_DIR {
                build_tree(dev, sb, bgds, child_ino)
            } else if file_type == EXT2_FT_REG_FILE {
                let child_inode = inode::read_inode_sync(dev, sb, bgds, child_ino);
                let data = inode::read_inode_data_sync(dev, sb, &child_inode);
                let file_size = child_inode.i_size as usize;
                Ext2File::new(alloc_ino(), data, file_size)
            } else if file_type == EXT2_FT_SYMLINK {
                let child_inode = inode::read_inode_sync(dev, sb, bgds, child_ino);
                let target = read_symlink_target_sync(dev, sb, &child_inode);
                TmpfsSymlink::new(target)
            } else {
                continue;
            };
            children.insert(name, child);
        }

        Ext2Dir::new(alloc_ino(), children) as VfsNodeRef
    } else if ext2_inode.is_regular() {
        let data = inode::read_inode_data_sync(dev, sb, &ext2_inode);
        let file_size = ext2_inode.i_size as usize;
        Ext2File::new(alloc_ino(), data, file_size) as VfsNodeRef
    } else {
        Ext2File::new(alloc_ino(), alloc::vec::Vec::new(), 0) as VfsNodeRef
    }
}

/// Read symlink target. Short symlinks (< 60 bytes) store the target directly
/// in the i_block bytes (little-endian u32 array); longer ones use data blocks.
fn read_symlink_target_sync(
    dev: usize,
    sb: &Ext2Superblock,
    inode: &structures::Ext2Inode,
) -> String {
    let size = inode.i_size as usize;
    if size < 60 {
        let mut target_bytes = alloc::vec![0u8; size];
        for (i, b) in target_bytes.iter_mut().enumerate() {
            *b = (inode.i_block[i / 4] >> ((i % 4) * 8)) as u8;
        }
        String::from(core::str::from_utf8(&target_bytes).expect("symlink target must be UTF-8"))
    } else {
        let data = inode::read_inode_data_sync(dev, sb, inode);
        String::from(core::str::from_utf8(&data[..size]).expect("symlink target must be UTF-8"))
    }
}

/// Read a VfsNode into a u64-aligned buffer (required by ElfFile::parse).
fn read_aligned(node: &VfsNodeRef, size: usize) -> sys::klibc::util::AlignedBuffer {
    let mut buf = sys::klibc::util::AlignedBuffer::new(size);
    let n = node
        .read(0, buf.as_bytes_mut())
        .expect("reading file must succeed");
    assert!(n == size, "short read: got {n}, expected {size}");
    buf
}

fn parse_dir_entries(data: &[u8]) -> alloc::vec::Vec<(String, u32, u8)> {
    let mut entries = alloc::vec::Vec::new();
    let mut offset = 0;

    while offset + core::mem::size_of::<Ext2DirEntry>() <= data.len() {
        let entry: &Ext2DirEntry = data[offset..].interpret_as();
        if entry.rec_len == 0 {
            break;
        }

        if entry.inode != 0 {
            let name_start = offset + core::mem::size_of::<Ext2DirEntry>();
            let name_end = name_start + entry.name_len as usize;
            if name_end <= data.len() {
                let name = core::str::from_utf8(&data[name_start..name_end])
                    .expect("ext2 dir entry name must be valid UTF-8");
                // Skip . and .. to avoid cycles
                if name != "." && name != ".." {
                    entries.push((String::from(name), entry.inode, entry.file_type));
                }
            }
        }

        offset += entry.rec_len as usize;
    }

    entries
}
