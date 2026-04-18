# Plan 04: Build System and Multi-Architecture Support

## 1. Current Build System Evaluation

### What We Have

The current build system is a three-layer stack:

1. **Nix flake** (`flake.nix`) -- provides the entire development environment: Rust nightly toolchain, RISC-V cross-compiler (GCC + binutils for riscv64-musl), QEMU, cargo-nextest, pwndbg, and pre-built userspace dependencies (musl libc, dash shell, Doom). Nix also cross-compiles musl and dash for RISC-V and symlinks them into the source tree via `shellHook`.

2. **just** (`justfile`) -- ~30 recipes orchestrating the multi-stage build: build-coreutils, build-userspace, build-cargo, patch-symbols, run, test, clippy, miri, debug, etc.

3. **Cargo workspace** -- 5 crates (kernel, arch, common, headers, userspace) in the main workspace, plus 3 standalone workspaces (system-tests, mcp-server, qemu-infra). The kernel uses `per-package-target` to set `default-target = "riscv64gc-unknown-none-elf"`, and userspace uses `forced-target = "riscv64gc-unknown-linux-musl"`.

Additional build-time machinery:
- `headers/build.rs` -- runs bindgen against Linux UAPI and musl headers to generate syscall numbers, errno, socket types, fs types
- `kernel/qemu.ld` -- RISC-V linker script (entry at 0x80200000, SBI convention)
- `qemu_wrapper.sh` -- 140-line bash script configuring QEMU flags (GDB, network, SMP, framebuffer, block devices)
- `.cargo/config.toml` -- sets default target to riscv64gc-unknown-none-elf, configures QEMU runner

### Strengths

- **Reproducibility**: Nix pins every dependency including the Rust nightly date (2026-03-02), musl version, GCC version. Any developer gets the identical environment.
- **Zero manual setup**: `nix develop` (or direnv) gives you everything. No "install QEMU 8.x, then get the right GCC cross-compiler" instructions.
- **Cargo integration**: The `per-package-target` feature lets kernel and userspace coexist in one workspace with different targets. `cargo build --release` from the workspace root "just works" for the kernel.
- **Fast iteration**: `just run` does full build + QEMU launch. System tests boot a fresh QEMU per test via the qemu-infra crate.
- **Separation of concerns**: just handles orchestration, Cargo handles Rust compilation, Nix handles toolchain provisioning. Each does what it is best at.

### Weaknesses

- **Single architecture hardcoded everywhere**: riscv64 is baked into the justfile, `.cargo/config.toml`, the linker script path, qemu_wrapper.sh, and build.rs. Adding x86_64 requires touching all of these.
- **Nix complexity**: The flake.nix is already 170 lines with custom overlays for musl debug symbols and a cross-compiled Doom build. Few contributors will be comfortable modifying it.
- **Scattered build logic**: Build steps span justfile (shell), build.rs (Rust), flake.nix (Nix), and qemu_wrapper.sh (bash). Understanding "what happens when I type `just build`" requires reading 4 files in 3 languages.
- **No parameterization**: There is no `ARCH` variable. You cannot do `just build ARCH=x86_64`. The justfile has zero conditional logic.
- **Symbol patching is fragile**: The post-build `objcopy --update-section` step to embed symbols is architecture-specific (uses riscv64 nm/objcopy) and would break for a second architecture.
- **Userspace embedding at build.rs time**: The kernel build.rs reads from `kernel/compiled_userspace/` and `kernel/compiled_userspace_nix/`. This path is hardcoded and assumes a single-architecture userspace.
- **CI uses self-hosted runners**: The GitHub Actions CI (`ci.yml`) runs on `[self-hosted, nix]` runners with direnv. This works but is not portable to standard GitHub-hosted runners and limits external contributors.

## 2. Build System Options

### Option A: Keep Nix + just (current, extended)

