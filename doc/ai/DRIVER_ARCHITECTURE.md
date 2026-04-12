# Driver Architecture — Design Contract

**Status:** in progress. This document is the contract every sub-agent working
on the driver refactor reads. It is updated as reality diverges from design.

**Target branch:** `challenge-architecture`.

**Plan file:** `/home/sysheap/.claude/plans/vivid-knitting-token.md`.
**Research report:** `/home/sysheap/.claude/plans/vivid-knitting-token-agent-ac0eb5626fc99d932.md`.
**Tracking issue:** sysheap/solaya#250 (item #1).

---

## 1. Goal

Replace the ad-hoc driver layer in `crates/kernel/src/drivers/` with a typed,
trait-based driver API inspired by Linux's subsystem `*_ops`, Solaris DDI's
nexus/leaf split, and Rust-for-Linux's RAII + typestate style — while staying
fully in-kernel (no microkernel IPC), with no ABI stability promise (Linux
model), and preserving `#![forbid(unsafe_code)]` in the kernel crate.

Every current driver migrates to the new model. `kernel/src/drivers/` shrinks
to enumeration glue; concrete drivers live in a new `crates/drivers/` top-level
crate with **no reach-ins** to kernel internals.

---

## 2. Current state (inventory)

Paths below are relative to the repo root.

### Workspace

`Cargo.toml` members: `boot`, `crates/hal`, `crates/abi`, `crates/headers`,
`crates/kernel` (crate name `solaya`), `crates/klib`, `crates/mm`, `userspace`.

**Inconsistency 1 — `crates/console` is not in the workspace.** The crate
exists at `crates/console/` with a valid `Cargo.toml` and is consumed by
`crates/kernel/Cargo.toml` as a path dep. It just isn't listed under
`workspace.members`, which means `cargo --workspace` operations skip it.
Phase 1 fixes this.

**Inconsistency 2 — `crates/kernel/src/klibc/` grab-bag still exists.**
Phase 2.8 of the workspace refactor was supposed to delete it. Today it has
7 files: `big_endian.rs`, `consumable_buffer.rs`, `elf.rs`, `leb128.rs`,
`mod.rs`, `util.rs`, `writable_buffer.rs`. `mod.rs` re-exports from `klib`
and `hal`:

```rust
pub use klib::{array_vec, btreemap, non_empty_vec, runtime_initialized, sizes};
pub mod big_endian; pub mod consumable_buffer; pub mod elf;
pub mod leb128; pub mod util; pub mod writable_buffer;
pub use hal::{mmio::{self, MMIO}, spinlock::{Spinlock, SpinlockGuard}};
```

The re-exports and the local modules are both live. Not part of this
refactor's scope (tracked as part of #250), but don't re-tangle them.

### Drivers today

| Driver | Location | Role |
|---|---|---|
| virtio-blk | `crates/kernel/src/drivers/virtio/block.rs` | Block storage |
| virtio-net | `crates/kernel/src/drivers/virtio/net/mod.rs` | Network |
| virtio-input | `crates/kernel/src/drivers/virtio/input.rs` | Input events |
| virtio-rng | `crates/kernel/src/drivers/virtio/rng.rs` | Entropy source |
| dwmac (+ jh7110 SoC glue) | `crates/kernel/src/drivers/dwmac/` | Network (StarFive HW) |
| bochs display | `crates/kernel/src/drivers/bochs_display.rs` | Framebuffer |
| jh7110 clock/reset | `crates/kernel/src/drivers/jh7110/` | Platform init |
| UART | `crates/kernel/src/io/uart.rs` | Serial console |
| PLIC | `crates/kernel/src/interrupts/plic.rs` | Interrupt controller |

### Initialization path today

`crates/kernel/src/drivers/mod.rs`:

```rust
pub fn init_all_pci_devices(mut pci_devices: Vec<PCIDevice>) {
    init_network_device(&mut pci_devices);
    init_block_devices(&mut pci_devices);
    init_display_device(&mut pci_devices);
    init_rng_device(&mut pci_devices);
    init_input_device(&mut pci_devices);
}
```

Each `init_*` function hard-codes the driver type, calls its static
`is_virtio_*` identifier, calls its `::initialize(device)`, pushes it to a
global, and registers a `fn()` interrupt handler with `plic::register_interrupt`.

`init_block_devices` additionally does
`kernel_tasks::spawn(fs::ext2::mount_ext2(0))` — policy mixed into mechanism.

