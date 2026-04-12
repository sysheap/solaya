//! Host-side sanity test for `DmaBuffer`.
//!
//! Runs on x86_64 via `cargo test -p driver-api --target x86_64-unknown-linux-gnu`.
//! `mm::page::PinnedHeapPages` goes through the Rust global allocator in this
//! environment (which honours 4 KiB alignment), so the test implicitly
//! verifies that allocation + Drop behave without requiring the kernel page
//! allocator.

use driver_api::DmaBuffer;

#[test]
fn allocate_write_read_drop() {
    let mut buf = DmaBuffer::new_coherent(4096).expect("allocate 4K");

    assert_eq!(buf.len(), 4096);
    assert_ne!(
        buf.phys_addr(),
        0,
        "page allocator must return a real address"
    );
    assert_eq!(buf.phys_addr(), buf.virt_addr() as u64);

    for (i, byte) in buf.as_mut_slice().iter_mut().enumerate() {
        *byte = (i & 0xff) as u8;
    }
    for (i, byte) in buf.as_slice().iter().enumerate() {
        assert_eq!(*byte, (i & 0xff) as u8, "mismatch at {i}");
    }

    // Drop releases the pages; allocating a new buffer afterwards must
    // succeed and typically reuses the same region under a bump-style
    // allocator.
    drop(buf);
    let another = DmaBuffer::new_coherent(8192).expect("allocate 8K after drop");
    assert_eq!(another.len(), 8192);
    assert_ne!(another.phys_addr(), 0);
}

#[test]
fn non_page_multiple_rounds_up() {
    let mut buf = DmaBuffer::new_coherent(100).expect("allocate 100B");
    assert_eq!(buf.len(), 100);
    // Write exactly len bytes; the page-rounded backing stays private.
    for byte in buf.as_mut_slice().iter_mut() {
        *byte = 0xAA;
    }
    assert!(buf.as_slice().iter().all(|b| *b == 0xAA));
}

#[test]
fn sync_is_noop() {
    let buf = DmaBuffer::new_coherent(4096).expect("allocate");
    buf.sync_for_device();
    buf.sync_for_cpu();
}
