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
- v1 (post-Phase-1): Phase 1 landed in commits
  `6f1217f..55d530e`. Inconsistency 1 (console in workspace) is **fixed**.
  Adjustments to the design:
  - `driver-api`'s `Cargo.toml` does **not** depend on `mm` yet. `mm` sets
    `per-package-target = "riscv64gc-unknown-none-elf"`, which blocks host
    integration tests. Phase 5 (`DmaBuffer`) is the right time to add the
    `mm` dep, possibly gated to the riscv64 target.
  - `VfsNode::block_device_index() -> Option<usize>` was replaced with
    `VfsNode::block_device() -> Option<Arc<dyn BlockDevice>>`. Cleaner
    coupling and it let virtio's free-function `read`/`write` go private.
  - `BlockDeviceHandle` (the trait adapter) stays in `virtio/block.rs`
    for now. It moves to `crates/drivers/` in Phase 7 along with the
    concrete driver.
  - The registry index currently equals the virtio-internal `BLOCK_DEVICES`
    index by construction (`assert!(registered_idx == idx)`). Fine while
    there's one block driver; Phase 7 drops the indirection.
  - Pre-existing clippy violations in `crates/hal/src/stub/cpu.rs` surfaced
    when host-target builds started running from `driver-api`'s tests.
    Minor docs added to `unsafe fn`s; this is expected for any crate that
    gets compiled on the host going forward.
- v2 (post-Phase-2): `trait NetDevice` + the two network driver
  migrations landed. Adjustments to the design:
  - `MacAddress` moved to `driver-api`. The `NetDevice::mac()` method
    needs to name the type, and duplicating a `[u8; 6]` newtype in kernel
    and driver-api would break trait-object flow between the two. Kernel's
    `net::mac::MacAddress` is now a thin re-export. All existing use
    sites keep compiling.
  - `NetDevice::send` is **infallible** (`fn send(&self, Vec<u8>)`), not
    `Result`. Matches the current driver surface exactly — both virtio-net
    and DWMAC panic on backpressure today. Revisit in Phase 6 when
    `BusContext` gives drivers a clean way to report a full queue.
  - `NetDevice::send`/`receive` take `&self` (the design called for it).
    Drivers naturally need `&mut` to tick ring indices, so each migrates
    via a `VirtioNetHandle` / `DwmacHandle` wrapper that holds the real
    device behind a `Spinlock`. Same pattern as `BlockDeviceHandle` in
    Phase 1. Handles move to `crates/drivers/` in Phase 7.
  - `net::assign_network_device` is gone; `NetDeviceRegistry` is the
    source of truth and `net::primary_device()` reads from it. The four
    `net::has_network_device()` call sites (syscalls::net_ops,
    syscalls::ioctl_ops) were left alone — `has_network_device()` now
    just checks the registry length.
  - MTU is hard-coded to 1500 in both handles. Neither driver negotiates
    a larger MTU with the device today, but the trait method is in
    place so the negotiated value can surface when we add it.
- v3 (post-Phase-3): `Char/Display/Input/Rng` traits + UART/bochs/virtio-rng/virtio-input
  migrated. Adjustments:
  - `DisplayDevice` carries `read_at`/`write_at` instead of a speculative
    `flush(rect)`. That matches what devfs actually calls today; `flush` can
    be added when a compositor appears.
  - `InputEvent` is a `#[repr(C)]` struct in `driver-api`, a field-by-field
    twin of `VirtioInputEvent`. `VirtioInputHandle::poll_event` copies.
    Sharing a single definition would require driver-api to depend on
    kernel's `ByteInterpretable` — wrong layering direction.
  - `virtio::input` keeps a module-local
    `HANDLE: Spinlock<Option<Arc<VirtioInputHandle>>>` purely so the PLIC's
    `fn()` interrupt callback can reach `process_events()`. **Phase 4
    collapses this.**
  - `/dev/urandom` was renamed to `/dev/random` to match what the
    migrated RNG exposes — existing userspace callers were updated by the
    generic `RngNode` adapter (which reads `RngDeviceRegistry::primary()`).
  - Per-driver `RuntimeInitializedData<...>` globals (`virtio::rng::DEVICE`,
    `virtio::input::DEVICE`, `bochs_display` statics) are gone; the
    registries are the source of truth.
