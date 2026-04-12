use alloc::{collections::BTreeMap, vec::Vec};

use crate::{debug, klibc::MMIO};
use driver_api::DmaBuffer;
use klib::{deconstructed_vec::DeconstructedVec, non_empty_vec::NonEmptyVec};

/// A virtio queue.
///
/// The three ring areas live in [`DmaBuffer`]s — the device reads/writes them
/// via their physical addresses. `DmaBuffer` owns the page-aligned backing
/// memory and releases it on `Drop`, replacing the earlier `Box` + raw cast
/// pattern.
pub struct VirtQueue<const QUEUE_SIZE: usize> {
    descriptor_area: DmaBuffer,
    free_descriptor_indices: Vec<u16>,
    outstanding_buffers: BTreeMap<u16, DeconstructedVec>,
    last_used_ring_index: u16,
    driver_area: DmaBuffer,
    device_area: DmaBuffer,
    queue_index: u16,
    notify: Option<MMIO<u16>>,
}

pub enum BufferDirection {
    DriverWritable,
    DeviceWritable,
}

#[derive(Debug)]
pub enum QueueError {
    NoFreeDescriptors,
}

impl<const QUEUE_SIZE: usize> VirtQueue<QUEUE_SIZE> {
    pub fn new(queue_size: u16, queue_index: u16) -> Self {
        assert!(
            queue_size == u16::try_from(QUEUE_SIZE).expect("queue size fits in u16"),
            "Queue size must be equal"
        );
        assert!(
            queue_size.is_power_of_two(),
            "Queue size must be a power of 2"
        );

        let descriptor_area =
            DmaBuffer::new_coherent(core::mem::size_of::<[virtq_desc; QUEUE_SIZE]>())
                .expect("allocate virtq descriptor area");
        let mut driver_area =
            DmaBuffer::new_coherent(core::mem::size_of::<virtq_avail<QUEUE_SIZE>>())
                .expect("allocate virtq driver area");
        let mut device_area =
            DmaBuffer::new_coherent(core::mem::size_of::<virtq_used<QUEUE_SIZE>>())
                .expect("allocate virtq device area");

        // DmaBuffer returns zeroed pages; set the non-zero defaults in place.
        *driver_area.as_typed_mut::<virtq_avail<QUEUE_SIZE>>() = virtq_avail::default();
        *device_area.as_typed_mut::<virtq_used<QUEUE_SIZE>>() = virtq_used::default();

        let queue = VirtQueue {
            descriptor_area,
            free_descriptor_indices: (0..queue_size).collect(),
            outstanding_buffers: BTreeMap::new(),
            last_used_ring_index: 0,
            driver_area,
            device_area,
            queue_index,
            notify: None,
        };
        assert!(
            queue.descriptor_area_physical_address().is_multiple_of(16),
            "Descriptor area not aligned"
        );
        assert!(
            queue.driver_area_physical_address().is_multiple_of(2),
            "Driver area not aligned"
        );
        assert!(
            queue.device_area_physical_address().is_multiple_of(4),
            "Device area not aligned"
        );

        queue
    }

    pub fn set_notify(&mut self, notify: MMIO<u16>) {
        self.notify = Some(notify);
    }

    pub fn descriptor_area_physical_address(&self) -> u64 {
        self.descriptor_area.phys_addr()
    }

    pub fn driver_area_physical_address(&self) -> u64 {
        self.driver_area.phys_addr()
    }

    pub fn device_area_physical_address(&self) -> u64 {
        self.device_area.phys_addr()
    }

    fn descriptors(&self) -> &[virtq_desc; QUEUE_SIZE] {
        self.descriptor_area.as_typed::<[virtq_desc; QUEUE_SIZE]>()
    }

    fn descriptors_mut(&mut self) -> &mut [virtq_desc; QUEUE_SIZE] {
        self.descriptor_area
            .as_typed_mut::<[virtq_desc; QUEUE_SIZE]>()
    }

    fn driver_area_mut(&mut self) -> &mut virtq_avail<QUEUE_SIZE> {
        self.driver_area.as_typed_mut::<virtq_avail<QUEUE_SIZE>>()
    }

    fn device_area(&self) -> &virtq_used<QUEUE_SIZE> {
        self.device_area.as_typed::<virtq_used<QUEUE_SIZE>>()
    }

