# Driver Framework Plan

## Current State

For current driver details, see `doc/ai/DRIVERS.md`. Key gaps relevant to this plan: no driver model, no bus abstraction, no sysfs, no dynamic device registration. Drivers are initialized by pattern-matching PCI devices in `kernel_init()` with hardcoded PLIC dispatch. Each VirtIO driver duplicates the initialization sequence.

---

## 1. Linux Driver Model Overview

### Core Abstractions

Linux's driver model is built on three runtime concepts:

**kobject** -- The fundamental reference-counted object. Every "thing" in the kernel that is visible in sysfs is backed by a kobject. A kobject has a name, a parent pointer (forming a hierarchy), a ktype (operations), and a kset (collection). kobjects handle reference counting, sysfs representation, and hotplug event generation.

**ktype** -- Defines the behavior of a class of kobjects: the sysfs attributes they expose and the release function called when the reference count hits zero.

**kset** -- A collection of kobjects of the same type. ksets provide grouping and can filter/generate uevents when kobjects are added or removed.

### The Bus/Device/Driver Triangle

Linux models hardware with three interrelated structures:

**bus_type** -- Represents a bus (PCI, USB, platform, virtio, etc.). A bus defines how devices are matched to drivers. It has a `match` function that compares device identity against driver capability, and a `probe` function that binds a matched driver to a device.

**device** -- Represents a hardware device or logical device. Every device belongs to exactly one bus. Devices form a tree (a USB device is a child of a USB hub which is a child of a PCI device). The device structure holds the bus-specific identity (PCI vendor/device ID, USB product/device ID, device tree compatible string).

**device_driver** -- Represents a piece of code that can drive a device. A driver registers with a bus and declares what devices it can handle (via an id_table). When the bus discovers a new device, it calls `match` against all registered drivers. When a match succeeds, the bus calls `probe` to bind them.

This triangle is what makes "write a driver once and have it work" possible. A driver does not scan for hardware -- it declares what it supports, and the bus infrastructure calls it when matching hardware appears. Hotplug is a natural consequence: when a device appears or disappears, the bus re-runs matching.

### sysfs

sysfs is a virtual filesystem (mounted at `/sys`) that exports the kernel's device model to userspace. Each kobject becomes a directory. Attributes become files. The hierarchy mirrors the device tree:

```
/sys/
  bus/
    pci/
      devices/        -> symlinks to /sys/devices/...
      drivers/
        virtio-pci/
    virtio/
      devices/
      drivers/
        virtio_net/
        virtio_blk/
  devices/
    pci0000:00/
      0000:00:01.0/   -> a PCI device
        virtio0/       -> the virtio device behind it
  class/
    net/
      eth0 -> /sys/devices/.../virtio0/net/eth0
    block/
      vda  -> /sys/devices/.../virtio1/block/vda
```

### Device Classes

Classes group devices by function rather than by bus. `/sys/class/net/` contains all network interfaces regardless of whether they are virtio, e1000, or loopback. `/sys/class/block/` contains all block devices. Classes create the symlinks that make `/dev` population straightforward.

### udev

udev is the userspace daemon that listens for kernel uevents (generated when kobjects are added/removed) and creates device nodes in `/dev`. It uses rules to set permissions, create symlinks, and run helper programs. For Linux binary compatibility, Solaya does not need to run a real udev -- it needs to (a) generate uevents so userspace programs that rely on them work, and (b) populate `/dev` with the right nodes. Since Solaya already has an in-kernel devfs, this is easier than the full udev model.

---

## 2. Reusing Linux Drivers

### 2.1 Rust-for-Linux Approach (Bindgen Wrappers)

The upstream Rust-for-Linux (RfL) project provides safe Rust abstractions over Linux kernel C APIs. Key aspects:

- **Architecture**: Thin C wrapper functions (in `rust/helpers/`) provide stable ABI entry points. Rust code calls these via `extern "C"`. Higher-level safe abstractions are built on top in `rust/kernel/`.
- **Key abstractions**: `device::Device` wraps `struct device`, `driver::Registration` handles driver lifecycle, `pci::Device` wraps PCI-specific operations.
- **Applicability to Solaya**: The RfL abstractions assume Linux internals (kmalloc, spinlock_t, struct page, etc.) that Solaya does not have. Solaya cannot use RfL crates directly. However, the *design patterns* are highly relevant -- particularly how they use Rust's type system to enforce driver lifecycle rules (e.g., a `Registration` that calls `driver_unregister` on `Drop`).

**Recommendation**: Study RfL's abstraction patterns but do not depend on RfL code. Build Solaya's own trait-based driver model inspired by it.

### 2.2 Binary Compatibility (Loading .ko Modules)

Loading compiled Linux kernel modules (.ko files) into Solaya would require:

1. **ELF relocatable object loading** -- .ko files are ELF relocatable objects with sections, relocations, and symbol references.
2. **Symbol resolution** -- Every .ko references kernel symbols (printk, kmalloc, register_netdev, etc.). Solaya would need to provide compatible implementations of *every referenced symbol*.
3. **ABI compatibility** -- Linux kernel structs change between versions. A .ko compiled for Linux 6.8 expects `struct net_device` to have fields at specific offsets. Solaya would need to match these layouts exactly.
4. **Internal API surface** -- Linux's internal API is enormous and unstable. The number of functions a typical driver calls is in the hundreds.

**Verdict**: Not feasible. The ABI surface is too large and too unstable. Even within Linux, modules must be compiled for the exact kernel version they target (MODVERSIONS helps but does not solve the fundamental problem).

### 2.3 Source Compatibility (Compile Linux C Drivers Against Solaya)

This is more viable but still significant work. The approach:

1. **Provide Linux-compatible headers** -- Create a set of header files that define the types and function signatures Linux drivers expect: `struct device`, `struct pci_dev`, `struct net_device`, `pci_read_config_word()`, `netif_rx()`, `kmalloc()`, etc.
2. **Implement a shim layer** -- Write C (or `extern "C"` Rust) implementations of these functions that bridge to Solaya's kernel internals.
3. **Compile the driver** -- Use the same toolchain to compile the Linux C driver source against Solaya's shim headers, producing an object that links into Solaya.

The shim would look like (pseudocode):

```c
// solaya_linux_compat.h
struct pci_dev {
    void *solaya_handle;  // Opaque pointer to Solaya's PCIDevice
    uint16_t vendor;
    uint16_t device;
    // ... subset of fields drivers actually use
};

// solaya_linux_compat.c (or extern "C" Rust)
int pci_read_config_word(struct pci_dev *dev, int offset, u16 *val) {
    // Bridge to Solaya's MMIO<GeneralDevicePciHeader>
    solaya_pci_read_config(dev->solaya_handle, offset, val);
    return 0;
}

void *kmalloc(size_t size, gfp_t flags) {
    return solaya_heap_alloc(size);
}
```

**Challenges**:
- Each driver uses a different subset of the Linux API. There is no small core that covers everything.
- Linux headers include transitive dependencies (one header pulls in 50 others).
- Drivers rely on Linux-specific patterns (workqueues, softirqs, RCU, per-CPU variables) that have no Solaya equivalent.

**Recommendation**: Do not pursue general C driver compatibility. Instead, focus on writing native Rust drivers. For the specific case where a complex driver is needed quickly (e.g., a filesystem), consider porting the C code to Rust directly rather than building a compatibility layer.

### 2.4 VirtIO Focus

VirtIO is the sweet spot for Solaya:

- **Standardized specification**: The VirtIO spec (maintained by OASIS) defines device behavior independently of any OS. Solaya's drivers are written against the spec, not against Linux code.
- **Simple device model**: Every VirtIO device follows the same pattern: PCI capability discovery, feature negotiation, virtqueue setup, buffer exchange. Solaya already implements this pattern in four drivers.
- **QEMU is the primary target**: QEMU implements VirtIO devices faithfully. All the devices Solaya needs (net, block, console, gpu, fs, rng, input) have VirtIO implementations in QEMU.
- **No need for Linux code reuse**: VirtIO drivers are typically 500-2000 lines. Writing them natively in Rust is less work than building a C compatibility layer.