**Approach**: Add an `ARCH` parameter to the justfile. Create per-architecture linker scripts, QEMU configs, and `.cargo/config-{arch}.toml` files. Use `just build ARCH=riscv64` or `just build ARCH=x86_64`.

**Pros**:
- Minimal disruption. Existing workflows stay the same.
- just supports variables and conditionals natively (`if ARCH == "x86_64" { ... }`).
- Nix already handles cross-compilation well; adding an x86_64 target is straightforward (no new cross-compiler needed for x86 when building on x86).
- AI agents already understand the current setup (documented in CLAUDE.md, MCP tools use `just build`).

**Cons**:
- just is not a real programming language. Complex conditional logic (different QEMU flags, different linker scripts, different objcopy commands per arch) will make the justfile messy.
- No dependency tracking between steps. `just build` always runs all steps regardless of what changed.
- Shell-based string manipulation for paths and flags is error-prone.

**Verdict**: Best short-term option. The justfile needs parameterization but not replacement.

### Option B: Plain Makefiles (Kbuild-style)

**Approach**: Replace the justfile with a Makefile hierarchy, potentially with Kconfig for feature selection.

**Pros**:
- Make has real dependency tracking (timestamps).
- Kconfig allows runtime selection of features (enable/disable network, select architecture, choose scheduler).
- Familiar to anyone who has built the Linux kernel.

**Cons**:
- Cargo already handles Rust dependency tracking. A Makefile would duplicate this and fight with it.
- Kconfig is a massive system. Implementing even a subset is weeks of work for minimal benefit at Solaya's current scale (118 kernel source files).
- Make's syntax for cross-platform, multi-target builds is painful.
- Linux uses Kbuild because it has 30,000+ source files, hundreds of config options, and in-tree C compilation. Solaya has none of these problems.
- AI agents handle justfiles and Cargo better than complex Makefile hierarchies.

**Verdict**: Not worth it. Kconfig makes sense at Linux scale (thousands of config options), not at Solaya scale. The dependency tracking benefit is already provided by Cargo.

### Option C: CMake

**Approach**: Use CMake as the top-level build orchestrator, invoking Cargo for Rust compilation.

**Pros**:
- Good cross-compilation support with toolchain files.
- IDE integration (CLion, VS Code CMake Tools).

**Cons**:
- CMake and Cargo fundamentally conflict. CMake wants to control compilation; Cargo also wants to control compilation. You end up with CMake calling `cargo build` as an external command, which is no better than a justfile doing the same.
- CMake's language is widely disliked.
- No one in the Rust OS community uses CMake.
- Adds a large dependency for no real benefit.

**Verdict**: Wrong tool for a Cargo-based project. Hard no.

### Option D: Meson / build2

**Approach**: Use Meson or build2 as the top-level build system.

**Pros**:
- Meson is faster than Make, has good cross-compilation support, and a cleaner syntax.
- build2 has explicit support for cross-compilation workflows.

**Cons**:
- Same fundamental problem as CMake: these are C/C++ build systems that would just shell out to Cargo.
- Tiny community adoption for Rust projects, and zero adoption in Rust OS development.
- Another dependency to install and learn.

**Verdict**: No benefit over just + Cargo. Not worth considering.

### Option E: Cargo xtask (drop just entirely)

**Approach**: Add an `xtask` crate to the workspace. Move all justfile logic into Rust code. Invoke via `cargo xtask build`, `cargo xtask run`, `cargo xtask test`.

**Pros**:
- Everything is Rust. No shell scripts, no justfile syntax to learn.
- Can import crates (e.g., `xshell` for running commands, `clap` for argument parsing).
- Type-safe, testable build logic. Architecture selection becomes an enum, not string manipulation.
- Used by major Rust projects: rust-analyzer, Hermit OS (`cargo xtask build --arch x86_64`), OpenVMM.
- AI agents work well with Rust code.

