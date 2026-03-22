mod dir;
mod file;
mod inode;
pub mod structures;

use alloc::{boxed::Box, collections::BTreeMap, string::String, vec};

use crate::{
    drivers::virtio::block,
    fs::vfs::{self, VfsNodeRef, alloc_ino},
    info,
    klibc::util::BufferExtension,
    warn,
};

use dir::Ext2Dir;
use file::Ext2File;
use inode::{read_inode, read_inode_data};
use structures::{
    EXT2_FT_DIR, EXT2_FT_REG_FILE, EXT2_MAGIC, EXT2_ROOT_INODE, Ext2BlockGroupDescriptor,
    Ext2DirEntry, Ext2Superblock,
};

pub async fn mount_ext2(dev: usize) {
    info!("ext2: mounting block device {}", dev);

    let sb = read_superblock(dev).await;
    if sb.s_magic != EXT2_MAGIC {
        warn!(
            "ext2: block device {} is not ext2 (magic 0x{:04X}), skipping",
            dev, sb.s_magic
        );
        return;
    }

    let block_size = sb.block_size();
    info!(
        "ext2: block_size={}, inodes={}, blocks={}",
        block_size, sb.s_inodes_count, sb.s_blocks_count
    );

    let bgds = read_block_group_descriptors(dev, &sb).await;

    let root = build_tree(dev, &sb, &bgds, EXT2_ROOT_INODE).await;
    vfs::mount("/mnt", root);
    info!("ext2: mounted at /mnt");
}

async fn read_superblock(dev: usize) -> Ext2Superblock {
    let sb_size = core::mem::size_of::<Ext2Superblock>();
    let mut buf = vec![0u8; sb_size];
    let n = block::read(dev, 1024, &mut buf)
        .await
        .expect("superblock read must succeed");
    assert!(n == sb_size, "short superblock read");

    sys::klibc::util::read_from_bytes(&buf)
}

async fn read_block_group_descriptors(
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
    let n = block::read(dev, bgd_offset, &mut buf)
        .await
        .expect("BGD read must succeed");
    assert!(n == total_size, "short BGD read");

    let mut bgds = alloc::vec::Vec::with_capacity(num_groups);
    for i in 0..num_groups {
        bgds.push(*buf[i * bgd_size..].interpret_as::<Ext2BlockGroupDescriptor>());
    }
    bgds
}

fn build_tree<'a>(
    dev: usize,
    sb: &'a Ext2Superblock,
    bgds: &'a [Ext2BlockGroupDescriptor],
    inode_number: u32,
) -> core::pin::Pin<Box<dyn Future<Output = VfsNodeRef> + Send + 'a>> {
    Box::pin(async move {
        let ext2_inode = read_inode(dev, sb, bgds, inode_number).await;

        if ext2_inode.is_dir() {
            let dir_data = read_inode_data(dev, sb, &ext2_inode).await;
            let entries = parse_dir_entries(&dir_data);

            let mut children = BTreeMap::new();
            for (name, child_ino, file_type) in entries {
                let child: VfsNodeRef = if file_type == EXT2_FT_DIR {
                    build_tree(dev, sb, bgds, child_ino).await
                } else if file_type == EXT2_FT_REG_FILE {
                    let child_inode = read_inode(dev, sb, bgds, child_ino).await;
                    let data = read_inode_data(dev, sb, &child_inode).await;
                    let file_size = child_inode.i_size as usize;
                    Ext2File::new(alloc_ino(), data, file_size)
                } else {
                    continue;
                };
                children.insert(name, child);
            }

            Ext2Dir::new(alloc_ino(), children) as VfsNodeRef
        } else if ext2_inode.is_regular() {
            let data = read_inode_data(dev, sb, &ext2_inode).await;
            let file_size = ext2_inode.i_size as usize;
            Ext2File::new(alloc_ino(), data, file_size) as VfsNodeRef
        } else {
            Ext2File::new(alloc_ino(), alloc::vec::Vec::new(), 0) as VfsNodeRef
        }
    })
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
