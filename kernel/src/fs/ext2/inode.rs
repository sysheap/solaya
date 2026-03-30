use alloc::{vec, vec::Vec};

use crate::{drivers::virtio::block, klibc::util::BufferExtension};

use super::structures::{
    EXT2_DIND_BLOCK, EXT2_IND_BLOCK, EXT2_NDIR_BLOCKS, EXT2_TIND_BLOCK, Ext2BlockGroupDescriptor,
    Ext2Inode, Ext2Superblock,
};

pub fn read_inode_sync(
    dev: usize,
    sb: &Ext2Superblock,
    bgds: &[Ext2BlockGroupDescriptor],
    inode_number: u32,
) -> Ext2Inode {
    assert!(inode_number >= 1, "inode numbers start at 1");
    let group = ((inode_number - 1) / sb.s_inodes_per_group) as usize;
    let index = ((inode_number - 1) % sb.s_inodes_per_group) as usize;
    let inode_size = sb.inode_size();
    let block_size = sb.block_size();

    let offset = bgds[group].bg_inode_table as usize * block_size + index * inode_size;
    let mut buf = vec![0u8; inode_size];
    let n = block::read_sync(dev, offset, &mut buf).expect("inode read must succeed");
    assert!(n == inode_size, "short inode read");

    sys::klibc::util::read_from_bytes(&buf)
}

pub fn read_inode_data_sync(dev: usize, sb: &Ext2Superblock, inode: &Ext2Inode) -> Vec<u8> {
    let file_size = inode.i_size as usize;
    if file_size == 0 {
        return Vec::new();
    }

    let block_size = sb.block_size();
    let mut data = Vec::with_capacity(file_size);
    let mut remaining = file_size;

    for i in 0..EXT2_NDIR_BLOCKS {
        if remaining == 0 {
            break;
        }
        if inode.i_block[i] == 0 {
            let hole = remaining.min(block_size);
            data.resize(data.len() + hole, 0);
        } else {
            read_block_data_sync(dev, inode.i_block[i], block_size, remaining, &mut data);
        }
        remaining = file_size.saturating_sub(data.len());
    }

    if remaining > 0 && inode.i_block[EXT2_IND_BLOCK] != 0 {
        read_indirect_sync(
            dev,
            inode.i_block[EXT2_IND_BLOCK],
            block_size,
            file_size,
            &mut data,
        );
        remaining = file_size.saturating_sub(data.len());
    }

    if remaining > 0 && inode.i_block[EXT2_DIND_BLOCK] != 0 {
        read_doubly_indirect_sync(
            dev,
            inode.i_block[EXT2_DIND_BLOCK],
            block_size,
            file_size,
            &mut data,
        );
        remaining = file_size.saturating_sub(data.len());
    }

    if remaining > 0 && inode.i_block[EXT2_TIND_BLOCK] != 0 {
        read_triply_indirect_sync(
            dev,
            inode.i_block[EXT2_TIND_BLOCK],
            block_size,
            file_size,
            &mut data,
        );
    }

    data.truncate(file_size);
    data
}

fn read_block_data_sync(
    dev: usize,
    block_num: u32,
    block_size: usize,
    remaining: usize,
    data: &mut Vec<u8>,
) {
    let offset = block_num as usize * block_size;
    let to_read = remaining.min(block_size);
    let start = data.len();
    data.resize(start + to_read, 0);
    let n = block::read_sync(dev, offset, &mut data[start..start + to_read])
        .expect("block read must succeed");
    assert!(n == to_read, "short block read");
}

fn read_block_pointers_sync(dev: usize, block_num: u32, block_size: usize) -> Vec<u32> {
    let mut buf = vec![0u8; block_size];
    let offset = block_num as usize * block_size;
    let n = block::read_sync(dev, offset, &mut buf).expect("indirect block read must succeed");
    assert!(n == block_size, "short indirect block read");

    let ptrs_per_block = block_size / 4;
    let mut pointers = Vec::with_capacity(ptrs_per_block);
    for i in 0..ptrs_per_block {
        pointers.push(*buf[i * 4..].interpret_as::<u32>());
    }
    pointers
}

fn read_indirect_sync(
    dev: usize,
    indirect_block: u32,
    block_size: usize,
    file_size: usize,
    data: &mut Vec<u8>,
) {
    let pointers = read_block_pointers_sync(dev, indirect_block, block_size);
    for &ptr in &pointers {
        if data.len() >= file_size {
            break;
        }
        let remaining = file_size - data.len();
        if ptr == 0 {
            let hole = remaining.min(block_size);
            data.resize(data.len() + hole, 0);
        } else {
            read_block_data_sync(dev, ptr, block_size, remaining, data);
        }
    }
}

fn read_doubly_indirect_sync(
    dev: usize,
    dind_block: u32,
    block_size: usize,
    file_size: usize,
    data: &mut Vec<u8>,
) {
    let l1_pointers = read_block_pointers_sync(dev, dind_block, block_size);
    for &l1_ptr in &l1_pointers {
        if data.len() >= file_size {
            break;
        }
        if l1_ptr != 0 {
            read_indirect_sync(dev, l1_ptr, block_size, file_size, data);
        }
    }
}

fn read_triply_indirect_sync(
    dev: usize,
    tind_block: u32,
    block_size: usize,
    file_size: usize,
    data: &mut Vec<u8>,
) {
    let l1_pointers = read_block_pointers_sync(dev, tind_block, block_size);
    for &l1_ptr in &l1_pointers {
        if data.len() >= file_size {
            break;
        }
        if l1_ptr != 0 {
            read_doubly_indirect_sync(dev, l1_ptr, block_size, file_size, data);
        }
    }
}
