# Solaya

[![ci](https://github.com/sysheap/solaya/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/sysheap/solaya/actions/workflows/ci.yml)

A RISC-V 64-bit hobby operating system kernel written in Rust. No third-party runtime dependencies — if third-party crates are used, they are build-time only.

Inspired by [SerenityOS](https://github.com/SerenityOS/serenity), this project exists because writing an OS from scratch is fun. Check out the [sysheap YouTube channel](http://www.youtube.com/@sysheap) for coding videos.

## Goal

**An experiment: can we write a Linux-compatible kernel in Rust from scratch?**

The long-term goal is a kernel that runs unmodified Linux userspace binaries — the same programs you'd run on a real Linux system, without recompilation. Only the kernel is rewritten; all userspace comes from existing projects (musl libc, dash, coreutils, etc.).

This is a hobby project and an honest experiment. We don't know how far we'll get. Progress is measured by running real programs: first a shell, then coreutils, then Python, then nginx. Each milestone is a concrete proof of compatibility, not a line-count metric.

See `plans/` for the roadmap and strategy.

## Status

### Kernel

- **Memory management** — Bitmap page allocator with lazy zeroing, Sv39 page tables (3-level), kernel heap, per-process address spaces, mmap/munmap, brk
- **Processes & threads** — ELF loading, per-process file descriptor tables, thread states (Running/Runnable/Waiting/Zombie), async syscall model with wakers
- **Scheduler** — Per-CPU round-robin scheduler with 10ms quantum, global run queue, SMP support
- **SMP** — Multi-core boot via SBI hart management, per-CPU state structs, inter-processor interrupts (IPI)
- **Syscalls** — 74 Linux-compatible syscalls including signals, mmap, futex, networking, and filesystem operations
- **Interrupts** — RISC-V trap handling, PLIC for external interrupts, timer interrupts
- **Networking** — TCP and UDP stacks with ARP, IPv4, Ethernet framing; VirtIO network driver; per-port socket binding
- **Filesystem** — VFS layer with tmpfs, procfs, devfs, and read-only ext2; programs can read from disk images
- **Drivers** — VirtIO network and block devices (feature negotiation, virtqueues), PCI enumeration with MMIO BAR allocation
- **Debugging** — DWARF-based backtrace with symbol resolution, Rust demangling, state dump on Ctrl+D, configurable per-module debug logging

### Userspace

- **Shell (SoSH)** — Command parsing, program execution with arguments, background processes (`&`), built-in help
- **30+ programs** — init, shell, TCP/UDP networking, sleep, stress testing, filesystem tests, signal tests, and various test utilities
- Programs are compiled against musl libc and embedded directly into the kernel binary

### Infrastructure

- **MCP server** — AI agents can boot QEMU, send shell commands, build the kernel, and run tests over the Model Context Protocol
- **GDB MCP server** — Programmatic GDB debugging (breakpoints, stepping, register inspection) exposed as MCP tools
- **System tests** — Integration tests that boot the OS in QEMU and interact via stdin/stdout, covering networking, processes, signals, stress, and shell behavior
- **Unit tests** — Kernel unit tests with a custom `#[test_case]` framework, plus Miri for undefined behavior detection
- **CI** — Build, fmt, clippy, unit tests, Miri, and system tests on Ubuntu via CMake
- **AI-assisted development** — Issues and PRs may be created by [Claude Code](https://claude.ai/code) under the maintainer's GitHub account

### Not Yet Implemented

- Copy-on-Write fork (currently copies the full address space on fork)
- Demand paging (mmap allocates all pages eagerly)
- epoll / select (only ppoll is implemented)
- Unix domain sockets
- Pseudo-terminals (PTY)
- Writable disk filesystem

## How Do I Run It?

The build system is CMake + Kconfig, driven through a thin `Makefile`
passthrough.  Three steps on a fresh clone:

```bash
make configure    # cmake --preset riscv64-virt (seeds build/.config)
make toolchain    # builds binutils/gcc/musl/linux-headers (~1h, cached)
make              # builds the kernel binary (build/kernel/solaya.bin)
make run          # boot the kernel in QEMU
```

The cross-toolchain builds from pinned source tarballs into
`build/toolchain/riscv64/`, so the only host deps are the packages that
bootstrap binutils/gcc/musl, drive the Rust build, and provide
`qemu-system-riscv64` for running the kernel.

### Host prerequisites

**Rust toolchain.**  The repo pins a specific nightly via
`rust-toolchain.toml`; install rustup and it will fetch the right
toolchain automatically on first `cargo` invocation:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain none --profile minimal
```

**Ubuntu / Debian.**  Exactly the set CI installs:

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
    build-essential \
    cmake ninja-build \
    python3 \
    libclang-dev clang \
    tmux \
    flex bison bc \
    texinfo gawk \
    pkg-config \
    rsync \
    e2fsprogs \
    qemu-system-misc qemu-system-data
```

**Fedora.**  Same tools, Fedora package names:

```bash
sudo dnf install -y \
    gcc gcc-c++ make \
    cmake ninja-build \
    python3 \
    clang-devel clang \
    tmux \
    flex bison bc \
    texinfo gawk \
    pkgconf-pkg-config \
    rsync \
    e2fsprogs \
    qemu-system-riscv seavgabios-bin
```

`e2fsprogs` is needed by the ext2 system tests (they shell out to
`mkfs.ext2`).  The `qemu-system-data` (Ubuntu) / `seavgabios-bin`
(Fedora) package ships `vgabios-bochs-display.bin`, which the
framebuffer + doom system tests need for `-device bochs-display`.

**cargo-nextest.**  Required for running the system tests:

```bash
cargo install cargo-nextest --locked
```

## What Can I Do?

Type `help` in the shell for available commands. Type a program name to execute it. Append `&` to run it in the background. See `userspace/src/bin/` for the full list of programs.

## Project Structure

```
kernel/           Main kernel (RISC-V 64-bit, no_std)
userspace/        Userspace programs (musl libc)
common/           Shared no_std library
system-tests/     Integration tests (run on x86, test via QEMU)
qemu-infra/       Shared QEMU communication library
mcp-server/       MCP server for AI agent interaction
gdb_mcp_server/   GDB MCP server for programmatic debugging
headers/          Linux C header bindings via bindgen
doc/ai/           Detailed AI-facing documentation
plans/            Roadmap and strategy documents
```

## Useful Commands

The `Makefile` at the root is a one-line passthrough to
`cmake --build build --target ...`; use whichever form you prefer.

```bash
make                # Build kernel binary
make run            # Build and run in QEMU
make run-fb         # Run with framebuffer
make debug          # QEMU (paused) + GDB attached in tmux
make attach         # Attach GDB to an already-running QEMU
make test           # Run all tests (unit + system)
make clippy         # Run linter across every workspace
make fmt-check      # cargo fmt --check across every workspace
make miri           # Undefined-behavior detector
make ci             # Full pre-merge gate (fmt + clippy + tests + miri + system)
make menuconfig     # Kconfig TUI to edit build/.config
```