    /// Put a single buffer into the virtqueue.
    pub fn put_buffer(
        &mut self,
        buffer: Vec<u8>,
        direction: BufferDirection,
    ) -> Result<u16, QueueError> {
        self.put_buffer_chain(NonEmptyVec::new((buffer, direction)))
    }

    /// Put a chain of descriptors into the virtqueue.
    /// Each element is (buffer, direction). The descriptors are chained via VIRTQ_DESC_F_NEXT.
    /// Only the head descriptor index is placed in the available ring.
    pub fn put_buffer_chain(
        &mut self,
        buffers: NonEmptyVec<(Vec<u8>, BufferDirection)>,
    ) -> Result<u16, QueueError> {
        if self.free_descriptor_indices.len() < buffers.len() {
            return Err(QueueError::NoFreeDescriptors);
        }

        let descriptor_count = buffers.len();
        let mut indices: Vec<u16> = Vec::with_capacity(descriptor_count);
        for _ in 0..descriptor_count {
            indices.push(self.free_descriptor_indices.pop().expect("checked above"));
        }

        for (i, (buffer, direction)) in buffers.into_iter().enumerate() {
            let desc_idx = indices[i];
            let descriptor = &mut self.descriptors_mut()[desc_idx as usize];
            // SAFETY (documented, non-unsafe cast): `buffer.as_ptr()` is the
            // kernel virtual address of a heap-backed Vec<u8>. The kernel
            // identity-maps RAM, so this equals the physical address the
            // virtio device will DMA against. Migrating per-request buffers
            // to DmaBuffer is deferred (ring memory migrated in this phase).
            descriptor.addr = buffer.as_ptr() as u64;
            descriptor.len = u32::try_from(buffer.len()).expect("buffer length fits in u32");

            let mut flags = match direction {
                BufferDirection::DeviceWritable => VIRTQ_DESC_F_WRITE,
                BufferDirection::DriverWritable => 0,
            };

            if i + 1 < descriptor_count {
                flags |= VIRTQ_DESC_F_NEXT;
                descriptor.next = indices[i + 1];
            } else {
                descriptor.next = 0;
            }
            descriptor.flags = flags;

            let insert_result = self
                .outstanding_buffers
                .insert(desc_idx, DeconstructedVec::from_vec(buffer))
                .is_none();
            assert!(
                insert_result,
                "Outstanding buffers is not allowed to contain this index"
            );
        }

        let head = indices[0];

        // Only head goes into the available ring
        let driver_area = self.driver_area_mut();
        let slot = driver_area.idx as usize % QUEUE_SIZE;
        driver_area.ring[slot] = head;

        hal::cpu::memory_fence();

        let driver_area = self.driver_area_mut();
        driver_area.idx = driver_area.idx.wrapping_add(1);

        hal::cpu::memory_fence();

        Ok(head)
    }

