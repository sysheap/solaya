# Build System

## Cargo Workspace

Root `Cargo.toml` defines workspace:
```
members = ["arch", "boot", "common", "headers", "kernel", "sys", "userspace"]
default-members = ["boot"]
```

### Workspace Crates

| Crate | Target | Purpose |
|-------|--------|---------|
| boot | riscv64gc-unknown-none-elf | Entry point wrapper (`#[no_mangle]` functions called from assembly) |
| kernel (solaya) | riscv64gc-unknown-none-elf | Main kernel logic (no_std, `#![forbid(unsafe_code)]`) |
| sys | riscv64gc-unknown-none-elf | Self-contained system library (page allocator, heap, spinlock, MMIO, logging) |
| arch | riscv64gc-unknown-none-elf | Hardware abstraction (CSR, SBI, backtrace, linker symbols) |
| userspace | riscv64gc-unknown-linux-musl | User programs (musl libc) |
| common | (inherited) | Shared no_std library |
| headers | (inherited) | Linux Header C bindings via bindgen |
| system-tests | x86_64-unknown-linux-gnu | Integration tests (separate workspace) |

### Crate Dependency Graph

```
boot -> kernel (solaya) -> sys -> arch
                        -> arch
                        -> common
                        -> headers
```

### Release Profile
```toml
[profile.release]
panic = 'abort'
lto = "fat"
debug = true              # Debug symbols enabled
overflow-checks = true    # Runtime overflow checks
debug-assertions = true   # Assertions enabled in release
```

## Build Process

### Full Build: `just build`
```
just build
  |
  +-> build-userspace
  |     cd userspace && cargo build --bins
  |     Output: kernel/compiled_userspace/*
  |
  +-> build-cargo
  |     cargo build --release (builds boot crate, which links kernel+sys+arch)
  |     Output: target/riscv64gc-unknown-none-elf/release/boot
  |
  +-> patch-symbols
        Extract symbols, embed in boot binary
        Output: symbols file appended to boot
```

### Userspace Delivery

Userspace is a buildroot-produced **cpio initramfs**, not a compile-time
embedding:

1. `userspace-rust` builds Solaya's Rust binaries (dhcpd, tests) into
   `build/userspace/artifacts/`.
2. `buildroot-overlay` copies those into `.buildroot/overlay/bin/`.
3. `buildroot-all` runs buildroot, which cross-builds busybox, dash,
   and GNU coreutils using the Bootlin prebuilt musl GCC toolchain
   (`BR2_TOOLCHAIN_EXTERNAL_BOOTLIN`), then layers our overlay on top
   and emits `.buildroot/output/images/rootfs.cpio`.
4. `qemu_wrapper.sh` passes the cpio via `-initrd`; kernel reads
   `/chosen/linux,initrd-{start,end}` from the DTB, reserves the
   range in the page allocator, and
   `initramfs::extract()` unpacks it into the tmpfs-backed root.
5. `process_table::init` reads `/sbin/init` from the VFS (buildroot
   symlinks it to `/bin/busybox`) and runs busybox as PID 1. Busybox
   reads `/etc/inittab` (shipped via overlay), runs `/etc/init.d/rcS`,
   waits on `/bin/dhcpd` to configure the network, then respawns
   `/bin/dash -i` on the console.

Kernel unit tests no longer embed userspace fixtures; all userspace
coverage lives in `system-tests/`, which boot the full image in QEMU.

**Adding a new userspace program:**
1. Create `userspace/src/bin/myprogram.rs`
2. Run `just build` (compiles) then
   `cmake --build build --target buildroot-all` (stages into overlay +
   re-assembles cpio)
3. Program lands at `/bin/myprogram` and is on dash's PATH.

### Symbol Embedding

Debug symbols are extracted and embedded in kernel for backtrace:
```bash
# Extract symbols
riscv64-unknown-linux-musl-nm --demangle --numeric-sort --line-numbers \
    target/riscv64gc-unknown-none-elf/release/kernel | grep -e ' t ' -e ' T ' > symbols

# Embed in binary
riscv64-unknown-linux-musl-objcopy --update-section symbols=./symbols \
    target/riscv64gc-unknown-none-elf/release/kernel
```

## Key Commands

| Command | Description |
|---------|-------------|
| `just build` | Full build (userspace + kernel + symbols) |
| `just build-cargo` | Build kernel only (assumes userspace built) |
| `just build-userspace` | Build userspace programs only |
| `just run` | Build and run in QEMU |
| `just clean` | Remove all build artifacts |
| `just clippy` | Run linter on all crates |
| `just miri` | Run miri for UB detection |

## Directory Output

```
target/
  riscv64gc-unknown-none-elf/
    release/
      boot                # Final kernel binary (entry point)
      libsolaya.a         # Kernel library

build/userspace/artifacts/   # userspace-rust output (Solaya Rust bins)

.buildroot/
  _dl/                      # Downloaded tarballs (buildroot, gcc, etc.)
  src/                      # Extracted buildroot source tree
  output/                   # Buildroot O= build dir
    images/rootfs.cpio      # Final initramfs image
  overlay/                  # Staged Rust bins + /etc skeleton (BR2_ROOTFS_OVERLAY)
```

## Linker Script

`kernel/qemu.ld` - Defines kernel memory layout for QEMU.

## Nix Environment

Development environment provided via `flake.nix`:
- Rust toolchain (nightly, riscv64 target)
- RISC-V cross-compiler toolchain
- QEMU system emulator
- GDB with pwndbg
- cargo-nextest

Automatically entered via direnv.

## Key Files

| File | Purpose |
|------|---------|
| Cargo.toml | Workspace configuration |
| justfile | Build commands |
| boot/Cargo.toml | Boot crate config (default build target) |
| boot/src/main.rs | Entry point wrapper |
| kernel/Cargo.toml | Kernel library config |
| kernel/build.rs | Userspace embedding |
| kernel/qemu.ld | Kernel linker script |
| sys/Cargo.toml | System library config |
| arch/Cargo.toml | Architecture HAL config |
| userspace/Cargo.toml | Userspace package config |
| flake.nix | Nix development environment |