The VirtIO device IDs (PCI subsystem IDs) are:

| Subsystem ID | Device Type |
|-------------|-------------|
| 1 | Network |
| 2 | Block |
| 3 | Console |
| 4 | Entropy (RNG) |
| 5 | Memory balloon |
| 9 | 9P transport (filesystem sharing) |
| 16 | GPU |
| 18 | Input |
| 26 | Filesystem (virtio-fs) |
| 27 | PMEM |

Solaya currently handles 1, 2, 4, and 18.

---

## 3. Recommended Architecture

### 3.1 Design Goals

- Model Linux's bus/device/driver triangle so that sysfs paths look correct to Linux userspace
- Use Rust traits instead of function pointers (safer, more ergonomic)
- Use Rust ownership for DMA buffers (no use-after-free, no forgotten frees)
- Centralize VirtIO initialization boilerplate
- Support dynamic device registration (hotplug-ready, even if not used initially)
- Keep the driver model minimal -- no unnecessary abstractions

### 3.2 Core Traits

```rust
// kernel/src/drivers/model.rs

/// Uniquely identifies a device on a bus.
pub trait DeviceId: Send + Sync + core::fmt::Debug {
    fn bus_name(&self) -> &str;
    fn device_path(&self) -> String;  // e.g., "0000:00:01.0"
}

/// A device discovered on a bus.
pub trait Device: Send + Sync {
    fn id(&self) -> &dyn DeviceId;
    fn name(&self) -> &str;
    fn parent(&self) -> Option<&dyn Device>;

    /// The device class (net, block, input, etc.) -- set after probe.
    fn device_class(&self) -> Option<DeviceClass>;
}

/// Describes what devices a driver can handle.
pub trait Driver: Send + Sync {
    fn name(&self) -> &str;
    fn bus_name(&self) -> &str;

    /// Check if this driver can handle the given device.
    fn matches(&self, device: &dyn Device) -> bool;

    /// Bind to the device. Called by the bus when a match succeeds.
    fn probe(&self, device: &mut dyn Device) -> Result<(), DriverError>;

    /// Unbind from the device. Called on removal or driver unload.
    fn remove(&self, device: &mut dyn Device);
}

/// Represents a bus that connects devices and drivers.
pub trait Bus: Send + Sync {
    fn name(&self) -> &str;

    /// Scan for devices on this bus.
    fn scan(&mut self) -> Vec<Box<dyn Device>>;

    /// Register a driver with this bus.
    fn register_driver(&mut self, driver: Arc<dyn Driver>);

    /// Try to match all unbound devices against all registered drivers.
    fn match_and_probe(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Net,
    Block,
    Input,
    Display,
    Random,
    Console,
    Filesystem,
}
```

### 3.3 PCI Bus Implementation

The PCI bus is the first concrete bus implementation:

```rust
// kernel/src/drivers/pci_bus.rs

pub struct PciDeviceId {
    pub domain: u16,     // always 0 for now
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: u16,
    pub subsystem_id: u16,
    pub class_code: u8,
    pub subclass: u8,
}

pub struct PciDevice {
    pub id: PciDeviceId,
    pub pci_device: PCIDevice,  // existing type from kernel/src/pci/
    pub plic_irq: u32,
    class: Option<DeviceClass>,
}

pub struct PciBus {
    devices: Vec<PciDevice>,
    drivers: Vec<Arc<dyn Driver>>,
    pci_information: PCIInformation,
}

impl PciBus {
    pub fn new(pci_information: PCIInformation) -> Self { ... }
}

impl Bus for PciBus {
    fn scan(&mut self) -> Vec<Box<dyn Device>> {
        // Uses existing enumerate_devices() logic
    }

    fn match_and_probe(&mut self) {
        // For each unbound device, try each driver's matches()
        // On match, call driver.probe()
    }
}
```

### 3.4 VirtIO Bus Layer