DWMAC lives in a separate `init_dwmac_devices()` that walks the device tree
and duplicates the same pattern.

### Top coupling offenders

1. **`crates/kernel/src/drivers/mod.rs:9-16`** reaches into
   `crate::{device_tree, fs, interrupts::plic, net, pci, processes::kernel_tasks}`.
2. **`crates/kernel/src/drivers/mod.rs:58`** spawns `fs::ext2::mount_ext2`
   from driver init.
3. **`crates/kernel/src/fs/ext2/mod.rs:8-9`**: `use crate::drivers::virtio::block;`
   — filesystem reaches into a specific driver.
4. **`crates/kernel/src/interrupts/plic.rs:202`**:
   `pub fn register_interrupt(irq: u32, handler: fn())` — no context, no
   unregister, no typed identity.
5. **`crates/kernel/src/drivers/virtio/block.rs:171-211`**: per-driver
   `DevBlock { ino, index }` struct + `VfsNode` impl + `devfs::register_device`
   call, duplicated in each driver.
6. **`crates/kernel/src/drivers/virtio/virtqueue.rs:72`** (approx):
   `self.descriptor_area.as_ptr() as u64` — raw physical-address cast.
7. **`crates/kernel/src/net/mod.rs:17`**: local `pub trait NetworkDevice`
   duplicating what should be a driver-api trait.
8. **`crates/kernel/src/drivers/virtio/block.rs:213-324`**: free functions
   `pub async fn read(index, offset, buf)` / `write` indexed into a global
   `BLOCK_DEVICES` — should be methods on a trait object.

---

## 3. Target architecture

### 3.1 Crate topology

```
        abi      headers
          \      /
           klib                      (hardware-free utilities)
           / \
         hal  util                   (CSR / SBI / MMIO / Spinlock / page_table /
          \  /                        PhysAddr / VirtAddr / validated_ptr)
           mm                         (page_allocator + heap)
           /\
    console  driver-api       NEW    (trait-only crate; no impls)
           \ /  \
            \    drivers       NEW   (virtio/dwmac/bochs/jh7110/uart)
             \   /
             kernel (crate name: solaya)
               |
              boot
```

**New crates:**
- `crates/driver-api/` — trait-only, `no_std`, `#![forbid(unsafe_code)]`.
  Depends on `klib`, `hal`, `mm`, `abi`, `headers`. Never depends on
  `drivers`, `console`, or kernel.
- `crates/drivers/` — all concrete drivers. Depends on `driver-api`, `hal`,
  `mm`, `console` (for logging), `abi`, `headers`. **Never depends on
  `solaya` (the kernel).**

**Workspace invariant:** both new crates are registered in top-level
`Cargo.toml` `workspace.members`. `crates/console` is registered too (fixes
Inconsistency 1).

### 3.2 Core traits (in `driver-api`)

Exact signatures may be refined per phase; this is the starting point.

```rust
// --- error types -----------------------------------------------------------

#[derive(Debug)]
pub enum ProbeError {
    DoesNotMatch,            // this driver doesn't handle this device
    InitializationFailed(&'static str),
}

pub use headers::errno::Errno as IoError;   // use the same errno set as syscalls

#[derive(Debug)]
pub enum BusError {
    NoSuchBar,
    MmioMapFailed,
    OutOfMemory,
    IrqUnavailable,
}

// --- subsystem traits ------------------------------------------------------

pub trait BlockDevice: Send + Sync {
    fn name(&self) -> &str;            // "vda"
    fn num_blocks(&self) -> u64;
    fn block_size(&self) -> usize;     // bytes per block
    fn read<'a>(
        &'a self,
        offset_bytes: u64,
        buf: &'a mut [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, IoError>> + Send + 'a>>;
    fn write<'a>(
        &'a self,
        offset_bytes: u64,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<usize, IoError>> + Send + 'a>>;
}

pub trait NetDevice: Send + Sync {
    fn name(&self) -> &str;
    fn mac(&self) -> MacAddress;
    fn mtu(&self) -> u16;
    fn send(&self, frame: Vec<u8>);
    fn receive(&self) -> Vec<Vec<u8>>;  // batched, matches current driver surface
}

pub trait CharDevice: Send + Sync {
    fn name(&self) -> &str;
    fn read(&self, buf: &mut [u8]) -> Result<usize, IoError>;
    fn write(&self, data: &[u8]) -> Result<usize, IoError>;
}

pub trait DisplayDevice: Send + Sync {
    fn name(&self) -> &str;
    fn framebuffer(&self) -> FramebufferInfo;   // width, height, stride, phys
    fn flush(&self, rect: Rect);                 // optional — default: noop
}

pub trait InputDevice: Send + Sync {
    fn name(&self) -> &str;
    fn poll_event(&self) -> Option<InputEvent>;
}

pub trait RngDevice: Send + Sync {
    fn name(&self) -> &str;
    fn fill(&self, buf: &mut [u8]) -> Result<usize, IoError>;
}
```