**Cons**:
- Initial migration effort to rewrite ~30 just recipes in Rust.
- Slower to iterate on build logic (need to compile xtask before running it, though this is cached).
- `cargo xtask build` is more typing than `just build`, though shell aliases fix this.
- Cannot intercept `cargo build` -- you must always use `cargo xtask build` for the full pipeline, which is a training/documentation issue.

**Verdict**: Strong option for the medium term. The Hermit OS precedent is directly applicable -- they solved the same multi-arch problem with xtask. However, migrating 30 recipes is not free. Do it when adding x86_64, not before.

### Option F: Bazel

**Approach**: Use Bazel with rules_rust for hermetic, reproducible builds.

**Pros**:
- Hermetic by design. Every build input is tracked.
- Excellent caching (remote cache, content-addressable).
- Can build Rust, C, assembly, and generate disk images in one system.
- Cross-compilation is a first-class concept (platforms, toolchains).

**Cons**:
- Enormous complexity overhead. Bazel's learning curve is steep even for application development; for OS kernel development it is brutal.
- rules_rust for bare-metal/no_std targets is poorly documented and rarely used. The one example (bazel-rust-cross) only gets as far as a toy bootloader.
- Bazel fights with Cargo. You either abandon Cargo entirely (losing cargo clippy, cargo miri, cargo test, cargo doc, the entire Rust ecosystem tooling) or maintain parallel build definitions.
- Nix already provides hermeticity. Bazel's main selling point is redundant.
- Massive runtime dependency (Bazel itself is hundreds of MB).
- AI agents have poor familiarity with Bazel BUILD files for kernel development.

**Verdict**: Overkill. Bazel solves problems at Google scale (millions of lines, thousands of engineers). For a hobby OS with 118 source files and 1-3 contributors, the overhead is not justified. Nix already provides the hermeticity benefit.

### Summary Table

| Option | Effort | Multi-arch | AI-friendly | Recommendation |
|--------|--------|-----------|-------------|----------------|
| A. Nix + just (extended) | Low | Medium | Good | **Do now** |
| B. Makefiles/Kbuild | High | Good | Medium | Skip |
| C. CMake | Medium | Good | Poor | Skip |
| D. Meson/build2 | Medium | Good | Poor | Skip |
| E. Cargo xtask | Medium | Excellent | Excellent | **Do when adding x86_64** |
| F. Bazel | Very High | Excellent | Poor | Skip |

## 3. Multi-Architecture Support: Adding x86_64

### 3.1 Structuring the arch/ Crate

The current `arch/` crate has a clean two-way dispatch:

```
arch/src/
  lib.rs              # cfg(target_arch = "riscv64") -> riscv64/, else -> stub/
  riscv64/
    cpu.rs            # CSR read/write, interrupt control, memory fence
    sbi/              # SBI ecall wrappers (timer, IPI, hart state)
    timer.rs          # rdtime, timer constants
    trap_cause.rs     # Interrupt/exception cause codes
  stub/
    cpu.rs            # No-op stubs for miri/unit tests
    sbi.rs
    timer.rs
    trap_cause.rs
```

For x86_64, the `arch/` crate should become a three-way dispatch:

```
arch/src/
  lib.rs              # cfg dispatch: riscv64 | x86_64 | stub
  riscv64/
    mod.rs
    cpu.rs            # CSR ops, sfence, interrupt guard
    sbi/              # SBI interface
    timer.rs          # rdtime-based timer
    trap_cause.rs     # RISC-V exception codes
  x86_64/
    mod.rs
    cpu.rs            # MSR ops, CR3, interrupt flag
    apic.rs           # Local APIC / IO-APIC (replaces PLIC)
    timer.rs          # APIC timer / TSC / PIT
    trap_cause.rs     # x86 exception vectors (0-31)
  stub/
    (unchanged, for miri/tests)
```

