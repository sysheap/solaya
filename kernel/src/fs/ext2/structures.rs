pub const EXT2_MAGIC: u16 = 0xEF53;
pub const EXT2_ROOT_INODE: u32 = 2;

pub const EXT2_FT_REG_FILE: u8 = 1;
pub const EXT2_FT_DIR: u8 = 2;
pub const EXT2_FT_SYMLINK: u8 = 7;

pub const S_IFMT: u16 = 0xF000;
pub const S_IFDIR: u16 = 0x4000;
pub const S_IFREG: u16 = 0x8000;

pub const EXT2_NDIR_BLOCKS: usize = 12;
pub const EXT2_IND_BLOCK: usize = 12;
pub const EXT2_DIND_BLOCK: usize = 13;
pub const EXT2_TIND_BLOCK: usize = 14;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Ext2Superblock {
    pub s_inodes_count: u32,
    pub s_blocks_count: u32,
    pub s_r_blocks_count: u32,
    pub s_free_blocks_count: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_log_frag_size: u32,
    pub s_blocks_per_group: u32,
    pub s_frags_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    pub s_magic: u16,
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_lastcheck: u32,
    pub s_checkinterval: u32,
    pub s_creator_os: u32,
    pub s_rev_level: u32,
    pub s_def_resuid: u16,
    pub s_def_resgid: u16,
    // EXT2_DYNAMIC_REV fields
    pub s_first_ino: u32,
    pub s_inode_size: u16,
}

impl Ext2Superblock {
    pub fn block_size(&self) -> usize {
        1024 << self.s_log_block_size
    }

    pub fn inode_size(&self) -> usize {
        if self.s_rev_level == 0 {
            128
        } else {
            self.s_inode_size as usize
        }
    }

    pub fn num_block_groups(&self) -> u32 {
        self.s_blocks_count.div_ceil(self.s_blocks_per_group)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ext2BlockGroupDescriptor {
    pub bg_block_bitmap: u32,
    pub bg_inode_bitmap: u32,
    pub bg_inode_table: u32,
    pub bg_free_blocks_count: u16,
    pub bg_free_inodes_count: u16,
    pub bg_used_dirs_count: u16,
    pub bg_pad: u16,
    pub bg_reserved: [u32; 3],
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Ext2Inode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks: u32,
    pub i_flags: u32,
    pub i_osd1: u32,
    pub i_block: [u32; 15],
    pub i_generation: u32,
    pub i_file_acl: u32,
    pub i_dir_acl: u32,
    pub i_faddr: u32,
    pub i_osd2: [u8; 12],
}

impl Ext2Inode {
    pub fn is_dir(&self) -> bool {
        self.i_mode & S_IFMT == S_IFDIR
    }

    pub fn is_regular(&self) -> bool {
        self.i_mode & S_IFMT == S_IFREG
    }
}

#[repr(C)]
pub struct Ext2DirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
    // name follows (variable length)
}
