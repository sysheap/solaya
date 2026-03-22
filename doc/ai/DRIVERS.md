# Device Drivers

## Overview

Device driver subsystems:
1. **PCI** - PCI device enumeration and configuration
2. **VirtIO** - VirtIO device framework (network, block)

## Clean-Room Development Policy

All drivers must be implemented from scratch using public hardware specs, VirtIO specifications, and RFCs. Never reference or port Linux kernel driver source code — Solaya is MIT-licensed and Linux drivers are GPL-2.0. If no public spec exists for a device, that device cannot be supported until one becomes available or a contributor takes on the licensing implications independently.

## PCI Subsystem

**File:** `kernel/src/pci/mod.rs`

### PCI Device Discovery

```rust
pub fn enumerate_devices(pci_info: &PCIInformation) -> EnumeratedDevices {
    // Scan PCI configuration space
    // Find devices by vendor/device ID
    // Return categorized devices (network, storage, etc.)
}
```

### PCI Header Structure

```rust
struct GeneralDevicePciHeader {
    vendor_id: u16,
    device_id: u16,
    command_register: u16,
    status_register: u16,
    revision_id: u8,
    programming_interface_byte: u8,
    subclass: u8,
    class_code: u8,
    cache_line_size: u8,
    latency_timer: u8,
    header_type: u8,
    built_in_self_test: u8,
    bars: [u32; 6],
    cardbus_cis_pointer: u32,
    subsystem_vendor_id: u16,
    subsystem_id: u16,
    expansion_rom_base_address: u32,
    capabilities_pointer: u8,
}
```

### PCIDevice

```rust
pub struct PCIDevice {
    header: MMIO<GeneralDevicePciHeader>,
    bars: BTreeMap<u8, PCIAllocatedSpace>,
}

impl PCIDevice {
    pub fn capabilities(&self) -> PciCapabilityIter
    pub fn get_or_initialize_bar(&mut self, index: u8) -> &PCIAllocatedSpace
}
```

### PCI Constants

```rust
const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
const VIRTIO_DEVICE_ID: RangeInclusive<u16> = 0x1000..=0x107F;
const VIRTIO_NETWORK_SUBSYSTEM_ID: u16 = 1;
```

### PCI Allocator

**File:** `kernel/src/pci/allocator.rs`

Allocates 64-bit memory space for BAR configuration:

```rust
pub static PCI_ALLOCATOR_64_BIT: Spinlock<PCIAllocator> = Spinlock::new(PCIAllocator::new());

impl PCIAllocator {
    pub fn init(&mut self, range: &PCIRange)
    pub fn allocate(&mut self, size: usize) -> Option<PCIAllocatedSpace>
}
```

### Device Tree Parser

**File:** `kernel/src/pci/devic_tree_parser.rs`

Parses PCI information from device tree:

```rust
pub fn parse() -> Result<PCIInformation, PCIError>

pub struct PCIInformation {
    pub pci_host_bridge_address: PciCpuAddr,
    pub pci_host_bridge_length: usize,
    pub ranges: Vec<PCIRange>,
}
```

## VirtIO Framework

**File:** `kernel/src/drivers/virtio/`

### VirtIO Network Device

**File:** `kernel/src/drivers/virtio/net/mod.rs`

```rust
pub struct NetworkDevice {
    device: PCIDevice,
    common_cfg: MMIO<virtio_pci_common_cfg>,
    net_cfg: MMIO<virtio_net_config>,
    notify_cfg: MMIO<virtio_pci_notify_cap>,
    transmit_queue: VirtQueue<EXPECTED_QUEUE_SIZE>,
    receive_queue: VirtQueue<EXPECTED_QUEUE_SIZE>,
    mac_address: MacAddress,
}

impl NetworkDevice {
    pub fn initialize(pci_device: PCIDevice) -> Result<Self, &'static str>
    pub fn send_packet(&mut self, packet: Vec<u8>) -> Result<(), QueueError>
    pub fn receive_packets(&mut self) -> Vec<Vec<u8>>
    pub fn get_mac_address(&self) -> MacAddress
}
```

### VirtIO Initialization

```rust
pub fn initialize(mut pci_device: PCIDevice) -> Result<Self, &'static str> {
    // 1. Find VirtIO capabilities
    let common_cfg = find_capability(VIRTIO_PCI_CAP_COMMON_CFG)?;
    let net_cfg = find_capability(VIRTIO_PCI_CAP_DEVICE_CFG)?;
    let notify_cfg = find_capability(VIRTIO_PCI_CAP_NOTIFY_CFG)?;

    // 2. Reset device
    common_cfg.device_status().write(0x0);

    // 3. Set ACKNOWLEDGE status
    device_status |= DEVICE_STATUS_ACKNOWLEDGE;

    // 4. Set DRIVER status
    device_status |= DEVICE_STATUS_DRIVER;

    // 5. Negotiate features
    let features = VIRTIO_NET_F_MAC | VIRTIO_F_VERSION_1;
    common_cfg.driver_feature().write(features);

    // 6. Set FEATURES_OK
    device_status |= DEVICE_STATUS_FEATURES_OK;

    // 7. Set up virtqueues
    let receive_queue = VirtQueue::new(...);
    let transmit_queue = VirtQueue::new(...);

    // 8. Set DRIVER_OK
    device_status |= DEVICE_STATUS_DRIVER_OK;
}
```

### VirtQueue

**File:** `kernel/src/drivers/virtio/virtqueue.rs`