**Key design principle**: The `arch` crate exposes a uniform API. Each architecture module must export the same set of public functions and types. The kernel imports `arch::cpu::read_*`, `arch::timer::*`, etc. without knowing which architecture it is running on.

The current arch API surface to preserve/generalize:

| Current (RISC-V) | x86_64 equivalent | Notes |
|---|----|---|
| `read_satp()` / `write_satp()` | `read_cr3()` / `write_cr3()` | Page table base register |
| `read_sepc()` | Read from trap frame (RIP) | x86 pushes IP to stack |
| `read_scause()` | Interrupt vector number | Different dispatch model |
| `write_satp_and_fence()` | `write_cr3()` (implicit TLB flush) | CR3 write flushes TLB on x86 |
| `InterruptGuard` | Same concept, `cli`/`sti` | |
| `wait_for_interrupt()` | `hlt` instruction | |
| `enable_timer_interrupt()` | APIC timer setup | Much more complex on x86 |
| `sbi_set_timer()` | APIC timer write | |
| `sbi_send_ipi()` | APIC IPI write | |
| `start_hart()` | SIPI sequence | |

**What needs abstraction**: The arch crate should define a trait-like interface (or just a set of function signatures that each arch module must implement). This does not need to be a Rust `trait` -- Hermit OS and Redox both use `cfg`-gated modules with matching function signatures, which is simpler and has zero runtime cost.

### 3.2 Boot Process Differences

**RISC-V (current)**:
- OpenSBI firmware runs first, sets up M-mode
- Jumps to kernel at 0x80200000 (configured in `qemu.ld`)
- Passes hart_id in a0, device tree pointer in a1
- Kernel entry: `boot.S` sets up stack, jumps to `kernel_init(hart_id, dtb_ptr)`

**x86_64 (new)**:
- Needs a bootloader. Two main options:
  1. **rust-osdev/bootloader crate**: Pure Rust bootloader that handles BIOS+UEFI, sets up long mode, identity maps memory, passes `BootInfo` struct to kernel. Used by the "Writing an OS in Rust" tutorial. Simplest integration path.
  2. **Limine protocol**: Modern bootloader protocol with a well-defined Rust binding (`limine` crate). Supports UEFI and legacy BIOS. More features than the bootloader crate.
  3. **Multiboot2 + GRUB**: Most "standard" but requires GRUB installation and more setup complexity.
- Bootloader sets up paging (4-level, 4KB pages), GDT, enters long mode
- Passes memory map, framebuffer info, kernel address info
- Kernel entry: Rust function receiving boot info struct

**Recommendation**: Use the `bootloader` crate from rust-osdev. It is the most common choice in the Rust OS community, handles the complex x86 boot process, and requires minimal kernel-side code. The kernel provides a `_start(boot_info: &BootInfo)` entry point and receives a clean environment.

### 3.3 Sharing Kernel Code While Isolating Arch-Specific Code

Currently, 46 `cfg(target_arch = "riscv64")` annotations are scattered across 9 kernel source files. The major areas that need per-architecture implementations:

| Subsystem | Files affected | What changes per arch |
|-----------|---------------|----------------------|
| Boot/entry | `main.rs`, `asm/` | Entry point, initial setup |
| Assembly | `asm/*.S` (198 lines) | Context switch, trap entry, boot |
| Interrupts | `interrupts/trap.rs`, `plic.rs` | Trap dispatch, interrupt controller |
| Page tables | `memory/page_tables.rs`, `page_table_entry.rs` | PTE format, page table levels |
| Timer | Via `arch::timer` | Timer source, frequency |
| UART | `io/uart.rs` | MMIO addresses (or use serial port on x86) |

**Strategy**:

1. **Move more code into `arch/`**: The 198 lines of RISC-V assembly in `kernel/src/asm/` should move into `arch/src/riscv64/asm/`. x86_64 assembly goes into `arch/src/x86_64/asm/`. The `kernel/src/asm/` module disappears.