**Note on `async fn in trait`:** stable since Rust 1.75 but trait objects
still need `Pin<Box<dyn Future>>`. We use the explicit `Pin<Box<...>>` form
above for object-safety — no magic, reviewable. Trait-internal async helpers
inside each driver can use plain `async fn`.

### 3.3 IRQ model (Phase 4)

```rust
pub trait IrqHandler: Send + Sync {
    /// Short — ack the device, wake the bottom-half task. Do not sleep.
    fn handle(&self);
}

/// RAII guard — dropping it removes the handler from the PLIC.
#[must_use]
pub struct IrqRegistration { /* opaque; holds the slot */ }
```

PLIC's public surface becomes:
```rust
pub fn register(irq: u32, handler: Arc<dyn IrqHandler>) -> IrqRegistration;
```
Bottom-half pattern stays: the `handle()` impl ack-reads the device's ISR
register and `Waker::wake()`s the driver's async task.

### 3.4 DMA model (Phase 5)

```rust
pub struct DmaBuffer { /* page-backed, Drop frees */ }

impl DmaBuffer {
    pub fn new_coherent(len: usize) -> Result<DmaBuffer, BusError>;
    pub fn phys_addr(&self) -> PhysAddr;        // today: virt as u64 (identity map)
    pub fn as_mut_slice(&mut self) -> &mut [u8];
    pub fn as_slice(&self) -> &[u8];
    pub fn sync_for_device(&self);              // no-op on today's target
    pub fn sync_for_cpu(&self);
}
```

Replaces every `Box::as_ptr() as u64` cast in virtio/dwmac. Phys-to-virt
translation is encapsulated so a later IOMMU change is localized.

### 3.5 BusContext (Phase 6)

```rust
pub trait BusContext: Send + Sync {
    fn map_mmio(&self, bar: BarIndex, len: usize) -> Result<MmioRegion, BusError>;
    fn dma_alloc_coherent(&self, len: usize) -> Result<DmaBuffer, BusError>;
    fn register_irq(
        &self,
        id: IrqId,
        handler: Arc<dyn IrqHandler>,
    ) -> Result<IrqRegistration, BusError>;
    fn read_config_u32(&self, offset: u16) -> u32;   // PCI cfg or DT prop
}
```

Two impls: `PciBusContext` (wraps `PCIDevice` + PLIC + page allocator) and
`DtBusContext` (wraps a device-tree node + PLIC + page allocator). Drivers'
`attach` functions take `&dyn BusContext`; they stop importing `pci::*`,
`interrupts::plic::*`, `device_tree::*`.

### 3.6 Registration flow

1. Kernel boot constructs the root buses (PCI root, DT platform root).
2. Kernel constructs a `DriverCatalog` and calls
   `drivers::register_builtin(&mut catalog)`. That function lives in the
   `drivers` crate and adds one entry per driver (an `Arc<dyn DriverFactory>`-ish
   value with a static `probe` method and an `attach` callable).
3. Kernel enumerates devices on each bus; for each discovered device, it
   walks the catalog and calls each factory's `probe`. The first match wins
   (simple — score-ranking can come later if needed).
4. `attach` returns a `DriverInstance` enum variant
   (`Block(Arc<dyn BlockDevice>)`, `Net(...)`, etc.). Kernel inserts it into
   the matching typed registry.
5. After **all** enumeration is done, `kernel/src/init/bring_up_system()`
   (Phase 8) reads the registries and executes *policy*: mount ext2 on vda,
   spawn `network_rx_task` bound to the first NIC, etc.

Catalog registration is **explicit function-call**, not macro/linker-section
magic. Simple, reviewable, compatible with `#![forbid(unsafe_code)]` in
kernel. If/when more drivers arrive, revisit.

### 3.7 Typed registries

One per device class, in `kernel/src/drivers/registry.rs`:

```rust
pub struct BlockDeviceRegistry(Spinlock<Vec<Arc<dyn BlockDevice>>>);
pub struct NetDeviceRegistry(Spinlock<Vec<Arc<dyn NetDevice>>>);
pub struct CharDeviceRegistry(Spinlock<Vec<Arc<dyn CharDevice>>>);
// ... etc
```

