# Solaya - AI Agent Reference

RISC-V 64-bit hobby OS kernel written in Rust. No third-party runtime dependencies.

## Quick Commands

```bash
just run          # Build and run in QEMU
just test         # Run unit tests + system tests
just ci           # Run all CI checks (clippy, fmt, features, tests, miri)
just build        # Build kernel with userspace
just kani         # Run Kani model checking proofs
just system-test  # Run only system tests
just unit-test    # Run only unit tests
just clippy       # Run linter
just miri         # Run miri (detects undefined behavior)
just check-features  # Verify all feature combinations compile
just mcp-server   # Build MCP server
just disassm      # Disassemble kernel
just addr2line 0x1234  # Get source line for kernel address
just attach       # Attach GDB to running QEMU
just stress-system-test       # Run system tests 5x in a row
just loop-system-test TEST    # Run one system test in a loop until failure
just deadlock-hunt            # Run system tests in loop with GDB enabled
```

## Project Structure

```
sys/              # Hardware abstraction + unsafe primitives (CSR, SBI, spinlock, etc.)
kernel/           # Main kernel (RISC-V 64-bit, no_std, #![deny(unsafe_code)])
userspace/        # Userspace programs (musl libc)
common/           # Shared no_std library
system-tests/     # Integration tests (run on x86, test via QEMU)
qemu-infra/       # Shared QEMU communication library (used by system-tests + mcp-server)
mcp-server/       # MCP server for AI agent interaction with QEMU
headers/          # Linux C header bindings via bindgen
doc/ai/           # Detailed AI documentation (see OVERVIEW.md)
```

## Key Kernel Subsystems

| Directory | Purpose |
|-----------|---------|
| kernel/src/memory/ | Page allocator, page tables, heap |
| kernel/src/processes/ | Process, thread, scheduler, signals |
| kernel/src/syscalls/ | syscall handlers |
| kernel/src/interrupts/ | Trap handling, PLIC, timer |
| kernel/src/fs/ | VFS layer (tmpfs, procfs, devfs) |
| kernel/src/net/ | Network stack (UDP, TCP) |
| kernel/src/drivers/virtio/ | VirtIO drivers (network, block) |
| kernel/src/io/ | UART, TtyDevice (terminal subsystem) |

## Debugging

### Logging Macros
- `info!()` - Always printed. Use sparingly (clutters user output).
- `debug!()` - Conditional. Enable per-module. Leave in code.
- `warn!()` - Always printed.

### Enable Debug Output for a Module
Edit `kernel/src/logging/configuration.rs`:
```rust
// Add to LOG_FOLLOWING_MODULES to enable:
const LOG_FOLLOWING_MODULES: &[&str] = &["kernel::processes::scheduler"];

// Or remove from DONT_LOG_FOLLOWING_MODULES if blocked there
```

### Syscall Tracer
Edit `kernel/src/syscalls/trace_config.rs` to add process names:
```rust
pub const TRACED_PROCESSES: &[&str] = &["prog2"];
```
All syscalls by those processes are logged with `[SYSCALL ENTER]` / `[SYSCALL EXIT]` lines showing syscall name, tid, formatted args, and return value or errno. Metadata is auto-generated from the `linux_syscalls!` macro — no manual table needed. `prog2` is always traced (tested in `system-tests/src/tests/syscall_tracer.rs`).

### GDB Debugging
```bash
just debug        # Start QEMU + GDB in tmux
just debugf FUNC  # Debug with breakpoint on function
```

### GDB MCP Server (Programmatic Debugging)

An MCP server exposes GDB as tools for Claude Code. Start QEMU first (`just run`), then use the `gdb_*` tools.

```
gdb_mcp_server/       # Python MCP server (pygdbmi + FastMCP)
    server.py         # Tool definitions
    gdb_session.py    # GDBSession wrapping pygdbmi
```

Key tools: `gdb_connect`, `gdb_backtrace`, `gdb_breakpoint`, `gdb_continue`, `gdb_step`, `gdb_next`, `gdb_print`, `gdb_registers`, `gdb_execute`.

## Testing Strategy

### System Tests (Preferred for AI iteration)
Located in `system-tests/src/tests/`. Run the OS in QEMU and interact via stdin/stdout.