2. **Abstract the interrupt controller**: Currently `kernel/src/interrupts/plic.rs` is RISC-V-specific (PLIC = Platform-Level Interrupt Controller). On x86_64, this becomes the APIC/IO-APIC. Create an `InterruptController` abstraction in `arch/` that both architectures implement.

3. **Parameterize page tables**: The current Sv39 (3-level, 39-bit VA) page table code hardcodes the level count and PTE format. x86_64 uses 4-level paging (48-bit VA) with a different PTE bit layout. The page table walker in `page_tables.rs` should be parameterized by:
   - Number of levels (3 for Sv39, 4 for x86_64)
   - PTE bit layout (handled by `PageTableEntry` being arch-specific)
   - Page size (both use 4KB, so this stays the same)

4. **Keep syscalls shared**: The syscall interface is Linux-compatible on both architectures. Syscall numbers differ (RISC-V uses the "new" Linux syscall numbers, x86_64 uses the traditional ones), but the implementations in `*_ops.rs` are architecture-independent. The `headers/` crate already generates syscall numbers from Linux headers -- it just needs to generate from the correct arch's headers.

5. **Device discovery**: RISC-V uses device tree (FDT). x86_64 uses ACPI. Both produce the same information (memory map, PCI ranges, interrupt routing). Abstract this behind a `PlatformInfo` structure populated at boot time.

### 3.4 Build System Changes for Dual Architecture

With the recommended "Nix + just (extended now), xtask later" approach:

**Phase 1 (just + ARCH variable)**:
```just
ARCH := env("ARCH", "riscv64")

kernel_target := if ARCH == "riscv64" { "riscv64gc-unknown-none-elf" } else { "x86_64-unknown-none" }
qemu_cmd := if ARCH == "riscv64" { "qemu-system-riscv64" } else { "qemu-system-x86_64" }
linker_script := "kernel/" + ARCH + ".ld"

build: build-userspace
    CARGO_BUILD_TARGET={{kernel_target}} cargo build --release
```

Additional changes:
- Per-arch linker scripts: `kernel/riscv64.ld`, `kernel/x86_64.ld`
- Per-arch QEMU wrapper or parameterized `qemu_wrapper.sh`
- Per-arch `.cargo/config-{arch}.toml` or dynamic CARGO_BUILD_TARGET
- Userspace: x86_64-unknown-linux-musl target (trivial, just change the target triple)
- headers/build.rs: select Linux headers by architecture (already uses `asm/unistd.h` which is arch-specific)

**Phase 2 (xtask)**:
```rust
// xtask/src/main.rs
enum Arch { Riscv64, X86_64 }

fn build(arch: Arch) -> Result<()> {
    build_userspace(arch)?;
    build_kernel(arch)?;
    if arch == Arch::Riscv64 {
        patch_symbols(arch)?;  // only needed for RISC-V currently
    }
    Ok(())
}
```

### 3.5 How Other Rust OS Projects Handle This

**Hermit OS** (closest comparison):
- Single Cargo crate, arch code in `src/arch/{x86_64,aarch64,riscv64}/`
- Each arch subdirectory has `kernel/`, `mm/` (memory management) subdirectories
- Uses `cargo xtask build --arch x86_64` for the build command
- Uses `#[cfg(target_arch)]` for dispatch at the source level
- Supports x86_64, aarch64, and riscv64
- Repository: https://github.com/hermit-os/kernel

**Redox OS**:
- Uses Make (Makefile) with modular includes in `mk/`
- Architecture selected via `ARCH` environment variable or `.config` file
- Supports x86_64, i586, aarch64, riscv64gc
- Cookbook recipe system for building 2000+ packages
- Much larger project (full OS with GUI), so the heavyweight build system is justified
- Repository: https://github.com/redox-os/redox

**Theseus OS**:
- Uses Make as top-level, invoking Cargo underneath
- Primarily x86_64 only (aarch64 experimental)
- Make handles the "assemble object files, link nano_core, generate ISO" pipeline
- Repository: https://github.com/theseus-os/Theseus