Ring buffer for device communication:

```rust
pub struct VirtQueue<const SIZE: usize> {
    descriptor_area: Box<[virtq_desc; SIZE]>,
    driver_area: Box<virtq_avail<SIZE>>,
    device_area: Box<virtq_used<SIZE>>,
    // ...
}

impl<const SIZE: usize> VirtQueue<SIZE> {
    pub fn new(queue_size: u16, queue_index: u16) -> Self
    pub fn put_buffer(&mut self, buffer: Vec<u8>, direction: BufferDirection) -> Result<u16, QueueError>
    pub fn put_buffer_chain(&mut self, buffers: Vec<(Vec<u8>, BufferDirection)>) -> Result<u16, QueueError>
    pub fn receive_buffer(&mut self) -> Vec<UsedBuffer>  // UsedBuffer has Vec<Vec<u8>> for chains
}
```

### VirtIO Block Device

**File:** `kernel/src/drivers/virtio/block.rs`

Polling-based block device driver using 3-descriptor chains (header, data, status):

```rust
pub struct BlockDevice {
    device: PCIDevice,
    common_cfg: MMIO<virtio_pci_common_cfg>,
    blk_cfg: MMIO<virtio_blk_config>,
    request_queue: VirtQueue<EXPECTED_QUEUE_SIZE>,
    capacity_sectors: u64,
}
```

- Subsystem ID: 2
- Single virtqueue (index 0)
- Read/write via `read_sectors()`/`write_sectors()` with spin-wait completion
- Global `BLOCK_DEVICES: Spinlock<Vec<BlockDevice>>` with indexed `read(index, ...)`/`write(index, ...)` byte-level API
- `assign_block_device()` returns the device index; each device is dynamically registered in devfs as `/dev/vda`, `/dev/vdb`, etc.
- Devfs uses `DevfsDir` with `Spinlock<BTreeMap>` entries for dynamic registration via `devfs::register_block_device(index)`
- Only appears in `/dev` when a block device is actually attached (no unconditional entries)
- QEMU: `--block disk.img` flag in `qemu_wrapper.sh` (auto-creates 1MB image if missing)

### VirtIO Capabilities

**File:** `kernel/src/drivers/virtio/capability.rs`

Shared VirtIO constants and MMIO structs used by both net and block drivers:

```rust
pub const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
pub const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
pub const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
pub const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
pub const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;
// Also: device status constants, VIRTIO_F_VERSION_1,
// virtio_pci_common_cfg, virtio_pci_notify_cap
```

## MMIO Utilities

Core type in `sys/src/klibc/mmio.rs`, re-exported via `kernel/src/klibc/mmio.rs`.

Memory-mapped I/O helper:

```rust
pub struct MMIO<T>(*mut T);

impl<T> MMIO<T> {
    pub fn new(addr: usize) -> Self
    pub fn read(&self) -> T
    pub fn write(&self, value: T)
}
```

### mmio_struct! Macro

**File:** `kernel/src/klibc/mmio.rs`

Generates an extension trait with MMIO accessors for struct fields:

```rust
mmio_struct! {
    struct GeneralDevicePciHeader {
        vendor_id: u16,
        device_id: u16,
        // ...
    }
}

// Generates trait GeneralDevicePciHeaderFields:
impl GeneralDevicePciHeaderFields for MMIO<GeneralDevicePciHeader> {
    fn vendor_id(&self) -> MMIO<u16>
    fn device_id(&self) -> MMIO<u16>
}
```

## Consolidated Driver Initialization

**File:** `kernel/src/drivers/mod.rs`

All PCI device initialization is consolidated in a single function:

```rust
pub fn init_all_pci_devices(pci_devices: Vec<PCIDevice>) {
    init_network_device(&mut pci_devices);
    init_block_devices(&mut pci_devices);
    init_display_device(&mut pci_devices);
    init_rng_device(&mut pci_devices);
    init_input_device(&mut pci_devices);
}
```

Called from `kernel_init()` after PCI enumeration. Each init function finds the relevant device by subsystem ID, initializes it, and registers it (interrupt handlers, devfs entries, etc.).

## Key Files

| File | Purpose |
|------|---------|
| kernel/src/drivers/mod.rs | Consolidated driver init |
| kernel/src/pci/mod.rs | PCI enumeration |
| kernel/src/pci/allocator.rs | BAR space allocation |
| kernel/src/pci/devic_tree_parser.rs | Device tree PCI info |
| kernel/src/pci/lookup.rs | Device ID lookup |
| kernel/src/drivers/virtio/mod.rs | VirtIO module |
| kernel/src/drivers/virtio/net/mod.rs | VirtIO network driver |
| kernel/src/drivers/virtio/block.rs | VirtIO block driver |
| kernel/src/drivers/virtio/virtqueue.rs | VirtQueue implementation (with descriptor chaining) |
| kernel/src/drivers/virtio/capability.rs | Shared VirtIO constants and MMIO structs |
| sys/src/klibc/mmio.rs | Core MMIO type |
| kernel/src/klibc/mmio.rs | mmio_struct! macro, re-exports from sys |

## Adding a New VirtIO Driver

1. Find device during PCI enumeration by subsystem ID
2. Parse VirtIO capabilities from PCI config space
3. Initialize common config (reset, acknowledge, negotiate features)
4. Set up virtqueues for communication
5. Set DRIVER_OK status
6. Implement send/receive via virtqueues