In Linux, VirtIO has its own bus (`virtio_bus`) sitting on top of PCI. A `virtio-pci` driver registers with the PCI bus and, when probed, creates a VirtIO device on the VirtIO bus. Individual VirtIO drivers (virtio_net, virtio_blk) register with the VirtIO bus.

Solaya should follow this layered model:

```rust
// kernel/src/drivers/virtio/bus.rs

pub struct VirtioDeviceId {
    pub device_type: u16,   // subsystem ID
    pub vendor_id: u32,     // always VIRTIO_VENDOR_ID for now
}

/// A VirtIO device after PCI-level initialization (capabilities parsed,
/// BARs allocated) but before device-specific initialization.
pub struct VirtioDevice {
    pub id: VirtioDeviceId,
    pub pci_device: PCIDevice,
    pub common_cfg: MMIO<virtio_pci_common_cfg>,
    pub isr_status: MMIO<u32>,
    pub notify_bar: PCIAllocatedSpace,
    pub notify_cfg: MMIO<virtio_pci_notify_cap>,
    pub device_cfg_bar: Option<PCIAllocatedSpace>,
    pub device_cfg_offset: u32,
    pub plic_irq: u32,
}

impl VirtioDevice {
    /// Perform the common VirtIO initialization sequence:
    /// reset, acknowledge, driver, negotiate features, features_ok.
    /// Returns a VirtioDevice ready for device-specific setup.
    pub fn from_pci(
        pci_device: PCIDevice,
        wanted_features: u64,
    ) -> Result<Self, &'static str> {
        // This consolidates the boilerplate currently duplicated
        // across net, block, rng, and input drivers.
    }

    /// Set up a virtqueue at the given index.
    pub fn setup_queue<const SIZE: usize>(
        &mut self,
        index: u16,
    ) -> VirtQueue<SIZE> {
        // Common queue setup logic
    }

    /// Mark the device as DRIVER_OK and enable bus mastering.
    pub fn activate(&mut self) {
        // Set DRIVER_OK, enable bus master, clear interrupt disable
    }
}
```

This eliminates the 60+ lines of duplicated initialization code in each current VirtIO driver.

### 3.5 DMA Buffer Ownership

Currently, VirtIO drivers pass `Vec<u8>` through the virtqueue. The `DeconstructedVec` tracks ownership, but there are no type-level guarantees about DMA safety. A proper DMA buffer type would:

```rust
/// A buffer allocated in physical memory suitable for DMA.
/// The physical address is valid for device access.
/// Drop frees the underlying pages.
pub struct DmaBuffer {
    virt: *mut u8,
    phys: PhysAddr,
    len: usize,
}

impl DmaBuffer {
    pub fn new(len: usize) -> Self { ... }
    pub fn phys_addr(&self) -> PhysAddr { self.phys }
    pub fn as_slice(&self) -> &[u8] { ... }
    pub fn as_mut_slice(&mut self) -> &mut [u8] { ... }
}

// DmaBuffer is Send but not Clone -- ownership transfer through the virtqueue
// prevents use-after-free. When the buffer goes through the virtqueue:
//   1. Driver creates DmaBuffer, writes data
//   2. Driver submits buffer to virtqueue (moves ownership)
//   3. Device processes buffer
//   4. Driver reclaims buffer from used ring (gets ownership back)
//   5. Driver reads data, then drops buffer
```

Note: Currently Solaya uses identity-mapped physical memory (virt == phys for kernel addresses), so `Vec<u8>` works as a DMA buffer by accident. The `DmaBuffer` type makes this assumption explicit and will be necessary when Solaya supports non-identity-mapped configurations or IOMMU.

### 3.6 Interrupt Dispatch

The current PLIC handler has hardcoded match arms for each device type. This should be replaced with a registration table:

