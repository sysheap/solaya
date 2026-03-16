#![allow(unsafe_code)]
use alloc::{vec, vec::Vec};

use crate::{drivers::virtio::block, klibc::util::BufferExtension};

use super::structures::{
    EXT2_DIND_BLOCK, EXT2_IND_BLOCK, EXT2_NDIR_BLOCKS, EXT2_TIND_BLOCK, Ext2BlockGroupDescriptor,
    Ext2Inode, Ext2Superblock,
};

pub async fn read_inode(
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
    let n = block::read(dev, offset, &mut buf)
        .await
        .expect("inode read must succeed");
    assert!(n == inode_size, "short inode read");

    // Copy into a properly aligned Ext2Inode
    let mut inode = core::mem::MaybeUninit::<Ext2Inode>::uninit();
    let inode_bytes = core::mem::size_of::<Ext2Inode>();
    // SAFETY: We copy exactly the right number of bytes into uninitialized memory,
    // then assume_init since all fields are plain integers with no invalid bit patterns.
    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), inode.as_mut_ptr().cast::<u8>(), inode_bytes);
        inode.assume_init()
    }
}

pub async fn read_inode_data(dev: usize, sb: &Ext2Superblock, inode: &Ext2Inode) -> Vec<u8> {
    let file_size = inode.i_size as usize;
    if file_size == 0 {
        return Vec::new();
    }

    let block_size = sb.block_size();
    let mut data = Vec::with_capacity(file_size);
    let mut remaining = file_size;

    // Direct blocks
    for i in 0..EXT2_NDIR_BLOCKS {
        if remaining == 0 {
            break;
        }
        if inode.i_block[i] == 0 {
            break;
        }
        read_block_data(dev, inode.i_block[i], block_size, remaining, &mut data).await;
        remaining = file_size.saturating_sub(data.len());
    }

    // Indirect block
    if remaining > 0 && inode.i_block[EXT2_IND_BLOCK] != 0 {
        read_indirect(
            dev,
            inode.i_block[EXT2_IND_BLOCK],
            block_size,
            file_size,
            &mut data,
        )
        .await;
        remaining = file_size.saturating_sub(data.len());
    }

    // Doubly indirect block
    if remaining > 0 && inode.i_block[EXT2_DIND_BLOCK] != 0 {
        read_doubly_indirect(
            dev,
            inode.i_block[EXT2_DIND_BLOCK],
            block_size,
            file_size,
            &mut data,
        )
        .await;
        remaining = file_size.saturating_sub(data.len());
    }

    // Triply indirect block
    if remaining > 0 && inode.i_block[EXT2_TIND_BLOCK] != 0 {
        read_triply_indirect(
            dev,
            inode.i_block[EXT2_TIND_BLOCK],
            block_size,
            file_size,
            &mut data,
        )
        .await;
    }

    data.truncate(file_size);
    data
}

async fn read_block_data(
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
    let n = block::read(dev, offset, &mut data[start..start + to_read])
        .await
        .expect("block read must succeed");
    assert!(n == to_read, "short block read");
}

async fn read_block_pointers(dev: usize, block_num: u32, block_size: usize) -> Vec<u32> {
    let mut buf = vec![0u8; block_size];
    let offset = block_num as usize * block_size;
    let n = block::read(dev, offset, &mut buf)
        .await
        .expect("indirect block read must succeed");
    assert!(n == block_size, "short indirect block read");

    let ptrs_per_block = block_size / 4;
    let mut pointers = Vec::with_capacity(ptrs_per_block);
    for i in 0..ptrs_per_block {
        pointers.push(*buf[i * 4..].interpret_as::<u32>());
    }
    pointers
}

async fn read_indirect(
    dev: usize,
    indirect_block: u32,
    block_size: usize,
    file_size: usize,
    data: &mut Vec<u8>,
) {
    let pointers = read_block_pointers(dev, indirect_block, block_size).await;
    for &ptr in &pointers {
        if ptr == 0 || data.len() >= file_size {
            break;
        }
        let remaining = file_size - data.len();
        read_block_data(dev, ptr, block_size, remaining, data).await;
    }
}

async fn read_doubly_indirect(
    dev: usize,
    dind_block: u32,
    block_size: usize,
    file_size: usize,
    data: &mut Vec<u8>,
) {
    let l1_pointers = read_block_pointers(dev, dind_block, block_size).await;
    for &l1_ptr in &l1_pointers {
        if l1_ptr == 0 || data.len() >= file_size {
            break;
        }
        read_indirect(dev, l1_ptr, block_size, file_size, data).await;
    }
}

async fn read_triply_indirect(
    dev: usize,
    tind_block: u32,
    block_size: usize,
    file_size: usize,
    data: &mut Vec<u8>,
) {
    let l1_pointers = read_block_pointers(dev, tind_block, block_size).await;
    for &l1_ptr in &l1_pointers {
        if l1_ptr == 0 || data.len() >= file_size {
            break;
        }
        read_doubly_indirect(dev, l1_ptr, block_size, file_size, data).await;
    }
}
