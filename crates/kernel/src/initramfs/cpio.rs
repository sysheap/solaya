//! newc-format cpio archive parser.
//!
//! Parses SVR4 portable (newc) cpio archives, uncompressed. Takes a full
//! in-memory slice and yields entries in archive order. Used by the
//! initramfs module to populate tmpfs at boot.
//!
//! Format reference: `initramfs_data.cpio` produced by `find . | cpio -o -H newc`.
//! Each record is a 110-byte ASCII header followed by a NUL-terminated
//! filename and file data, each padded to 4-byte alignment from the start
//! of the archive. The archive ends with an entry named `TRAILER!!!`.

use core::fmt;

const MAGIC: &[u8; 6] = b"070701";
const HEADER_SIZE: usize = 110;
const TRAILER: &str = "TRAILER!!!";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpioError {
    Truncated,
    BadMagic,
    BadHex,
    InvalidName,
}

impl fmt::Display for CpioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CpioError::Truncated => write!(f, "cpio: archive truncated"),
            CpioError::BadMagic => write!(f, "cpio: bad magic (not newc)"),
            CpioError::BadHex => write!(f, "cpio: malformed hex field"),
            CpioError::InvalidName => write!(f, "cpio: filename not UTF-8 or missing NUL"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub ino: u32,
    pub mode: u32,
    pub nlink: u32,
    pub name: &'a str,
    pub data: &'a [u8],
}

pub fn iter(archive: &[u8]) -> CpioIter<'_> {
    CpioIter {
        buf: archive,
        pos: 0,
        done: false,
    }
}

pub struct CpioIter<'a> {
    buf: &'a [u8],
    pos: usize,
    done: bool,
}

impl<'a> Iterator for CpioIter<'a> {
    type Item = Result<Entry<'a>, CpioError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.pos >= self.buf.len() {
            return None;
        }
        match read_one(self.buf, self.pos) {
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
            Ok((entry, _)) if entry.name == TRAILER => {
                self.done = true;
                None
            }
            Ok((entry, new_pos)) => {
                self.pos = new_pos;
                Some(Ok(entry))
            }
        }
    }
}

fn read_one(buf: &[u8], pos: usize) -> Result<(Entry<'_>, usize), CpioError> {
    let header = buf
        .get(pos..pos + HEADER_SIZE)
        .ok_or(CpioError::Truncated)?;
    if &header[0..6] != MAGIC {
        return Err(CpioError::BadMagic);
    }
    let ino = parse_hex(&header[6..14])?;
    let mode = parse_hex(&header[14..22])?;
    let nlink = parse_hex(&header[38..46])?;
    let filesize = parse_hex(&header[54..62])? as usize;
    let namesize = parse_hex(&header[94..102])? as usize;

    let name_start = pos + HEADER_SIZE;
    let name_end = name_start
        .checked_add(namesize)
        .ok_or(CpioError::Truncated)?;
    let name_bytes = buf.get(name_start..name_end).ok_or(CpioError::Truncated)?;
    if namesize == 0 || *name_bytes.last().ok_or(CpioError::InvalidName)? != 0 {
        return Err(CpioError::InvalidName);
    }
    let name =
        core::str::from_utf8(&name_bytes[..namesize - 1]).map_err(|_| CpioError::InvalidName)?;

    let data_start = align4(name_end);
    let data_end = data_start
        .checked_add(filesize)
        .ok_or(CpioError::Truncated)?;
    let data = buf.get(data_start..data_end).ok_or(CpioError::Truncated)?;

    let next_pos = align4(data_end);
    Ok((
        Entry {
            ino,
            mode,
            nlink,
            name,
            data,
        },
        next_pos,
    ))
}

fn parse_hex(bytes: &[u8]) -> Result<u32, CpioError> {
    assert_eq!(bytes.len(), 8, "newc hex fields are always 8 ASCII chars");
    let mut acc: u32 = 0;
    for &b in bytes {
        let nibble = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(CpioError::BadHex),
        };
        acc = (acc << 4) | u32::from(nibble);
    }
    Ok(acc)
}

