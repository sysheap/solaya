# Solaya - AI Agent Reference

RISC-V 64-bit hobby OS kernel written in Rust. No third-party runtime dependencies.

## Quick Commands

```bash
just run          # Build and run in QEMU
just test         # Run unit tests + system tests
just ci           # Run all CI checks (clippy, fmt, tests, miri)
just build        # Build kernel with userspace
just system-test  # Run only system tests
just unit-test    # Run only unit tests
just clippy       # Run linter
just miri         # Run miri (detects undefined behavior)
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
boot/             # Entry point wrapper (#[no_mangle] fns, calls into kernel)
kernel/           # Main kernel logic (RISC-V 64-bit, no_std, #![forbid(unsafe_code)])
crates/abi/        # Shared no_std ABI types (was "common")
crates/headers/    # Linux/musl UAPI bindgen bindings
crates/klib/       # Hardware-free utilities (array_vec, runtime_initialized, util, ...)
crates/hal/        # Hardware abstraction: CSR/SBI/timer, MMIO, Spinlock, page_table, PhysAddr/VirtAddr
crates/mm/         # Page allocator + heap allocator (generic, host-testable)
crates/console/    # UART driver + log macros
crates/driver-api/ # Trait-only driver API (Block/Net/Char/Display/Input/Rng/IrqHandler/DmaBuffer/BusContext)
crates/drivers/    # Concrete drivers (virtio/*, dwmac, bochs). Never depends on `solaya`.
crates/kernel/     # Main kernel logic. Crate name is `solaya`. #![forbid(unsafe_code)].
boot/              # Entry point wrapper (calls into `solaya`)
userspace/         # Userspace programs (musl libc)
system-tests/      # Integration tests (run on x86, test via QEMU)
qemu-infra/        # Shared QEMU communication library (used by system-tests + mcp-server)
mcp-server/        # MCP server for AI agent interaction with QEMU
doc/ai/            # Detailed AI documentation (see OVERVIEW.md, DRIVER_ARCHITECTURE.md)
```

## Key Kernel Subsystems

| Directory | Purpose |
|-----------|---------|
| crates/hal/src/memory/ | PhysAddr, VirtAddr, Page, PageTable types |
| crates/hal/src/mmio.rs + spinlock.rs | MMIO, Spinlock primitives |
| crates/hal/src/validated_ptr.rs | ValidatedPtr for userspace pointer checks |
| crates/mm/src/ | Page allocator, heap allocator (generic) |
| crates/console/src/ | Log macros and UART driver |
| crates/klib/src/ | Utility functions, non_empty_vec, runtime_initialized, sizes |
| crates/driver-api/src/ | Driver traits (Block/Net/Char/Display/Input/Rng/Irq/DMA/BusContext) |
| crates/drivers/src/virtio/ | virtio-blk, virtio-net, virtio-input, virtio-rng, virtqueue |
| crates/drivers/src/dwmac/ | Synopsys DWMAC + StarFive JH7110 init |
| crates/drivers/src/bochs_display.rs | QEMU Bochs VBE framebuffer |
| crates/kernel/src/memory/ | RootPageTableHolder, kernel mappings |
| crates/kernel/src/processes/ | Process, thread, scheduler, signals |
| crates/kernel/src/syscalls/ | Syscall handlers |
| crates/kernel/src/interrupts/ | Trap handling, PLIC (+ IrqController impl) |
| crates/kernel/src/fs/ | VFS layer (tmpfs, procfs, devfs, ext2) |
| crates/kernel/src/net/ | Network stack (UDP, TCP) |
| crates/kernel/src/drivers/ | Thin orchestrator: PCI/DT enumeration, typed registries. No policy. |
| crates/kernel/src/init/ | Policy layer: mount ext2, spawn network_rx_task |
| crates/kernel/src/pci/ | PCI enumeration + PciBusContext |
| crates/kernel/src/device_tree.rs | Device-tree parser + DtBusContext |
| crates/kernel/src/platform/ | Emergency reset (JH7110 syscon, SBI fallback) |
| crates/kernel/src/io/ | UART extensions, TtyDevice |

