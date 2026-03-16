#[cfg(feature = "virtio-blk")]
pub mod block;
mod capability;
pub mod input;
#[cfg(feature = "virtio-net")]
pub mod net;
pub mod rng;
mod virtqueue;