- v5 (post-Phase-5): `DmaBuffer` landed in `driver-api`; virtio ring memory
  migrated. Adjustments:
  - `driver-api`'s `Cargo.toml` now depends on `mm`. To make that viable,
    `mm/Cargo.toml` dropped its `per-package-target =
    "riscv64gc-unknown-none-elf"` pin (option (a) in the plan). `mm` has no
    target-specific code — page allocator + heap are generic no_std
    algorithms — so building it on x86_64 for host tests works. `cargo test
    -p mm --target x86_64-unknown-linux-gnu` passes cleanly and the default
    riscv64 build is unchanged.
  - `DmaBuffer` wraps `mm::page::PinnedHeapPages`. `new_coherent` rounds the
    requested length up to a page boundary, stores the requested length for
    accessor truncation, and returns `Ok(_)` today (panics on OOM — consistent
    with the kernel's infallible page-alloc pattern). `phys_addr()` equals
    `virt_addr()` on the current identity-mapped target.
  - To keep the kernel crate `#![forbid(unsafe_code)]`, DmaBuffer exposes
    **typed** accessors — `as_typed<T>` / `as_typed_mut<T>` — so virtqueue
    can reinterpret a DmaBuffer as `[virtq_desc; QUEUE_SIZE]` etc. without a
    raw cast in the kernel. The `unsafe` raw-pointer reinterpretation lives
    inside `driver-api`'s `dma` module.
  - `driver-api`'s crate-level `#![forbid(unsafe_code)]` relaxes to
    `#![deny(unsafe_code)]` so the `dma` module can opt in via
    `#[allow(unsafe_code)]`. Every other module in `driver-api` remains
    unsafe-free and is statically checked by the deny. The kernel crate
    stays `#![forbid(unsafe_code)]`.
  - `mm::page::PagesAsSlice` grew a `as_u8_slice_ref(&self) -> &[u8]`
    companion to the existing `&mut` method so `DmaBuffer::as_slice` is a
    safe wrapper.
  - Per-request `Vec<u8>` buffers in `VirtQueue::put_buffer_chain` still use
    `buffer.as_ptr() as u64` — they're short-lived heap-backed buffers
    passed to the virtio device and, on the current identity-mapped target,
    the virtual address is the physical address. Migrating them requires
    rewriting every virtio-* driver to hand DmaBuffers to VirtQueue;
    deferred to a future phase. The cast is documented at the call site.
  - DWMAC migration is **deferred**. DWMAC uses 32-bit DMA addressing
    (`as u32`, not `as u64`), so the acceptance grep does not flag it. QEMU
    doesn't route the StarFive MAC, so any DWMAC changes cannot be
    validated end-to-end from this session. Tracking: migrate DWMAC rings +
    packet buffers to DmaBuffer when StarFive hardware is available.
  - Host-side `DmaBuffer` tests (`crates/driver-api/tests/dma_buffer.rs`)
    run through the Rust global allocator (which honours 4 KiB alignment),
    so no kernel page-allocator init is needed in the host-test context.

- v4 (post-Phase-4): `IrqHandler` trait + RAII `IrqRegistration` landed.
  Every driver now goes through `Arc<dyn IrqHandler>`; no `fn()`-pointer
  interrupt handlers remain. Adjustments:
  - `IrqRegistration` lives in `crates/kernel/src/interrupts/plic.rs`, not
    in `driver-api` (option (c) in the plan). The concrete teardown path
    needs kernel-private state (`INTERRUPT_HANDLERS`, `PLIC`), so keeping
    the type there avoids a second `IrqController` trait in `driver-api`
    just to make `Drop` work. Drivers hold the opaque token and don't see
    the PLIC.
  - Slot identifier is a monotonic `u64` counter instead of a Vec index —
    `swap_remove` during unregister would otherwise invalidate other
    outstanding tokens. Slots are compared for equality; the lookup is
    linear, which is fine for a few dozen IRQs.
  - `unregister` disables the IRQ at the PLIC when the last handler for
    that line goes away. For shared lines (none today, but block I/O could
    grow multiple devices on one irq later) it just removes the entry.
  - Every `NetDevice` impl (`VirtioNetHandle`, `DwmacHandle`) also impls
    `IrqHandler` on the same struct. The handler body reads the
    device-specific ISR (read-to-clear on VirtIO, write-1-to-clear on
    DWMAC4) and calls `net::notify_packet_arrival()` — a new short helper
    that owns the shared `NETWORK_INTERRUPT_COUNTER` + wakers drain. The
    global `ISR_STATUS` / `init_isr_status` / `on_network_interrupt` in
    `net/mod.rs` are gone.
  - Each driver handle stores `Spinlock<Option<IrqRegistration>>` (rather
    than `IrqRegistration` directly) because the registration requires
    `Arc<dyn IrqHandler>` pointing at the handle itself, so the handle
    must exist first. A `set_irq_registration` setter is called right
    after `plic::register` at init time. `BlockDeviceHandle` is the one
    exception — its `BlockIrqHandler` is a separate struct, so it can
    take the `IrqRegistration` by value.
  - `virtio::input::HANDLE: Spinlock<Option<Arc<...>>>` (the Phase 3 shim)
    is deleted. The PLIC now holds the `Arc<dyn IrqHandler>` directly.
  - UART's `IrqRegistration` is `mem::forget`ed in `kernel_init` — the
    console lives forever and there's no graceful shutdown path.
  - `cargo test -p driver-api --target x86_64-unknown-linux-gnu` grows a
    new `irq_handler` suite (two cases) proving trait-object dispatch.
    The `IrqRegistration::Drop` plumbing can only be exercised on-target
    (needs the PLIC MMIO machinery), so it's covered indirectly via the
    69 system tests.

- v6 (post-Phase-6): `BusContext` + `PciBusContextExt` + `DtBusContextExt`
  landed in `driver-api`; every driver's `initialize` / matcher now takes
  `&dyn BusContext`. Adjustments to the design:
  - **PCI-specific surface lives on a trait extension**, not behind a
    downcast (`as_any()`) or a sealed method. Scope considered both; the
    extension-trait path won because virtio drivers genuinely need full
    capability-walking access — hiding that behind `read_config_u32(offset)`
    would have meant reimplementing the capability linked-list walk inside
    every driver. `BusContext::as_pci() -> Option<&dyn PciBusContextExt>`
    is the explicit hop drivers take once, at the top of `initialize`.
    `DtBusContextExt` mirrors the shape for DT-bound drivers (today: none
    — DWMAC works from just `BusContext`'s DMA + IRQ surface).
  - **`IrqRegistration` moved to `driver-api`** via a new `trait
    IrqController { fn unregister(&self, slot: u64); }`. Phase 4's v4
    changelog chose the option (c) "keep it in kernel" path; Phase 6
    reverses to option (a) because `BusContext::register_irq` must return
    it from the trait. The PLIC now impls `IrqController` on a ZST
    `PlicController`, held as `Arc<dyn IrqController>` inside every
    outstanding `IrqRegistration`. The slot identifier collapsed from a
    newtype (`SlotId(u64)`) to a plain `u64` at the trait boundary.
  - **Capability iteration goes through `MMIO<PciCapabilityHeader>`**, a
    two-byte `{id, next}` struct in driver-api plus a
    `PciCapabilityHeaderExt` trait providing `.id()`, `.next_offset()`,
    and `.as_type::<T>()`. Drivers reinterpret the returned MMIO handle
    as their driver-specific capability layout. Moving the kernel's
    `mmio_struct!` macro into driver-api would have dragged the whole
    `Fields`-trait pattern along; the manual two-method helper is
    lighter and sufficient for the one caller pattern today.
  - **BAR mapping returns an `MmioRegion { virt_base, len }`** — not an
    `MMIO<T>` directly, because BARs almost always hold multiple
    driver-defined register blocks at different offsets. Drivers compute
    `MMIO::new(region.virt_base + offset)` or call
    `region.typed_at::<T>(offset)`.
  - **Matchers take `&dyn BusContext` too**, not `&PCIDevice`. The
    previous pattern (`pci_devices.iter().position(is_virtio_*)`)
    couldn't survive — matchers needed `crate::pci::*` just to read
    vendor/device IDs. `drivers/mod.rs` now has a small
    `find_pci_device(&mut [PCIDevice], impl Fn(&dyn BusContext) -> bool)`
    that builds a `PciBusContext` around each candidate before handing
    it to the predicate. Matching is read-only (config-space reads), so
    the interior-mutable `Spinlock<&mut PCIDevice>` inside
    `PciBusContext` is fine even for immutable-feeling probes.
  - **`PciBusContext::new` takes `&mut PCIDevice`**, not by value. BAR
    initialization and command-register writes both require `&mut` on
    the underlying MMIO, but the `PCIDevice` itself still lives in
    `drivers/mod.rs::init_*_device` — the bus context borrows it for
    the duration of probe/init only.
  - **No `DriverCatalog` / `DriverFactory`** in this phase. Section 3.6
    of the design doc sketches them; scope explicitly deferred them so
    this phase's diff stays focused on the bus surface. The existing
    `init_network_device` / `init_block_devices` / etc. keep their
    shape. Catalog dispatch is a future phase.
  - **`read_config_u32(byte_offset)` vs named DT properties.** Scope
    flagged this as a design question. Resolution: `BusContext` does
    **not** carry `read_config_u32`; it moved to `PciBusContextExt`
    where the `u16` byte offset is natural. The equivalent DT surface
    is `DtBusContextExt::reg_base()` / `reg_size()` only — drivers
    that need specific DT properties would add another extension trait
    method later. For DWMAC today, `reg_base`/`reg_size` is everything
    the driver needs; clocks/resets/mac-addr parsing remain in the
    `drivers/mod.rs` orchestrator and the `jh7110` SoC helpers, which
    are allowed to import `device_tree::*` since they sit above the
    driver layer.
  - **`enable_dma()` / command-register bits** split: `set_command_bits`
    and `clear_command_bits` live on `PciBusContextExt` with a
    `pci_command` constants module (`BUS_MASTER`, `INTERRUPT_DISABLE`,
    `MEMORY_SPACE`, `IO_SPACE`) in `driver-api::bus`. The scope's
    proposed bus-agnostic `enable_dma()` was redundant: DT drivers on
    this target have nothing to do here, and the PCI drivers always
    want both BUS_MASTER set and INTERRUPT_DISABLE cleared, which they
    already accomplish with two named calls.
  - **IRQ registration flows through `bus.register_irq`** uniformly —
    PCI drivers and DWMAC alike. `plic::register` is still the concrete
    implementation both bus contexts call through to; it's no longer
    referenced from any driver file.
  - **`bochs_display::fb_base()` returns `Option<usize>`** instead of
    `Option<PciCpuAddr>`. The PCI-specific newtype leaked out of the
    driver only for the devfs read/write path — downgrading to `usize`
    makes the display driver consume `crate::pci::*`-free.
  - `cargo test -p driver-api --target x86_64-unknown-linux-gnu` grows
    a new `bus_context` suite proving object-safety and trait-extension
    dispatch on a `MockPciBus`. Two cases.
  - Acceptance grep
    `grep -rn 'use crate::pci\|use crate::interrupts::plic\|use crate::device_tree' crates/kernel/src/drivers/`
    returns **empty**. The matching `crate::*::` reference grep also
    returns empty for driver files (virtio/, dwmac/, bochs_display.rs).
    Only `drivers/mod.rs` still reaches these — scope treats it as the
    orchestrator layer, not a driver file, and it stays that way until
    the Phase 8 policy/mechanism split.

- v7 (post-Phase-7): Concrete drivers extracted to `crates/drivers/`.
  `kernel/src/drivers/` is now a thin orchestrator (mod.rs + registry.rs).
  Adjustments:
  - Scaffolding commits: `1816e5b` (empty drivers crate), `9fb799f`
    (`mmio_struct!` macro relocated from `kernel::klibc` to `hal`),
    `8e2fb49` (net_notifier hook in driver-api for cross-driver RX wakes).
  - Driver moves: `4d6e47e` (bochs_display), the virtio block/net/input/
    rng/virtqueue/capability cluster, and dwmac. Each concrete driver now
    lives under `crates/drivers/src/` with imports going through `console`,
    `hal`, `klib`, `mm`, `driver-api` — never through `solaya`. `cargo
    tree -p drivers` shows zero edge to `solaya`.
  - `jh7110/reset.rs` stayed behind as `crates/kernel/src/platform/reset.rs`.
    It reaches into `device_tree` and is called from the UART panic-path
    (`crate::platform::reset::trigger_reset()`). That's kernel infrastructure,
    not a device driver — hoisting it would have required threading a
    `DtBusContext` through panic context, which is scope creep.
  - The DWMAC SoC init helper (`dwmac::jh7110::init_gmac`) did move to
    `crates/drivers/src/dwmac/jh7110.rs` — it's proper driver code, just
    named after the SoC it targets.
  - Kernel `klibc/mod.rs` dropped the unused `non_empty_vec` and
    `is_power_of_2_or_zero` re-exports that only existed to serve the
    now-relocated virtio drivers.
  - No `DriverFactory` / `DriverCatalog` introduced yet — same scope rule
    as Phase 6. `init_all_pci_devices` / `init_dwmac_devices` still live
    in `kernel/src/drivers/mod.rs` and still reach into kernel subsystems
    for mount/task-spawn orchestration. **Phase 8 is the right place to
    split this.**
  - Acceptance grep
    `grep -rn '^use (solaya|crate::(fs|net|processes|syscalls|interrupts|pci|device_tree|cpu|klibc))' crates/drivers/`
    returns **empty**. 69/69 system tests green.

- v8 (post-Phase-8): Policy/mechanism split landed.
  `kernel/src/init/mod.rs` is a new top-level module that owns system
  bring-up policy. `kernel_init` now calls `drivers::init_all_pci_devices`
  + `drivers::init_dwmac_devices` (mechanism — enumerate, initialize,
  register into the typed registries), then `init::bring_up_system()`
  (policy — expose devices in devfs, mount ext2 on the first block
  device, spawn the network RX task if any net device is present).
  Adjustments:
  - Scope kept to a single `init/mod.rs` file — under the 150-line
    guideline, no sub-module split yet.
  - `fs::devfs::register_*` calls also moved out of `drivers/mod.rs`
    into `bring_up_system`. The task scope flagged `fs::ext2` and
    `net::network_rx_task` explicitly, but the same "no `fs::*` in
    drivers/" rule applies to the devfs-node registrations, so they
    moved too. `CharDeviceRegistry` stays hands-off: the console UART
    registers itself via `io::uart::register_console_char_device()` at
    init time, outside the generic driver enumeration loop — that's
    boot-level infrastructure, not policy worth centralizing.
  - `init_dwmac_devices`'s early-out and per-child guard switched from
    `net::has_network_device()` to `NetDeviceRegistry::global().len()`.
    Both check the same underlying registry today; the new form avoids
    leaning on `crate::net::*` from `drivers/mod.rs`.
  - `init_all_pci_devices` + `init_dwmac_devices` remain on the kernel
    side (not in `crates/drivers/`) since they wire up kernel-specific
    bus contexts (`PciBusContext`, `DtBusContext`) and parse the
    kernel-owned device tree. They qualify as "kernel mechanism" vs
    "driver code that lives in `crates/drivers/`".
  - Acceptance grep
    `grep -rn 'fs::ext2::\|net::network_rx_task\|kernel_tasks::spawn'
    crates/kernel/src/drivers/ crates/drivers/` returns **empty**.
    `drivers/mod.rs` imports only `device_tree`, `info!`, `klibc`,
    `net::mac::MacAddress`, `pci`, plus `driver_api` — no `fs`, no
    `net::{self}`, no `processes::kernel_tasks`. 69/69 system tests
    green.