```bash
# Run all system tests
just system-test

# Run specific test
cargo nextest run --release --manifest-path system-tests/Cargo.toml \
    --target x86_64-unknown-linux-gnu test_name
```

### Writing Throw-Away Tests
Add to `system-tests/src/tests/basics.rs` or create new test file:
```rust
#[tokio::test]
async fn my_test() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("prog1").await?;
    assert_eq!(output, "expected");
    Ok(())
}
```

### Unit Tests
Kernel unit tests use `#[test_case]` macro (custom test framework).

### Kani Model Checking
Kani verifies correctness of pure functions via bounded model checking. Run with `just kani`. Existing proofs are in `kernel/src/memory/address.rs`, `kernel/src/memory/page_table_entry.rs`, and `kernel/src/klibc/util.rs`.

When adding or modifying pure logic (bit manipulation, arithmetic, data structure invariants, encoding/decoding), add Kani proof harnesses. Good candidates: functions with bitwise operations, numeric conversions, invariants that must hold for all inputs. Not suited for: code requiring hardware, allocators, or complex kernel state.

```rust
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    #[kani::proof]
    fn my_roundtrip_proof() {
        let input: u64 = kani::any();
        let encoded = encode(input);
        let decoded = decode(encoded);
        assert_eq!(input, decoded);
    }
}
```

The `sys` crate provides no-op stubs for non-riscv64 targets so Kani can compile kernel code without hardware dependencies.

## Adding Userspace Programs

1. Create `userspace/src/bin/myprogram.rs`
2. Run `just build` (automatically embedded in kernel)
3. Available in shell as `myprogram`

## Key Files Quick Reference