**Blog OS (phil-opp)**:
- Pure Cargo with the `bootloader` crate
- x86_64 only
- No external build system needed -- `cargo build` produces the kernel, `bootloader` crate creates the disk image
- Simplest approach but only works for single-arch

**Takeaway**: Hermit OS is the best model for Solaya. Similar scope, multi-arch, Rust-only, uses xtask. Follow their pattern.

## 4. Development Environment

### 4.1 Should Nix Stay?

**Yes, as the primary environment**. The benefits are too significant to abandon:
- Exact toolchain pinning (Rust nightly date, GCC version, QEMU version, musl build options)
- Cross-compilation "just works" (musl, dash, coreutils for RISC-V)
- No "works on my machine" problems
- direnv integration means developers do not even notice they are using Nix

**But**: Nix should not be the only option. Contributors who cannot or will not install Nix need an alternative.

### 4.2 Docker/Devcontainer as Alternative

Add a `Dockerfile` and `.devcontainer/devcontainer.json` as a second-tier option:

```dockerfile
FROM ubuntu:24.04
RUN apt-get update && apt-get install -y \
    qemu-system-riscv64 qemu-system-x86 \
    gcc-riscv64-linux-gnu binutils-riscv64-linux-gnu \
    curl git just
# Install Rust nightly with riscv64 target
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
    --default-toolchain nightly-2026-03-02 -y
RUN rustup target add riscv64gc-unknown-none-elf riscv64gc-unknown-linux-musl
```

**Pros**: Works on Windows (WSL2), macOS (Docker Desktop), Linux. VS Code devcontainers provide one-click setup. GitHub Codespaces support.

**Cons**: Docker images are less reproducible than Nix (apt packages float). Pre-built musl/dash for RISC-V would need to be cached as build artifacts or built inside the container (slow). Cannot match Nix's exact reproducibility.

**Recommendation**: Provide a Dockerfile for quick onboarding and CI portability. Keep Nix as the "source of truth" for exact versions. Potentially use Nix to build the Docker image (best of both worlds, as the nix2docker ecosystem supports this).

### 4.3 AI Agent Considerations

The current setup is already quite good for AI agents:
- MCP server provides `build_kernel`, `boot_qemu`, `send_command`, `screenshot` tools
- `just` commands are simple and documented in CLAUDE.md
- System tests provide a programmatic test harness

For multi-arch, the MCP tools need:
- An `arch` parameter on `build_kernel` and `boot_qemu`
- The QEMU wrapper needs to know which architecture to launch
- Screenshot tool works regardless of architecture (QEMU VNC/framebuffer is arch-independent)

### 4.4 x86_64 with KVM for Fast Testing

When developing on an x86_64 host:
- RISC-V tests run in QEMU with software emulation (slow but functional)
- x86_64 tests can run in QEMU with KVM acceleration (near-native speed)

This is a significant advantage of adding x86_64 support: the edit-compile-test cycle becomes much faster. KVM-accelerated QEMU boots in milliseconds vs seconds for emulated RISC-V.

To enable: `qemu-system-x86_64 -enable-kvm -cpu host` (instead of software emulation). The QEMU wrapper already supports dynamic flags; adding `-enable-kvm` when the host arch matches the target arch is straightforward.

## 5. CI/CD Pipeline

### 5.1 Current State

The current CI (`ci.yml`) runs on self-hosted runners with Nix:
- Jobs: build, fmt, clippy, unit-test, miri, system-test
- All jobs depend on `build` completing first
- Uses `direnv export gha` to load the Nix environment
- Concurrency group prevents parallel CI runs

This works but is fragile (self-hosted runners require maintenance) and not accessible to external contributors.

### 5.2 Target CI Pipeline

