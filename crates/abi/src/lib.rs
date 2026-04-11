//! UAPI boundary. Types and constants that are part of the user/kernel
//! contract. No functions, no statics, no macros.
//!
//! Layering invariant: may depend on nothing. Test: "would a userspace
//! program link against this?" If no, it doesn't belong.
#![no_std]
#![allow(dead_code)]
#![allow(unused_variables)]
#![feature(auto_traits)]
#![feature(negative_impls)]
#![feature(str_from_raw_parts)]

pub mod cpu;
pub mod errors;
pub mod ioctl;
pub mod macros;
pub mod numbers;
pub mod pid;
pub mod pointer;
pub mod syscalls;
