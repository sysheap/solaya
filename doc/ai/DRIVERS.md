# Device Drivers

**Primary reference:** [DRIVER_ARCHITECTURE.md](DRIVER_ARCHITECTURE.md) — the
design contract, trait surface, registration flow, and phase-by-phase
rationale for the driver model. This file is a short map of what sits where
today.

## Clean-Room Development Policy

All drivers must be implemented from scratch using public hardware specs,
VirtIO specifications, and RFCs. Never reference or port Linux kernel driver
source code — Solaya is MIT-licensed and Linux drivers are GPL-2.0. If no
public spec exists for a device, that device cannot be supported until one
becomes available or a contributor takes on the licensing implications
independently.

## Where to find things

| Concern | Location |
|---------|----------|
| Trait surface (`BlockDevice`, `NetDevice`, `CharDevice`, `DisplayDevice`, `InputDevice`, `RngDevice`) | `crates/driver-api/src/lib.rs` |
| `BusContext` + `PciBusContextExt` + `DtBusContextExt` | `crates/driver-api/src/bus.rs` |
| `IrqHandler` + `IrqRegistration` + `IrqController` | `crates/driver-api/src/lib.rs` |
| `DmaBuffer` | `crates/driver-api/src/dma.rs` |
| virtio-blk / virtio-net / virtio-input / virtio-rng + virtqueue | `crates/drivers/src/virtio/` |
| DWMAC (Synopsys MAC + JH7110 init) | `crates/drivers/src/dwmac/` |
| Bochs display | `crates/drivers/src/bochs_display.rs` |
| PCI enumeration + `PciBusContext` | `crates/kernel/src/pci/` |
| PLIC + `IrqController` impl | `crates/kernel/src/interrupts/plic.rs` |
| Device tree parser + `DtBusContext` | `crates/kernel/src/device_tree.rs` |
| Typed registries per device class | `crates/kernel/src/drivers/registry.rs` |
| Driver enumeration (mechanism) | `crates/kernel/src/drivers/mod.rs` |
| Mount / task-spawn (policy) | `crates/kernel/src/init/mod.rs` |
| Devfs generic adapters (`BlockNode`, `CharNode`, ...) | `crates/kernel/src/fs/devfs.rs` |

## Adding a new driver (summary)

1. Decide which trait(s) in `driver-api` the device implements. If none fits,
   read DRIVER_ARCHITECTURE.md §3.2 before inventing a new trait.
2. Add the driver under `crates/drivers/src/`. It depends only on
   `driver-api`, `hal`, `mm`, `console`, `klib`, `abi`, `headers` — **never
   on `solaya`**.
3. Expose an `initialize(bus: &dyn BusContext) -> Result<...>` entry point
   plus an IRQ handler (usually an impl on the same handle struct).
4. Wire it into `crates/kernel/src/drivers/mod.rs::init_all_pci_devices` (or
   `init_dwmac_devices` for DT-enumerated devices): find a matching device,
   construct `PciBusContext`/`DtBusContext`, call `initialize`, register the
   `Arc<dyn Trait>` into the matching `Registry`. No mounts or task spawns
   here.
5. If the device needs userspace-visible side effects (auto-mount,
   background task), wire them in `crates/kernel/src/init/mod.rs` where
   policy lives.

See DRIVER_ARCHITECTURE.md for the full rationale, including per-phase
changelogs that record why specific shapes were chosen.