```rust
// kernel/src/interrupts/plic.rs

type IrqHandler = fn(u32); // IRQ number

static IRQ_HANDLERS: Spinlock<BTreeMap<u32, IrqHandler>> = ...;

pub fn register_irq(irq: u32, priority: u32, handler: IrqHandler) {
    let mut plic = PLIC.lock();
    plic.enable(irq);
    plic.set_priority(irq, priority);
    IRQ_HANDLERS.lock().insert(irq, handler);
}

// In handle_external_interrupt:
fn dispatch_plic_interrupt() {
    let irq = plic.claim();
    if let Some(handler) = IRQ_HANDLERS.lock().get(&irq) {
        handler(irq);
    }
    plic.complete(irq);
}
```

This makes PLIC dispatch O(1) lookup instead of a chain of `if` matches, and allows drivers to register their own IRQ handlers without modifying PLIC code.

### 3.7 sysfs-Compatible Interface

Solaya should expose a `/sys` filesystem with the standard Linux layout. This does not need to be backed by kobjects -- it can be built on the existing VFS infrastructure:

```rust
// kernel/src/fs/sysfs.rs

/// A sysfs directory node. Children can be added dynamically.
struct SysfsDir { ... }

/// A sysfs attribute file. Read returns the attribute value.
struct SysfsAttr {
    read_fn: fn() -> String,
    write_fn: Option<fn(&str) -> Result<(), Errno>>,
}
```

The minimal sysfs tree needed for Linux userspace compatibility:

```
/sys/
  class/
    net/
      eth0/
        address         -> MAC address
        mtu             -> MTU
        operstate       -> "up"
    block/
      vda/
        size            -> device size in 512-byte sectors
    input/
      event0/
    tty/
      console/
  bus/
    pci/
      devices/
        0000:00:01.0/
          vendor        -> 0x1af4
          device        -> 0x1000
          class         -> 0x020000
    virtio/
      devices/
        virtio0/
          device        -> device type number
  devices/
    platform/
    pci0000:00/
      0000:00:01.0/
```

Programs like `ip`, `lsblk`, and `systemd` read sysfs to discover devices. Getting the structure right is more important than having every attribute -- most programs check a small number of paths.

### 3.8 C Driver FFI Shim (Future)

If a C driver is ever needed, the FFI layer would look like:

```rust
// kernel/src/drivers/ffi.rs

/// C-compatible device handle.
#[repr(C)]
pub struct CDevice {
    opaque: *mut (),
    bus_type: u32,
}

/// Functions exported to C drivers via extern "C".
#[unsafe(no_mangle)]
pub extern "C" fn solaya_pci_read_config_word(
    dev: *const CDevice,
    offset: u32,
    val: *mut u16,
) -> i32 {
    // Bridge to Rust PCI code
}

#[unsafe(no_mangle)]
pub extern "C" fn solaya_register_netdev(dev: *const CDevice) -> i32 {
    // Register with Solaya's network stack
}
```

This is explicitly a future consideration, not a near-term priority. The VirtIO drivers Solaya needs are small enough (500-2000 lines each) that writing them in Rust is faster than building a C compatibility layer.

---

## 4. Priority Drivers for QEMU

### 4.1 Must-Have for Running Real Linux Userspace

These are needed to boot a Linux userspace with a shell, coreutils, and basic networking:

| Driver | Status | Priority | Notes |
|--------|--------|----------|-------|
| virtio-net | Done | -- | Working, interrupt-driven RX, polling TX |
| virtio-blk | Done | -- | Working, async interrupt-driven, ext2 mount |
| virtio-rng | Done | -- | Working, used for `/dev/random` |
| virtio-console | Not started | High | Many programs expect `/dev/console` or `/dev/ttyS0` to behave like a proper terminal. The current UART-based console works but virtio-console would provide a cleaner abstraction and multiport support. |
| virtio-gpu | Not started | Medium | Required for graphical output beyond the current Bochs VBE hack. Programs like `weston`, `Xorg` need DRM/KMS, but for framebuffer-only (`/dev/fb0`), the current Bochs display works. |
| virtio-fs | Not started | High | Sharing a host directory into the guest. This is the fastest way to run real Linux binaries without building a full disk image. Uses FUSE protocol over VirtIO. |
| virtio-input | Done | -- | Working, keyboard events buffered |
| virtio-9p | Not started | Medium | Alternative to virtio-fs for host directory sharing. Simpler protocol (9P2000.L), well-documented, and QEMU support is mature. |
| Platform RTC | Not started | Medium | `clock_gettime()` currently uses RISC-V `rdtime`. A proper RTC gives wall-clock time. QEMU provides `goldfish-rtc` on RISC-V. |
| Platform serial (ns16550) | Done (as UART) | -- | Working via `io/uart.rs`, hardcoded at 0x10000000 |

