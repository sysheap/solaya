# Solaya AI Documentation Index

Quick reference to find detailed documentation. Each file covers a specific subsystem.

## Documentation Files

| File | Contents | When to Read |
|------|----------|--------------|
| BUILD.md | Cargo workspace, build process, Nix environment | Build issues, adding dependencies |
| ARCHITECTURE.md | Boot sequence, subsystem interactions, data structures | Understanding overall system |
| MEMORY.md | Page allocator, page tables, heap | Memory bugs, allocation issues |
| PROCESSES.md | Process/thread lifecycle, scheduler, ELF loading | Process management, scheduling |
| INTERRUPTS.md | Trap handling, PLIC, timer interrupts | Interrupt issues, timer bugs |
| SYSCALLS.md | Syscall dispatch, async syscalls, validation | Adding/modifying syscalls |
| NETWORKING.md | Network stack (UDP, TCP), sockets, packet flow | Network features/bugs |
| DRIVERS.md | VirtIO, PCI enumeration, device tree | Device driver work |
| DRIVER_ARCHITECTURE.md | Trait-based driver model: BlockDevice/NetDevice/CharDevice/etc., BusContext, IrqHandler, DmaBuffer | Understanding driver API, adding a new driver |
| TESTING.md | Unit tests, system tests, QEMU infrastructure | Writing/debugging tests |
| FS.md | VFS layer, tmpfs, procfs, devfs, open files | Filesystem work, adding devices/proc entries |
| DEBUGGING.md | Logging, backtrace, GDB, dump functions | Debugging kernel issues |

## Quick Navigation by Task

### "I need to add a new syscall"
1. Read SYSCALLS.md for syscall dispatch and patterns
2. Check PROCESSES.md for process/thread context
3. See TESTING.md for how to test it

### "I need to debug a crash"
1. Read DEBUGGING.md for logging and backtrace
2. Check INTERRUPTS.md for trap handling
3. Use `just addr2line` for crash addresses

### "I need to understand memory management"
1. Read MEMORY.md for allocators and page tables
2. Check ARCHITECTURE.md for memory layout

### "I need to add a userspace program"
1. Read BUILD.md for build process
2. Check TESTING.md for system test patterns

### "I need to work on the filesystem"
1. Read FS.md for VFS architecture, mount layout, and how to add entries
2. Check SYSCALLS.md for filesystem syscalls (openat, fstat, lseek, getdents64, etc.)

### "I need to work on networking"
1. Read NETWORKING.md for stack architecture
2. Check DRIVERS.md for VirtIO network device

## Key Directories

The workspace is split by concern. No `sys` or `arch` crate any more; each concern owns its own crate.

```
crates/abi/           - Shared no_std ABI types (was "common")
crates/headers/       - Linux/musl bindgen-generated UAPI headers
crates/klib/          - Hardware-free utilities (array_vec, BTreeMap helpers,
                        non_empty_vec, runtime_initialized, util)
crates/hal/           - Hardware abstraction: RISC-V CSR/SBI/timer, MMIO,
                        Spinlock, page_table, PhysAddr/VirtAddr,
                        validated_ptr, signal trampoline, panic_support.
                        Has riscv64/ and stub/ submodules.
crates/mm/            - Page allocator, heap allocator (generic, host-testable)
crates/console/       - UART driver + log macros (info!, debug!, warn!)
crates/driver-api/    - Trait-only crate: BlockDevice, NetDevice, CharDevice,
                        DisplayDevice, InputDevice, RngDevice, IrqHandler,
                        IrqController, IrqRegistration, DmaBuffer,
                        BusContext (+ PciBusContextExt, DtBusContextExt),
                        MacAddress, InputEvent, FramebufferInfo. No impls.
                        See doc/ai/DRIVER_ARCHITECTURE.md.
crates/drivers/       - Concrete device drivers. Depends on driver-api, hal,
                        mm, console, klib, abi, headers. Never on `solaya`.
  src/virtio/         - virtio-blk, virtio-net, virtio-input, virtio-rng
  src/dwmac/          - Synopsys DWMAC + StarFive JH7110 init glue
  src/bochs_display.rs - QEMU Bochs VBE framebuffer
crates/kernel/ (crate name: `solaya`)
  src/lib.rs          - kernel_init, prepare_for_scheduling
  src/cpu.rs          - Cpu struct (embeds hal::cpu::CpuBase)
  src/memory/         - RootPageTableHolder, kernel mappings
  src/processes/      - Process, thread, scheduler, loader, signals
  src/syscalls/       - Syscall handlers (*_ops.rs per subsystem)
  src/interrupts/     - Trap handler, PLIC (owns IrqController impl)
  src/fs/             - VFS layer (tmpfs, procfs, devfs, ext2)
  src/net/            - Network stack (UDP, TCP, ARP, IP)
  src/drivers/        - Thin orchestrator: init_all_pci_devices /
                        init_dwmac_devices (mechanism only after Phase 8;
                        registers Arc<dyn Trait>s into registries).
    mod.rs            - PCI + DT enumeration loops
    registry.rs       - Typed registries per device class
  src/init/           - Policy layer: bring_up_system() reads the
                        registries and spawns ext2 mount + network_rx_task.
  src/io/             - UART extensions, TtyDevice (terminal subsystem)
  src/pci/            - PCI enumeration + PciBusContext
  src/device_tree.rs  - DT parser + DtBusContext
  src/platform/       - Emergency reset (jh7110 syscon-reboot, SBI fallback)
  src/klibc/          - Kernel-local grab-bag (big_endian, consumable_buffer,
                        elf, leb128, writable_buffer). Phase 2.8 of the
                        sys refactor was supposed to delete this; it still
                        lives. Tracked in issue #250.
  src/debugging/      - Backtrace, symbols, unwinder

boot/src/             - Thin entry point wrapper (#[no_mangle] functions)
                        that calls into `solaya`.
userspace/src/bin/    - Userspace programs (musl libc)
system-tests/src/     - QEMU-based integration tests
```