## Debugging

### Logging Macros
- `info!()` - Always printed. Use sparingly (clutters user output).
- `debug!()` - Conditional. Enable per-module. Leave in code.
- `warn!()` - Always printed.

### Enable Debug Output for a Module
Edit `crates/console/src/logging/configuration.rs`:
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

## Adding Userspace Programs

1. Create `userspace/src/bin/myprogram.rs`
2. Run `just build` (automatically embedded in kernel)
3. Available in shell as `myprogram`

## Key Files Quick Reference

| Purpose | File |
|---------|------|
| Boot entry points | boot/src/main.rs |
| Kernel init | crates/kernel/src/lib.rs |
| CPU struct (base) | crates/hal/src/cpu.rs |
| CPU struct (full) | crates/kernel/src/cpu.rs |
| CSR access | crates/hal/src/riscv64/cpu.rs |
| SBI calls | crates/hal/src/riscv64/sbi/ |
| Assembly (boot, trap) | crates/hal/src/riscv64/asm/ |
| Syscall dispatch | crates/kernel/src/syscalls/linux.rs (thin trait methods) |
| Syscall impls | crates/kernel/src/syscalls/*_ops.rs |
| Process struct | crates/kernel/src/processes/process.rs |
| Scheduler | crates/kernel/src/processes/scheduler.rs |
| Page table types | crates/hal/src/memory/page_table.rs |
| Page table mapping | crates/kernel/src/memory/page_tables.rs |
| Address types | crates/hal/src/memory/address.rs, crates/kernel/src/pci/address.rs |
| Page allocator | crates/mm/src/page_allocator.rs |
| Heap allocator | crates/mm/src/heap.rs |
| Trap handler | crates/kernel/src/interrupts/trap.rs |
| Driver API traits | crates/driver-api/src/lib.rs (+ bus.rs, dma.rs) |
| Driver enumeration | crates/kernel/src/drivers/mod.rs |
| Driver registries | crates/kernel/src/drivers/registry.rs |
| Policy layer (mount/spawn) | crates/kernel/src/init/mod.rs |
| VirtIO drivers | crates/drivers/src/virtio/ |
| DWMAC driver | crates/drivers/src/dwmac/ |
| MMIO type | crates/hal/src/mmio.rs |
| Spinlock | crates/hal/src/spinlock.rs |
| ValidatedPtr | crates/hal/src/validated_ptr.rs |
| Logging | crates/console/src/ |
| Log config | crates/console/src/logging/configuration.rs |
| QEMU infra | qemu-infra/src/qemu.rs |
| MCP server | mcp-server/src/server.rs |
| Signals | crates/kernel/src/processes/signal.rs |
| Syscall tracer config | crates/kernel/src/syscalls/trace_config.rs |

## Detailed Documentation

See `doc/ai/OVERVIEW.md` for comprehensive subsystem documentation including:
- Per-CPU struct architecture (`kernel/src/cpu.rs`) for multi-core support
- Async syscall model
- Memory layout and page tables

## Codebase Navigation — MUST USE indxr MCP tools

An MCP server called `indxr` is available. **Always use indxr tools before the Read tool.** Do NOT read full source files as a first step — use the MCP tools to explore, then read only what you need.

### Token savings reference

| Action | Approx tokens | When to use |
|--------|--------------|-------------|
| `get_tree` | ~200-400 | First: understand directory layout |
| `get_file_summary` | ~200-400 | Understand a file without reading it |
| `batch_file_summaries` | ~400-1200 | Summarize multiple files in one call |
| `get_file_context` | ~400-600 | Understand dependencies and reverse deps |
| `lookup_symbol` | ~100-200 | Find a specific function/type across codebase |
| `search_signatures` | ~100-300 | Find functions by signature pattern |
| `search_relevant` | ~200-400 | Find files/symbols by concept or partial name (supports `kind` filter) |
| `explain_symbol` | ~100-300 | Everything to USE a symbol without reading its body |
| `get_public_api` | ~200-500 | Public API surface of a file or module |
| `get_callers` | ~100-300 | Who references this symbol (imports + signatures) |
| `get_related_tests` | ~100-200 | Find tests for a symbol by naming convention |
| `get_diff_summary` | ~200-500 | Structural changes since a git ref (vs reading raw diffs) |
| `get_hotspots` | ~200-500 | Most complex functions ranked by composite score |
| `get_health` | ~200-400 | Codebase health summary with aggregate complexity metrics |
| `get_type_flow` | ~200-500 | Track which functions produce/consume a type across the codebase |
| `read_source` (symbol) | ~50-300 | Read one function/struct. Supports `symbols` array and `collapse`. |
| `get_token_estimate` | ~100 | Check cost before reading. Supports `directory`/`glob`. |
| `Read` (full file) | **500-10000+** | ONLY when editing or need exact formatting |

### Exploration workflow (follow this order)

1. `search_relevant` — find files/symbols related to your task by concept, partial name, or type pattern. **Start here when you know what you're looking for but not where it is.**
2. `get_tree` — see directory/file layout. Use `path` param to scope to a subtree.
3. `get_file_summary` — get a complete overview of any file without reading it. Use `batch_file_summaries` for multiple files.
4. `get_file_context` — understand a file's reverse dependencies and related files.
5. `lookup_symbol` — find declarations by name across all indexed files.
6. `explain_symbol` — get full interface details for a symbol without reading its body.
7. `search_signatures` — find functions/methods by signature substring.
8. `get_callers` — find who references a symbol.
9. `get_token_estimate` — before deciding to `Read` a file, check how many tokens it costs.
10. `read_source` — read source code by symbol name or line range. Use `symbols` array to read multiple in one call.
11. `get_public_api` — get only public declarations with signatures for a file or directory.
12. `get_related_tests` — find test functions for a symbol.
13. `list_declarations` — list all declarations in a file.
14. `get_imports` — get import statements for a file.
15. `get_stats` — codebase stats: file count, line count, language breakdown.
16. `get_diff_summary` — get structural changes since a git ref.
17. `get_hotspots` — get the most complex functions ranked by composite score.
18. `get_health` — get codebase health summary: aggregate complexity, documentation coverage, test ratio.
19. `get_type_flow` — track where a type flows across function boundaries. Shows producers and consumers.
20. `regenerate_index` — re-index after code changes.

### When to use the Read tool instead
- You need to **edit** a file (Read is required before Edit)
- You need exact formatting/whitespace that `read_source` doesn't preserve
- The file is not a source file (e.g., config files, documentation)

### DO NOT
- Read full source files just to understand what's in them — use `get_file_summary`
- Read full source files to review code — use `get_file_summary` to triage, then `read_source` on specific symbols
- Dump all files into context — use MCP tools to be surgical
- Read a file without first checking `get_token_estimate` if you're unsure about its size
- Use `git diff` to understand changes — use `get_diff_summary` instead

### After making code changes
Run `regenerate_index` to keep INDEX.md current.

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

**Keep docs in sync.** Before starting implementation tasks, check `doc/ai/OVERVIEW.md` for available documentation, read the docs relevant to the task, and update them if the implementation changes. Update `CLAUDE.md` and `doc/ai/*` when discovering inconsistencies or implementing new features.

**GitHub issue attribution.** When creating GitHub issues via `gh`, always append this footer to the issue body: `---\n_Created by [Claude Code](https://claude.ai/code)_`

**Network port.** Both system tests and `--net` without an explicit port use dynamic port allocation. Use `--net PORT` to specify a fixed port. See `doc/ai/DEBUGGING.md` for all QEMU wrapper options.