```
                    +--------+
                    | build  |
                    +---+----+
                        |
          +-------------+-------------+
          |             |
     +----v----+  +-----v-----+
     |   fmt   |  |  clippy   |
     +---------+  +-----------+
          |             |
     +----v----+  +-----v-----+
     |  miri   |  | unit-test |
     +---------+  +-----------+
                        |
                  +-----v-------+
                  | system-test |
                  +-------------+
```

For multi-architecture, this becomes a build matrix:

```yaml
strategy:
  matrix:
    arch: [riscv64, x86_64]
    include:
      - arch: riscv64
        target: riscv64gc-unknown-none-elf
        qemu: qemu-system-riscv64
      - arch: x86_64
        target: x86_64-unknown-none
        qemu: qemu-system-x86_64
```

**Debug vs Release**: Currently only release is built (with debug symbols and assertions enabled). This is fine -- the release profile already has `debug = true` and `debug-assertions = true`, so it behaves like a debug build with optimizations. No need for a separate debug matrix dimension.

### 5.3 Running QEMU in CI

**Self-hosted runners (current)**: QEMU runs directly. Works well, but limits scalability.

**GitHub-hosted runners**: As of 2025, GitHub Actions Linux runners support nested virtualization (KVM) on ubuntu-latest. This means:
- x86_64 QEMU tests can use KVM acceleration on GitHub-hosted runners
- RISC-V QEMU tests run in software emulation (slower but works)

```yaml
system-test:
  runs-on: ubuntu-latest  # or ubuntu-24.04 for KVM support
  steps:
    - name: Enable KVM
      run: |
        echo 'KERNEL=="kvm", GROUP="kvm", MODE="0666"' | sudo tee /etc/udev/rules.d/99-kvm.rules
        sudo udevadm control --reload-rules && sudo udevadm trigger
    - name: Install QEMU
      run: sudo apt-get install -y qemu-system-riscv64 qemu-system-x86
```

**Recommendation**: Migrate to GitHub-hosted runners for most jobs. Keep self-hosted as a fallback for jobs that need specific hardware or for faster iteration.

### 5.4 Caching Strategy

**Nix caching**:
- Use `DeterminateSystems/magic-nix-cache-action` for free, zero-config Nix binary caching on GitHub Actions. Saves 30-50% of CI time.
- Alternatively, use Cachix for a shared binary cache across developers and CI.

**Cargo caching**:
- Cache `~/.cargo/registry/`, `~/.cargo/git/`, and `target/` between runs.
- Use `sccache` with the GitHub Actions cache backend to cache individual compilation units. This provides cross-job caching (e.g., clippy and test share compiled dependencies).
- The `Swatinem/rust-cache` action handles common Cargo caching patterns.

**Combined strategy**:
```yaml
- uses: DeterminateSystems/nix-installer-action@main
- uses: DeterminateSystems/magic-nix-cache-action@main
- uses: mozilla-actions/sccache-action@v0.0.7
  with:
    cache-backend: gha
```

### 5.5 Future: Compliance Testing (LTP)

Once the OS reaches sufficient Linux compatibility, add Linux Test Project (LTP) runs:
- Build LTP for the target architecture (cross-compile with musl)
- Run a subset of LTP syscall tests inside QEMU
- Start with basic tests (open, read, write, mmap, fork, exec) and expand
- This is a long-term goal, not needed for initial multi-arch support

## 6. Recommendation: What to Do and When

### Phase 1: Parameterize the Build (Do Now)

**Goal**: Make the existing build system architecture-aware without changing tools.

1. Add `ARCH` variable to justfile with `riscv64` as default
2. Parameterize `qemu_wrapper.sh` to accept architecture
3. Move the linker script reference to be arch-dependent: `kernel/riscv64.ld` (rename from `qemu.ld`)
4. Parameterize the symbol-patching step in justfile
5. Update `.cargo/config.toml` to not hardcode `riscv64` as the default target (use `CARGO_BUILD_TARGET` env var set by justfile instead)
6. Add a Dockerfile for non-Nix users