### 4.2 Nice-to-Have

| Driver | Priority | Notes |
|--------|----------|-------|
| virtio-balloon | Low | Memory management optimization |
| virtio-vsock | Medium | Host-guest communication without networking |
| virtio-sound | Low | Audio |
| virtio-crypto | Low | Hardware crypto acceleration |

### 4.3 What Programs Expect

Real Linux userspace programs assume:

- `/dev/null`, `/dev/zero` -- **Done** (devfs)
- `/dev/random`, `/dev/urandom` -- **Done** (virtio-rng)
- `/dev/console`, `/dev/tty` -- Partially done (UART-based DevConsole)
- `/dev/fb0` -- **Done** (Bochs display)
- `/dev/vda` etc. -- **Done** (virtio-blk)
- `/proc/` -- Partially done (procfs exists but limited)
- `/sys/` -- **Not done** (no sysfs)
- `/dev/pts/*` -- **Not done** (no PTY support, needed for SSH/screen)
- `/tmp` -- **Done** (tmpfs)

### 4.4 x86-Specific Devices (When Adding x86 Support)

When Solaya adds x86_64 as a second architecture, QEMU provides both VirtIO and legacy devices:

| Device | Type | Notes |
|--------|------|-------|
| i8259 PIC | Legacy | Replaceable by APIC, but some legacy paths assume it |
| Local APIC | Platform | Per-CPU interrupt controller (replaces RISC-V PLIC) |
| I/O APIC | Platform | Routes external interrupts (replaces PLIC for device IRQs) |
| HPET | Platform | High-precision timer (replaces RISC-V `rdtime`) |
| i8042 | Legacy | PS/2 keyboard/mouse (virtio-input is preferred) |
| CMOS RTC | Legacy | Real-time clock at I/O port 0x70/0x71 |
| VGA/VESA | Legacy | Bochs VBE works here too (same PCI device) |
| PCI host bridge | Platform | Different from RISC-V (ECAM via ACPI, not device tree) |

VirtIO devices are the same on x86 -- that is the entire point of VirtIO. The architecture-specific part is interrupt routing (APIC vs PLIC) and device discovery (ACPI vs device tree).

---

## 5. x86 Support Considerations

### 5.1 What Changes in the Driver Model

The driver model itself (bus/device/driver traits, sysfs, devfs) is architecture-independent. What changes:

**Interrupt controller**: RISC-V uses PLIC (memory-mapped, flat interrupt space). x86 uses APIC (MSR-based local APIC + memory-mapped I/O APIC, with redirect entries). The abstraction needed:

```rust
// kernel/src/interrupts/mod.rs

pub trait InterruptController: Send + Sync {
    fn enable_irq(&mut self, irq: u32, cpu: CpuId);
    fn disable_irq(&mut self, irq: u32);
    fn set_priority(&mut self, irq: u32, priority: u32);
    fn claim(&mut self) -> Option<u32>;
    fn complete(&mut self, irq: u32);
}
```

PLIC and APIC both implement this trait. Drivers call `register_irq()` without knowing which controller is underneath.

**Device discovery**: RISC-V uses a device tree (FDT) to describe platform devices and PCI host bridge location. x86 uses ACPI tables (RSDT/XSDT -> MCFG for PCI ECAM base, MADT for APIC topology, DSDT/SSDT for device descriptions). The PCI enumeration code itself is the same -- it just needs a different way to find the ECAM base address:

```rust
// kernel/src/pci/discovery.rs

pub trait PciDiscovery {
    fn ecam_base(&self) -> PciCpuAddr;
    fn ecam_size(&self) -> usize;
    fn pci_ranges(&self) -> Vec<PCIRange>;
}

struct DeviceTreePciDiscovery { ... }  // existing code
struct AcpiPciDiscovery { ... }        // future
```

**Timer**: RISC-V uses SBI timer (`sbi_set_timer`) or the CLINT. x86 uses APIC timer, HPET, or PIT. The arch crate already abstracts this (`arch::timer`).

**Serial console**: RISC-V QEMU uses ns16550 at 0x10000000 (from device tree). x86 QEMU uses ns16550 at I/O port 0x3f8. Same driver, different bus (memory-mapped vs. I/O port). The ns16550 driver should be parameterized by address and access mode.

### 5.2 ACPI vs Device Tree

ACPI is significantly more complex than device tree:

- Device tree is a static data structure (read-once at boot). ACPI includes AML bytecode that must be interpreted at runtime.
- For QEMU targets, the ACPI tables are simple and predictable. A minimal ACPI parser that handles RSDP -> RSDT -> MCFG (PCI), MADT (APIC), and FADT (power management) is sufficient.
- Libraries like `acpi` (Rust crate) can parse ACPI tables. This is one area where using an existing crate makes sense, since ACPI parsing is pure computation with no OS dependencies.

### 5.3 Strategy for Multi-Architecture Support

The existing `arch` crate pattern (cfg-gated modules with a common API surface) extends naturally:

```
arch/src/
  lib.rs              # CpuId, shared types
  riscv64/            # existing
  x86_64/             # future
    cpu.rs            # CR/MSR access, IDT setup
    apic.rs           # APIC driver
    mod.rs
  stub/               # existing (for miri/tests)
```

The key principle: **driver code must not contain `#[cfg(target_arch)]`**. Architecture differences should be hidden behind the `arch` crate and the interrupt controller trait. A VirtIO net driver should compile identically on RISC-V and x86.

---

## 6. What Other Rust Kernels Do

### Redox OS

Redox uses a microkernel architecture where drivers run as userspace processes communicating via schemes (a URL-based IPC mechanism). There is no in-kernel driver model. Drivers open a scheme, read/write to it, and the kernel routes requests. This is elegant but fundamentally different from Linux's model. Not directly applicable to Solaya's goal of Linux binary compatibility.

### Theseus OS

Theseus treats each driver as a separately compiled crate loaded at runtime. The driver model is based on Rust traits with runtime dispatch. Each device type has a trait (e.g., `NetworkDevice`, `StorageDevice`), and drivers implement these traits. Device discovery is done via PCI enumeration similar to Solaya's current approach. Theseus's approach of per-crate isolation is interesting but not necessary for Solaya.

### Maestro (Rust kernel targeting Linux compatibility)

Maestro implements a simplified Linux driver model with kobject-like types and a sysfs-like virtual filesystem. It focuses on VirtIO drivers and has a bus abstraction. Its approach is closest to what Solaya needs.

### gVisor and Firecracker

Both are VM-oriented and use VirtIO:

**gVisor** (Google's application kernel): Implements VirtIO-net and VirtIO-vsock in Go. Uses a "netstack" that reimplements TCP/IP. No driver model -- devices are configured at startup. The main lesson: for a QEMU-focused kernel, VirtIO is sufficient; you do not need legacy device drivers.

**Firecracker** (AWS's microVM): Runs a minimal Linux kernel with only VirtIO drivers enabled. It stripped Linux down to the bare minimum -- proving that VirtIO alone is enough for a production workload in a VM context.

**Key takeaway**: Both projects validate the strategy of supporting only VirtIO for a QEMU-targeted kernel. Real hardware support (and thus real driver complexity) can be deferred.

---

## 7. Phased Implementation

### Phase 1: Refactor VirtIO Initialization (Near-Term)

**Goal**: Eliminate boilerplate duplication across VirtIO drivers.

1. Create `VirtioDevice::from_pci()` that performs the common initialization sequence (reset, acknowledge, driver, negotiate features, features_ok, parse capabilities, allocate BARs).
2. Move common queue setup into `VirtioDevice::setup_queue()`.
3. Move bus master enable / interrupt disable clear into `VirtioDevice::activate()`.
4. Refactor all four existing drivers (net, block, rng, input) to use the shared `VirtioDevice`.
5. Generalize PLIC interrupt registration: replace hardcoded `InterruptSource` enum with a handler table keyed by IRQ number.

**Size estimate**: ~300 lines of new shared code, ~200 lines removed from each driver.

### Phase 2: Bus/Device/Driver Abstraction (Medium-Term)

**Goal**: Introduce the driver model framework.

1. Define `Bus`, `Device`, `Driver` traits in `kernel/src/drivers/model.rs`.
2. Implement `PciBus` that wraps existing enumeration code.
3. Implement `VirtioPciBus` that sits on top of PCI and creates `VirtioDevice` instances.
4. Convert each VirtIO driver to implement the `Driver` trait with `matches()` and `probe()`.
5. Replace the manual device matching in `kernel_init()` with `bus.scan()` + `bus.match_and_probe()`.

**Size estimate**: ~500 lines for the framework, ~100 lines per driver conversion.

### Phase 3: sysfs (Medium-Term)

**Goal**: Expose device hierarchy to userspace via `/sys`.

1. Implement `SysfsDir` and `SysfsAttr` VFS nodes.
2. Build the `/sys/class/{net,block,input}/` tree from registered devices.
3. Build the `/sys/bus/{pci,virtio}/` tree.
4. Build the `/sys/devices/` tree with proper parent-child relationships.
5. Add uevent generation (write to `/sys/.../uevent` or kobject_uevent netlink).

**Size estimate**: ~600 lines.

### Phase 4: New VirtIO Drivers (Ongoing)

Add drivers in this priority order:

1. **virtio-console** -- Provides proper `/dev/hvc0` terminal. Relatively simple (two virtqueues for RX/TX character data). Needed for programs that open `/dev/console` and expect terminal semantics different from UART.
2. **virtio-fs** or **virtio-9p** -- Host directory sharing. virtio-9p is simpler (well-documented 9P2000.L protocol). virtio-fs uses FUSE protocol and requires DAX window support. Start with 9p.
3. **virtio-gpu** -- For DRM/KMS support. Complex (command ring, 2D/3D resources, scanouts). Defer unless graphical applications are a priority.

### Phase 5: Interrupt Controller Abstraction (When Adding x86)

1. Define `InterruptController` trait.
2. Implement for PLIC (wrap existing code).
3. Implement for x86 APIC.
4. Update driver IRQ registration to use the trait.

### Phase 6: ACPI and x86 Platform (Future)

1. Add ACPI table parsing (use `acpi` crate or write minimal parser).
2. Implement x86 PCI discovery via MCFG table.
3. Implement x86 timer (APIC timer).
4. Implement x86 serial (I/O port access for ns16550).
5. Boot on QEMU x86_64 with the same VirtIO drivers -- they should work unchanged.

---

## 8. Summary of Key Decisions

1. **Write native Rust drivers, do not try to reuse Linux C code.** The compatibility surface is too large and the drivers Solaya needs are small.
2. **Model Linux's bus/device/driver hierarchy** so that sysfs paths and device discovery look correct to Linux userspace programs.
3. **VirtIO is the only device family that matters for QEMU.** Do not implement legacy device drivers until real hardware support is a goal.
4. **Refactor VirtIO initialization first** -- this is the highest-value, lowest-risk change that immediately reduces code duplication.
5. **sysfs is required for Linux userspace compatibility** but can be a simple VFS tree backed by device metadata -- no need for the full kobject/kset/ktype machinery.
6. **Architecture-specific code stays in the `arch` crate.** Drivers must not contain `#[cfg(target_arch)]`.
7. **Defer x86 platform work** until the RISC-V driver model is solid. The driver model itself is architecture-independent; only interrupt routing and device discovery change.
