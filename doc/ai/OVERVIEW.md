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

```
arch/src/          - Hardware abstraction layer (CSR, SBI, timer, trap causes)
  riscv64/         - Real RISC-V implementations (+ backtrace, linker symbols)
  stub/            - No-op stubs for non-riscv64 targets (Kani, miri)

sys/src/           - Self-contained system library (no_std, no kernel deps)
  cpu.rs           - CpuBase struct, per-CPU access via sscratch
  asm/             - Assembly files (boot.S, trap.S, powersave.S, panic.S)
  memory/          - PhysAddr, VirtAddr, Page, PageTable, page allocator, heap
  klibc/           - Spinlock, MMIO, ValidatedPtr, runtime_initialized, sizes
  logging/         - Log macros (info!, debug!, warn!) and configuration
  io/              - UART driver

boot/src/          - Thin entry point wrapper (#[no_mangle] functions)
  main.rs          - Calls into kernel (solaya) crate

kernel/src/        - Main kernel logic (library crate, #![forbid(unsafe_code)])
  lib.rs           - kernel_init, prepare_for_scheduling
  cpu.rs           - Cpu struct (embeds sys::cpu::CpuBase)
  asm/             - Re-exports from arch
  memory/          - RootPageTableHolder, linker info, runtime mappings
  processes/       - Process, thread, scheduler, loader
  syscalls/        - Syscall handlers and validation
  interrupts/      - Trap handler, PLIC, timer
  fs/              - VFS layer (tmpfs, procfs, devfs, open file tracking)
  net/             - Network stack (UDP, TCP)
  drivers/         - VirtIO drivers, consolidated init
  io/              - UART extensions, TtyDevice (terminal subsystem)
  pci/             - PCI enumeration
  klibc/           - Re-exports from sys + kernel-specific utils (elf, mmio_struct!)
  debugging/       - Backtrace, symbols, unwinder
  logging/         - Re-exports from sys

userspace/src/
  bin/           - Userspace programs
  lib.rs         - Syscall wrappers

system-tests/src/
  infra/         - QEMU test infrastructure
  tests/         - Integration test suites
```