**Estimated effort**: 1-2 days.

### Phase 2: Restructure arch/ for Real Multi-Arch (When Starting x86_64)

**Goal**: Make the arch crate support x86_64 as a real target, not just stubs.

1. Add `arch/src/x86_64/` directory structure mirroring `riscv64/`
2. Define the architecture abstraction boundary: which functions must each arch implement
3. Move `kernel/src/asm/` into `arch/src/riscv64/asm/`
4. Move `kernel/src/interrupts/plic.rs` behind an arch-gated interrupt controller abstraction
5. Parameterize page table code (level count, PTE format)
6. Add x86_64 linker script (`kernel/x86_64.ld`)
7. Integrate the `bootloader` crate for x86_64 boot
8. Create x86_64 QEMU configuration (OVMF/UEFI firmware)

**Estimated effort**: 2-4 weeks for basic x86_64 boot to shell prompt.

### Phase 3: Migrate to xtask (When justfile Becomes Painful)

**Goal**: Replace justfile with a Rust xtask binary for type-safe build orchestration.

1. Create `xtask/` crate in the workspace
2. Port justfile recipes to Rust functions
3. Add `cargo xtask build --arch {riscv64,x86_64}`, `cargo xtask run`, `cargo xtask test`
4. Retire justfile (or keep as thin wrapper: `just build` calls `cargo xtask build`)
5. Update CLAUDE.md and MCP tools to use new commands

**Trigger**: Migrate when the justfile exceeds ~200 lines or when the conditional logic for 2+ architectures becomes unmanageable.

**Estimated effort**: 3-5 days.

### Phase 4: CI Modernization (After Multi-Arch Works)

**Goal**: Portable CI that external contributors can use.

1. Add GitHub-hosted runner support (ubuntu-latest with Nix installer)
2. Add Nix binary caching (magic-nix-cache or Cachix)
3. Add sccache for Cargo compilation caching
4. Build matrix: riscv64 + x86_64
5. x86_64 system tests with KVM acceleration
6. Keep self-hosted runners as optional fast path

**Estimated effort**: 2-3 days.

### What NOT to Do

- **Do not adopt Bazel, CMake, Meson, or Kbuild**. The Cargo + orchestration layer approach is the right one for a Rust OS. Every successful Rust OS project uses this pattern.
- **Do not rewrite the build system before adding x86_64**. The current system works. Refactor it incrementally as needs arise.
- **Do not abandon Nix**. It provides too much value for toolchain management. Add Docker as a complement, not a replacement.
- **Do not try to unify RISC-V and x86_64 page table code into a single generic implementation**. The formats are different enough that `cfg`-gated separate implementations (sharing the walker logic via const generics for level count) is cleaner than a fully generic approach.

---

**Sources consulted**:
- [Hermit OS kernel](https://github.com/hermit-os/kernel) -- multi-arch Rust unikernel with xtask build system
- [Redox OS](https://github.com/redox-os/redox) -- multi-arch Rust OS with Make + Cookbook build system
- [Redox OS Build System Reference](https://doc.redox-os.org/book/build-system-reference.html)
- [Theseus OS](https://github.com/theseus-os/Theseus) -- Rust OS with Make + Cargo build
- [cargo-xtask pattern](https://github.com/matklad/cargo-xtask) -- official documentation
- [rust-osdev/bootloader](https://github.com/rust-osdev/bootloader) -- x86_64 Rust bootloader crate
- [Magic Nix Cache](https://github.com/DeterminateSystems/magic-nix-cache-action) -- GitHub Actions Nix caching
- [sccache in GitHub Actions](https://depot.dev/blog/sccache-in-github-actions) -- Rust compilation caching
- [GitHub Actions KVM support](https://github.com/actions/runner-images/issues/7541) -- nested virtualization on hosted runners