const fn align4(x: usize) -> usize {
    (x + 3) & !3
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};
    use headers::fs::{S_IFDIR, S_IFLNK, S_IFREG};

    fn write_hex(dst: &mut Vec<u8>, value: u32) {
        let s = alloc::format!("{value:08x}");
        dst.extend_from_slice(s.as_bytes());
    }

    fn push_record(
        archive: &mut Vec<u8>,
        ino: u32,
        mode: u32,
        nlink: u32,
        name: &str,
        data: &[u8],
    ) {
        let name_with_nul_len = (name.len() + 1) as u32;
        let header_start = archive.len();
        archive.extend_from_slice(MAGIC);
        write_hex(archive, ino);
        write_hex(archive, mode);
        write_hex(archive, 0); // uid
        write_hex(archive, 0); // gid
        write_hex(archive, nlink);
        write_hex(archive, 0); // mtime
        write_hex(archive, data.len() as u32);
        write_hex(archive, 0); // devmajor
        write_hex(archive, 0); // devminor
        write_hex(archive, 0); // rdevmajor
        write_hex(archive, 0); // rdevminor
        write_hex(archive, name_with_nul_len);
        write_hex(archive, 0); // check
        assert_eq!(archive.len() - header_start, HEADER_SIZE);
        archive.extend_from_slice(name.as_bytes());
        archive.push(0);
        while !archive.len().is_multiple_of(4) {
            archive.push(0);
        }
        archive.extend_from_slice(data);
        while !archive.len().is_multiple_of(4) {
            archive.push(0);
        }
    }

    fn push_trailer(archive: &mut Vec<u8>) {
        push_record(archive, 0, 0, 1, TRAILER, &[]);
    }

    #[test_case]
    fn empty_archive_iterates_empty() {
        let mut archive = Vec::new();
        push_trailer(&mut archive);
        let entries: Vec<_> = iter(&archive).collect();
        assert_eq!(entries.len(), 0);
    }

    #[test_case]
    fn single_file() {
        let mut archive = Vec::new();
        push_record(&mut archive, 42, S_IFREG | 0o644, 1, "hello.txt", b"hi\n");
        push_trailer(&mut archive);

        let entries: Vec<_> = iter(&archive).map(|r| r.expect("parse")).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].ino, 42);
        assert_eq!(entries[0].mode, S_IFREG | 0o644);
        assert_eq!(entries[0].nlink, 1);
        assert_eq!(entries[0].name, "hello.txt");
        assert_eq!(entries[0].data, b"hi\n");
    }

    #[test_case]
    fn three_entries_dir_file_symlink() {
        let mut archive = Vec::new();
        push_record(&mut archive, 1, S_IFDIR | 0o755, 2, "bin", &[]);
        push_record(
            &mut archive,
            2,
            S_IFREG | 0o755,
            1,
            "bin/dash",
            b"\x7fELFPAYLOAD",
        );
        push_record(&mut archive, 3, S_IFLNK | 0o777, 1, "bin/sh", b"dash");
        push_trailer(&mut archive);

        let entries: Vec<_> = iter(&archive).map(|r| r.expect("parse")).collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "bin");
        assert_eq!(entries[0].mode & S_IFDIR, S_IFDIR);
        assert_eq!(entries[1].name, "bin/dash");
        assert_eq!(entries[1].data, b"\x7fELFPAYLOAD");
        assert_eq!(entries[2].name, "bin/sh");
        assert_eq!(entries[2].mode & S_IFLNK, S_IFLNK);
        assert_eq!(entries[2].data, b"dash");
    }

    #[test_case]
    fn hardlinks_share_inode() {
        let mut archive = Vec::new();
        push_record(&mut archive, 7, S_IFREG | 0o755, 2, "bin/cat", b"BINARY");
        push_record(&mut archive, 7, S_IFREG | 0o755, 2, "bin/head", &[]);
        push_trailer(&mut archive);

        let entries: Vec<_> = iter(&archive).map(|r| r.expect("parse")).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].ino, entries[1].ino);
        assert_eq!(entries[0].nlink, 2);
        assert_eq!(entries[0].data, b"BINARY");
        assert_eq!(entries[1].data, b"");
    }

    #[test_case]
    fn padding_works_for_awkward_sizes() {
        // namesize and filesize chosen so the 4-byte padding at each step
        // matters: header=110, name 3 bytes +NUL=4 -> 110+4=114, pad 2
        // -> data at 116; data 5 bytes -> 121, pad 3 -> next at 124.
        let mut archive = Vec::new();
        push_record(&mut archive, 1, S_IFREG | 0o644, 1, "foo", b"ABCDE");
        push_record(&mut archive, 2, S_IFREG | 0o644, 1, "bar", b"XYZ");
        push_trailer(&mut archive);

        let entries: Vec<_> = iter(&archive).map(|r| r.expect("parse")).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "foo");
        assert_eq!(entries[0].data, b"ABCDE");
        assert_eq!(entries[1].name, "bar");
        assert_eq!(entries[1].data, b"XYZ");
    }

    #[test_case]
    fn bad_magic_returns_error() {
        let mut archive = vec![b'0', b'7', b'0', b'7', b'0', b'0']; // "070700"
        archive.extend_from_slice(&[b'0'; HEADER_SIZE - 6]);
        let mut it = iter(&archive);
        assert_eq!(it.next(), Some(Err(CpioError::BadMagic)));
        assert_eq!(it.next(), None);
    }

    #[test_case]
    fn truncated_header_returns_error() {
        let archive = vec![0u8; 50];
        let mut it = iter(&archive);
        assert_eq!(it.next(), Some(Err(CpioError::Truncated)));
    }

    #[test_case]
    fn truncated_data_returns_error() {
        let mut archive = Vec::new();
        push_record(&mut archive, 1, S_IFREG, 1, "x", b"AAAAAAAA");
        // Chop off the last 4 bytes to simulate a truncated archive.
        archive.truncate(archive.len() - 4);
        let mut it = iter(&archive);
        assert!(matches!(it.next(), Some(Err(CpioError::Truncated))));
    }

    #[test_case]
    fn bad_hex_returns_error() {
        let mut archive = Vec::new();
        push_record(&mut archive, 1, S_IFREG, 1, "x", b"");
        push_trailer(&mut archive);
        // Corrupt a hex digit in the first record's ino field (offset 6..14).
        archive[6] = b'Z';
        let mut it = iter(&archive);
        assert_eq!(it.next(), Some(Err(CpioError::BadHex)));
    }

    #[test_case]
    fn trailer_ends_iteration_even_if_more_bytes_follow() {
        let mut archive = Vec::new();
        push_record(&mut archive, 1, S_IFREG, 1, "a", b"");
        push_trailer(&mut archive);
        archive.extend_from_slice(&[0xff; 32]); // garbage past TRAILER
        let entries: Vec<_> = iter(&archive).map(|r| r.expect("parse")).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a");
    }
}