Each has `global()`, `register`, `get(index)`, `iter`. Kernel subsystems
(ext2, net, devfs) read from these registries.

### 3.8 Devfs adapters

One generic adapter per trait, in `kernel/src/fs/devfs.rs`:

```rust
struct BlockNode(Arc<dyn BlockDevice>);
struct CharNode(Arc<dyn CharDevice>);
struct DisplayNode(Arc<dyn DisplayDevice>);
struct InputNode(Arc<dyn InputDevice>);
struct RngNode(Arc<dyn RngDevice>);
```

Each impl `VfsNode`. No driver synthesizes its own `VfsNode` impl after
Phase 3.

---

## 4. Invariants every phase preserves

1. `#![forbid(unsafe_code)]` at the top of `crates/driver-api/src/lib.rs`
   and `crates/kernel/src/lib.rs`. The `drivers` crate may contain
   `unsafe` (MMIO, DMA phys-address arithmetic) but must document each
   `unsafe` block.
2. `CpuBase` stays `#[repr(C)]` with `cpu_id` at a fixed offset.
3. Logging macros keep using the `::hal::cpu_id()` absolute path.
4. Kernel crate name stays `solaya`.
5. `hal::panic_support::panic_disable_interrupts` wrapper stays (hides
   `unsafe` from kernel's forbid-unsafe).
6. Phase ordering is strict. No phase starts until the previous is
   green and committed.
7. `just ci` passes after **every commit**, not just end of phase. If a
   pre-commit hook formats code (Rust files auto-formatted, clippy --fix
   applied), accept that and re-stage if needed.
8. Every phase commits incrementally (2-7 small commits typical, not one
   mega-commit). CLAUDE.md mandates this.
9. Don't reintroduce a `sys` crate. Don't re-tangle
   `crates/kernel/src/klibc/` — it's out of scope for this refactor.

---

## 5. Phase-by-phase plan

Each phase is executed by one sub-agent. The coordinator (Claude) hands the
agent this document plus the phase's section reference.

### Phase 1 — `driver-api` foundation + `BlockDevice` + virtio-blk

**Scope:**
- Create `crates/driver-api/` with `Cargo.toml`, `src/lib.rs`
  (`#![no_std]`, `#![forbid(unsafe_code)]`, layering `//!` doc).
- Add `driver-api` to workspace members. **Also** add `crates/console` to
  workspace members (fixes Inconsistency 1).
- Define `trait BlockDevice`, `IoError` re-export, `ProbeError`.
- Create `crates/kernel/src/drivers/registry.rs` with `BlockDeviceRegistry`.
- Implement `driver_api::BlockDevice` for
  `crates/kernel/src/drivers/virtio/block.rs::BlockDevice`. Keep existing
  internal machinery; add a trait impl on top.
- In `init_block_devices` (`crates/kernel/src/drivers/mod.rs:42`), wrap each
  initialized block device in `Arc<dyn BlockDevice>` and register it.
- Refactor `crates/kernel/src/fs/ext2/mod.rs`: `mount_ext2(index)` reads
  from `BlockDeviceRegistry::global().get(index)` instead of importing
  `crate::drivers::virtio::block`.
- Collapse per-driver `DevBlock` in `virtio/block.rs:171-211` into one
  generic `BlockNode(Arc<dyn BlockDevice>)` in `crates/kernel/src/fs/devfs.rs`.
- Delete the now-unused `pub async fn read(index, ...)` / `write` free
  functions in `virtio/block.rs` **only if nothing else calls them** after
  refactor. If they're still referenced, leave them for Phase 8 cleanup.
- Add one trait-only unit test in `driver-api` that exercises a mock
  `BlockDevice` on x86_64 (`cargo test -p driver-api`). First
  driver-facing test that runs off target.

**Acceptance:**
- `just ci` green.
- `just run` boots to shell; `cd /mnt; ls` works; read/write of a file on
  vda works (existing system tests).
- `cargo test -p driver-api` passes on x86_64.
- `grep 'use crate::drivers::virtio::block' crates/kernel/src/fs/` returns empty.
- `grep 'register_devfs_node' crates/kernel/src/drivers/virtio/block.rs` returns empty.

### Phase 2 — `NetDevice` + virtio-net + dwmac

**Scope:**
- Add `trait NetDevice` to `driver-api`.
- Delete the local `pub trait NetworkDevice` in
  `crates/kernel/src/net/mod.rs:17`; replace its usage with
  `Arc<dyn driver_api::NetDevice>`.
- Migrate `virtio::net::NetworkDevice` and `dwmac::DwmacDevice` to impl
  `driver_api::NetDevice`.
- Add `NetDeviceRegistry`. Initialization pushes `Arc<dyn NetDevice>` into
  the registry.
- `network_rx_task` reads the primary device from the registry.
- Delete `net::assign_network_device` / `net::has_network_device` helpers
  and their back-references; the registry is the source of truth.

**Acceptance:**
- `just ci` green.
- `just run --net` boots with network; network system tests pass.
- No `trait NetworkDevice` exists anywhere in the tree except in
  `driver-api`.

### Phase 3 — char/display/input/rng + remaining drivers

**Scope:**
- Add `CharDevice`, `DisplayDevice`, `InputDevice`, `RngDevice` to
  `driver-api`.
- Wrap UART (`crates/kernel/src/io/uart.rs`) in a type implementing
  `CharDevice`. TTY layer (`io/tty_device.rs`) accepts
  `Arc<dyn CharDevice>` as backing device (narrow bridge only; full TTY
  migration stays deferred).
- Migrate `bochs_display` → `DisplayDevice`.
- Migrate `virtio::input` → `InputDevice`.
- Migrate `virtio::rng` → `RngDevice`.
- Replace per-driver `register_devfs_node` functions with generic devfs
  adapters (Section 3.8).
- Delete the per-device static globals in each driver once the trait-based
  path is the only one (`virtio::rng::set_device`, `virtio::input::set_device`,
  etc.).

**Acceptance:**
- `just ci` green.
- `/dev/console`, `/dev/fb0`, `/dev/input0`, `/dev/urandom` work through
  trait objects.
- Existing system tests for input/rng/display pass.

### Phase 4 — `IrqHandler` + RAII IRQ registration

**Scope:**
- Add `trait IrqHandler` and `struct IrqRegistration` to `driver-api`.
- Refactor `crates/kernel/src/interrupts/plic.rs`:
  - `INTERRUPT_HANDLERS: Vec<InterruptHandler>` becomes
    `Vec<(u32, Arc<dyn IrqHandler>)>` (or similar typed form).
  - `pub fn register_interrupt(u32, fn())` is replaced by
    `pub fn register(u32, Arc<dyn IrqHandler>) -> IrqRegistration`.
  - `dispatch_interrupt` calls `handler.handle()` through the trait object.
- Every driver replaces its `on_*_interrupt` free function with an
  `IrqHandler` impl on a per-driver context struct. Implementations stay
  short — they ack the device and wake the async task, same as today.
- `Drop for IrqRegistration` unregisters.
- Drivers hold the `IrqRegistration` in their device struct so interrupt
  teardown is automatic on `Drop`.

**Acceptance:**
- `just ci` green.
- `grep -rn 'fn() *$' crates/kernel/src/interrupts/` returns nothing
  matching IRQ registration.
- Interrupts still fire for block I/O, net, input.

### Phase 5 — `DmaBuffer` typed API

**Scope:**
- Add `struct DmaBuffer` to `driver-api` (wraps page allocator).
- Replace raw `as u64` / `as_ptr() as u64` casts in:
  - `crates/kernel/src/drivers/virtio/virtqueue.rs` — the 3 physical-address
    getters.
  - `crates/kernel/src/drivers/dwmac/mod.rs` — descriptor ring + packet
    buffers.
- `DmaBuffer::Drop` frees the backing pages via `mm::page_allocator`.
- `phys_addr()` today returns `virt as u64`; tomorrow might consult the
  IOMMU. Type hides the change.

**Acceptance:**
- `just ci` green.
- `grep -rn 'as \*const .* as u64\| as \*mut .* as u64\|\.as_ptr() as u64'
  crates/kernel/src/drivers/ crates/drivers/` returns empty.
- Virtio I/O works; DWMAC TX/RX works on hardware (best-effort; QEMU-only
  acceptance allowed if hardware isn't accessible this session).

### Phase 6 — `BusContext` (nexus/leaf split)

**Scope:**
- Define `trait BusContext` + supporting types in `driver-api`.
- Implement `PciBusContext` wrapping `PCIDevice` + PLIC + page allocator.
- Implement `DtBusContext` wrapping device-tree node + PLIC + page allocator.
- Change every driver's `::initialize` / `::new` signature to take
  `&dyn BusContext` instead of `PCIDevice` / DT node.
- Drivers lose their `use crate::pci::*`, `use crate::interrupts::plic::*`,
  `use crate::device_tree::*` imports.
- Registration flow (Section 3.6) fleshed out: catalog + probe + attach.

**Acceptance:**
- `just ci` green.
- `grep -rn 'crate::pci\|crate::interrupts::plic\|crate::device_tree'
  crates/kernel/src/drivers/` returns empty.
- Catalog-based dispatch replaces `init_network_device` / `init_block_devices`
  hardcoded bodies.

### Phase 7 — Extract `crates/drivers/`

**Scope:**
- Create `crates/drivers/` top-level crate. `no_std`; may contain
  `unsafe` (MMIO); does **not** `#![forbid(unsafe_code)]` but documents
  each `unsafe` block.
- Move `crates/kernel/src/drivers/virtio/`,
  `crates/kernel/src/drivers/dwmac/`,
  `crates/kernel/src/drivers/bochs_display.rs`,
  `crates/kernel/src/drivers/jh7110/` to `crates/drivers/src/`.
- `crates/kernel/src/drivers/` keeps only: `mod.rs` (thin glue), `registry.rs`
  (typed registries), the catalog bootstrap. Everything else moves.
- `crates/drivers/Cargo.toml` depends on `driver-api`, `hal`, `mm`, `console`,
  `abi`, `headers`. **Not** on `solaya`.
- Verify no `use crate::*` in `crates/drivers/` reaches the kernel crate.
- Add `crates/drivers` to workspace members.

**Acceptance:**
- `just ci` green.
- `grep -rn '^use solaya\|^use crate::fs\|^use crate::net\|^use crate::processes\|^use crate::syscalls'
  crates/drivers/ ` returns empty.
- `cargo tree -p drivers` shows no edge to `solaya`.
- All existing system tests pass.

### Phase 8 — Policy/mechanism split (`kernel/src/init/`)

**Scope:**
- Create `crates/kernel/src/init/` module.
- `init::bring_up_system()` orchestrates: read `BlockDeviceRegistry`, pick
  the first device, `kernel_tasks::spawn(fs::ext2::mount_ext2(...))`. Read
  `NetDeviceRegistry`, spawn `network_rx_task`. Etc.
- `drivers::register_builtin` + bus enumeration + per-device `attach` stays
  pure mechanism — no `fs::*`, no `kernel_tasks::*`, no `net::network_rx_task`.
- Update `crates/kernel/src/lib.rs` (or wherever `kernel_init` lives) to
  call `init::bring_up_system()` *after* driver enumeration completes.
- Remove `init_dwmac_devices` as a special case — DT enumeration becomes
  one of the buses the catalog walks.

**Acceptance:**
- `just ci` green.
- `grep -rn 'fs::ext2::\|net::network_rx_task\|kernel_tasks'
  crates/kernel/src/drivers/ crates/drivers/` returns empty.
- `kernel_init` reads as: parse device tree → enumerate buses → attach
  drivers → `init::bring_up_system()`.
- Full system test suite passes.

### Phase 9 — Doc + memory sync

**Scope:**
- Replace stale parts of `doc/ai/DRIVERS.md` with a short "see
  DRIVER_ARCHITECTURE.md" pointer + current reality.
- Update `doc/ai/OVERVIEW.md` with the new `drivers` crate + `driver-api`
  in the subsystem map.
- Update `CLAUDE.md` top-level file map (new crates, new paths).
- Update memory (`project_workspace_refactor.md` → mark workspace refactor
  done; add new memory `project_driver_refactor.md` noting driver architecture
  overhaul is complete on this PR; update `MEMORY.md` index).
- Bump `DRIVER_ARCHITECTURE.md` status to "complete — source of truth".

**Acceptance:**
- Docs build / render correctly.
- MEMORY.md entries are current.

---

## 6. Open-question escalation

The coordinator escalates to the user (via AskUserQuestion) when:
- A trait signature has non-local impact the doc didn't anticipate.
- A driver has no clean mapping to any defined trait.
- A system test starts failing and the fix is non-obvious.
- Two phases' plans contradict (doc must be amended first).
- A simpler alternative path appears mid-phase.

Do not paper over disagreement.

---

## 7. Document changelog

- v0 (initial): Written by coordinator from Phase-1 reconnaissance.
  Tracks planned architecture + 2 detected inconsistencies in the current
  tree (`crates/console` not in workspace; `crates/kernel/src/klibc/`
  grab-bag survives).
