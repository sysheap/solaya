//! initramfs support: parse a cpio archive and materialize it into tmpfs
//! at boot. Discovery (DTB `/chosen/linux,initrd-{start,end}`) and
//! extraction into the VFS live here; the pure-format parser lives in
//! [`cpio`].
//!
//! Wired into `kernel_init` in a later commit; this commit adds the parser
//! only. The allow(dead_code) is lifted when the boot path calls into
//! `cpio::iter`.
#![allow(dead_code)]

mod cpio;