    pub fn receive_buffer(&mut self) -> Vec<UsedBuffer> {
        hal::cpu::memory_fence();
        let current_device_index = self.device_area().idx;
        if self.last_used_ring_index == current_device_index {
            return Vec::new();
        }
        debug!("Current device index: {:#x?}", current_device_index);
        let mut return_buffers: Vec<UsedBuffer> = Vec::new();
        while self.last_used_ring_index != current_device_index {
            debug!("last used ring index: {:#x?}", self.last_used_ring_index);
            let ring_slot = self.last_used_ring_index as usize % QUEUE_SIZE;
            let result_id = self.device_area().ring[ring_slot].id;
            let result_len = self.device_area().ring[ring_slot].len;
            assert!(
                (result_id as usize) < QUEUE_SIZE,
                "Device returned descriptor ID {} outside queue bounds {}",
                result_id,
                QUEUE_SIZE
            );

            let head_index = u16::try_from(result_id).expect("descriptor id fits in u16");
            let total_written = result_len as usize;

            // Follow the descriptor chain collecting all buffers
            let mut first_buffer: Option<Vec<u8>> = None;
            let mut rest_buffers: Vec<Vec<u8>> = Vec::new();
            let mut current_idx = head_index;
            let mut remaining_written = total_written;

            loop {
                let (is_device_writable, has_next, next_idx) = {
                    let descriptor = &self.descriptors()[current_idx as usize];
                    (
                        descriptor.flags & VIRTQ_DESC_F_WRITE != 0,
                        descriptor.flags & VIRTQ_DESC_F_NEXT != 0,
                        descriptor.next,
                    )
                };

                let stored = self
                    .outstanding_buffers
                    .remove(&current_idx)
                    .expect("There must be an outstanding buffer for this id");

                let buf_len = if is_device_writable {
                    let len = core::cmp::min(remaining_written, stored.length());
                    remaining_written = remaining_written.saturating_sub(stored.length());
                    len
                } else {
                    stored.length()
                };

                let buf = stored.into_vec_with_len(buf_len);
                if first_buffer.is_none() {
                    first_buffer = Some(buf);
                } else {
                    rest_buffers.push(buf);
                }

                let descriptor = &mut self.descriptors_mut()[current_idx as usize];
                descriptor.addr = 0;
                descriptor.len = 0;
                descriptor.flags = 0;
                descriptor.next = 0;
                self.free_descriptor_indices.push(current_idx);

                if has_next {
                    current_idx = next_idx;
                } else {
                    break;
                }
            }

            let first = first_buffer.expect("chain always has at least one descriptor");
            let mut buffers = NonEmptyVec::new(first);
            for buf in rest_buffers {
                buffers = buffers.push(buf);
            }

            return_buffers.push(UsedBuffer {
                index: head_index,
                buffers,
            });
            self.last_used_ring_index = self.last_used_ring_index.wrapping_add(1);
        }
        return_buffers
    }

    pub fn enable_interrupts(&mut self) {
        self.driver_area_mut().flags = 0;
        hal::cpu::memory_fence();
    }

    pub fn notify(&mut self) {
        if let Some(notify) = &mut self.notify {
            notify.write(self.queue_index);
        }
    }
}

#[derive(Debug)]
pub struct UsedBuffer {
    pub index: u16,
    pub buffers: NonEmptyVec<Vec<u8>>,
}

/* This marks a buffer as continuing via the next field. */
const VIRTQ_DESC_F_NEXT: u16 = 1;
/* This marks a buffer as device write-only (otherwise device read-only). */
const VIRTQ_DESC_F_WRITE: u16 = 2;
/* This means the buffer contains a list of buffer descriptors. */
#[allow(dead_code)]
const VIRTQ_DESC_F_INDIRECT: u16 = 4;

#[allow(non_camel_case_types)]
#[repr(C, align(16))]
#[derive(Default, Debug)]
struct virtq_desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

const VIRTQ_AVAIL_F_NO_INTERRUPT: u16 = 1;

#[allow(non_camel_case_types)]
#[repr(C, align(2))]
struct virtq_avail<const QUEUE_SIZE: usize> {
    flags: u16,
    idx: u16,
    ring: [u16; QUEUE_SIZE],
    used_event: u16, /* Only if VIRTIO_F_EVENT_IDX */
}

impl<const QUEUE_SIZE: usize> Default for virtq_avail<QUEUE_SIZE> {
    fn default() -> Self {
        Self {
            flags: VIRTQ_AVAIL_F_NO_INTERRUPT, // Ignore interrupts for the beginning
            idx: 0,
            ring: [0; QUEUE_SIZE],
            used_event: Default::default(),
        }
    }
}

const VIRTQ_USED_F_NO_NOTIFY: u16 = 1;

#[allow(non_camel_case_types)]
#[repr(C, align(4))]
struct virtq_used<const QUEUE_SIZE: usize> {
    flags: u16,
    idx: u16,
    ring: [virtq_used_elem; QUEUE_SIZE],
    avail_event: u16, /* Only if VIRTIO_F_EVENT_IDX */
}

impl<const QUEUE_SIZE: usize> Default for virtq_used<QUEUE_SIZE> {
    fn default() -> Self {
        Self {
            flags: VIRTQ_USED_F_NO_NOTIFY,
            idx: 0,
            ring: core::array::from_fn(|_| virtq_used_elem::default()),
            avail_event: Default::default(),
        }
    }
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Default, Debug)]
struct virtq_used_elem {
    id: u32, /* Index of start of used descriptor chain. */
    len: u32, /*
              * The number of bytes written into the device writable portion of
              * the buffer described by the descriptor chain.
              */
}