| Purpose | File |
|---------|------|
| Kernel entry | kernel/src/main.rs |
| CSR access | sys/src/riscv64/cpu.rs |
| SBI calls | sys/src/riscv64/sbi/ |
| Syscall dispatch | kernel/src/syscalls/linux.rs (thin trait methods) |
| Syscall impls | kernel/src/syscalls/*_ops.rs (io, ioctl, fs, mm, signal, net, time, id, process, exec) |
| Process struct | kernel/src/processes/process.rs |
| Scheduler | kernel/src/processes/scheduler.rs |
| Page tables | kernel/src/memory/page_tables.rs |
| Address types | kernel/src/memory/address.rs, kernel/src/pci/address.rs |
| Trap handler | kernel/src/interrupts/trap.rs |
| QEMU infra | qemu-infra/src/qemu.rs |
| MCP server | mcp-server/src/server.rs |
| Log config | kernel/src/logging/configuration.rs |
| Signals | kernel/src/processes/signal.rs |
| Syscall tracer config | kernel/src/syscalls/trace_config.rs |

## Sys Crate

The `sys` crate (`sys/`) encapsulates hardware abstraction and unsafe primitives. The kernel crate uses `#![deny(unsafe_code)]` with `#[allow(unsafe_code)]` on modules that still need it.

**Modules in sys:**
- `riscv64/` / `stub/` — CPU, SBI, timer, trap causes (conditional compilation)
- `spinlock` — Bare spinlock (no deadlock detection, no interrupt guard)
- `array_vec` — Fixed-capacity stack vector
- `runtime_initialized` — One-time late-initialization
- `raw_ptr` — Safe wrappers for common unsafe pointer operations

The kernel wraps `sys::spinlock::Spinlock` with `klibc::Spinlock` that adds interrupt guard management and deadlock detection.

## Kernel Features

The kernel supports cargo features for compile-time subsystem selection:

```toml
[features]
default = ["net", "tcp", "udp", "virtio-net", "virtio-blk", "smp", "ext2"]
net = []           # Base networking stack
tcp = ["net"]      # TCP protocol
udp = ["net"]      # UDP protocol
virtio-net = ["net"]  # VirtIO network driver
virtio-blk = []    # VirtIO block driver
smp = []           # Multi-core support
ext2 = ["virtio-blk"]  # EXT2 filesystem
```

Use `just check-features` (runs `cargo hack check --each-feature`) to verify feature combinations compile. Use `cargo build --release --no-default-features` to build a minimal kernel.

## Detailed Documentation

See `doc/ai/OVERVIEW.md` for comprehensive subsystem documentation including:
- Per-CPU struct architecture (`kernel/src/cpu.rs`) for multi-core support
- Async syscall model
- Memory layout and page tables

## MCP Server

The MCP server (`mcp-server/`) lets AI agents interact with Solaya running in QEMU over the Model Context Protocol.

### Build & Run
```bash
just mcp-server                    # Build
./mcp-server/target/x86_64-unknown-linux-gnu/release/mcp-server  # Run (stdio transport)
```

### Available Tools

| Tool | Description |
|------|-------------|
| `boot_qemu` | Start QEMU with Solaya. Options: network, smp, force. |
| `shutdown_qemu` | Send exit to shell and wait for QEMU to exit. |
| `get_status` | Check if QEMU is running. |
| `send_command` | Send shell command, return output. |
| `send_input` | Send raw input, wait for custom marker. |
| `send_ctrl_c` | Send Ctrl+C, wait for prompt. |
| `read_output` | Non-blocking read of available output. |
| `build_kernel` | Run `just build`, optionally `just clippy`. |
| `run_system_tests` | Run `just system-test` or a specific test. |

### Claude Code Integration
Already configured in `.mcp.json` at the project root. Claude Code picks it up automatically on startup.

## Licensing and Clean-Room Policy

Solaya is licensed under **MIT**. To keep it that way:

**Never reference Linux kernel source code.** All implementations must be written from scratch based on public specifications, hardware documentation, RFCs, and man pages — not by reading or porting Linux kernel code (which is GPL-2.0). This applies to drivers, syscalls, filesystems, and all other subsystems.

**Loading Linux kernel modules (.ko) is allowed.** Implementing the interfaces to load and run GPL-licensed kernel modules is fine — that's interface compatibility, not a derivative work.

**Third-party driver contributions.** If external contributors want to port a Linux driver to Solaya, the licensing implications are theirs to manage. We do not accept code ported from GPL sources into the MIT-licensed codebase.

## Development Guidelines

**Prefer less code.** Achieve the same result with fewer lines. Avoid unnecessary abstractions, helpers for one-time operations, or premature optimization. Simplify existing code when touching it for a feature.

**Fail fast with assertions.** Use `assert!` instead of `debug_assert!`. An inconsistent state in the kernel should panic immediately rather than continue with corrupted data. Crashing early makes bugs easier to diagnose and prevents cascading failures.

**No bloated comments.** Add comments only when explaining invariants or non-obvious logic. Never add comments that restate what the code does, separators, or decorative formatting.

**Use existing utilities.** Before implementing helper functions, check for existing utilities:
- `ByteInterpretable::as_slice()` (kernel/src/klibc/util.rs) - Convert any struct to &[u8]
- `is_power_of_2_or_zero()`, `is_aligned()` (kernel/src/klibc/util.rs) - Common checks

**Reuse Linux/musl header definitions.** Constants and structs from Linux UAPI or musl libc headers must be auto-generated via bindgen in the `headers` crate, not defined manually. Only define types manually when they are not available in any header (e.g., kernel-internal structs like `linux_dirent64`).

**Syscall organization.** New syscalls: add the trait method in `linux.rs` (≤5 lines, delegates to `do_*` helper), implement in the appropriate `*_ops.rs` file grouped by concern. Trivial stubs stay inline.

**Userspace programs must use musl libc.** Never use raw `ecall` assembly in userspace binaries. Declare `extern "C"` functions to bind to musl libc (e.g., `extern "C" { fn fork() -> i32; }`), or use Rust std library functions that call libc internally.

**Commit automatically.** After completing a task, commit without waiting for user intervention. Before committing:
- Remove any dead or unused code introduced by your changes
- The pre-commit hook runs `cargo fmt` and `cargo clippy --fix` automatically; it will block the commit if clippy finds unfixable warnings.

**Commit incrementally.** Commit each small working step toward a larger goal. Include test code in commits. This enables incremental progress verification rather than large, hard-to-debug changesets.

**Keep docs in sync.** Update `CLAUDE.md` and `doc/ai/*` when discovering inconsistencies or implementing new features.

**GitHub issue attribution.** When creating GitHub issues via `gh`, always append this footer to the issue body: `---\n_Created by [Claude Code](https://claude.ai/code)_`

**Network port.** Both system tests and `--net` without an explicit port use dynamic port allocation. Use `--net PORT` to specify a fixed port. See `doc/ai/DEBUGGING.md` for all QEMU wrapper options.
