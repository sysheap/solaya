# Debugging

## Overview

Debugging tools available:
1. **Logging** - info!, debug!, warn! macros
2. **Backtrace** - Stack unwinding on panic
3. **State Dump** - Heap and process info (Ctrl+D)
4. **GDB** - Interactive debugging via QEMU

## Logging Macros

**File:** `sys/src/logging/mod.rs` (re-exported by kernel)

### info!

Always printed. Use sparingly - clutters user output.

```rust
info!("Message: {}", value);
// Output: [CPU 0][info][kernel::module] Message: value
```

### debug!

Conditionally printed based on module configuration. **Preferred for debugging.**

```rust
debug!("Debug info: {}", value);
// Output (if enabled): [CPU 0][debug][kernel::module] Debug info: value
```

### warn!

Always printed. For warnings.

```rust
warn!("Warning: {}", issue);
// Output: [CPU 0][warn][kernel::module] Warning: issue
```

### print! / println!

Direct output without metadata. Used for user-facing output.

## Enabling Debug Output

**File:** `sys/src/logging/configuration.rs`

```rust
// Modules that should be logged (if empty, nothing logged by debug!)
const LOG_FOLLOWING_MODULES: &[&str] = &[
    "kernel::processes::scheduler",
    "kernel::syscalls",
];

// Modules that should never be logged (overrides LOG_FOLLOWING_MODULES)
const DONT_LOG_FOLLOWING_MODULES: &[&str] = &[
    "kernel::interrupts::trap",
    "kernel::debugging::unwinder",
    "kernel::debugging::symbols",
    "kernel::processes::scheduler",
    "kernel::processes::process_table",
    "kernel::processes::timer",
    "kernel::io::tty_device",
];
```

### Enable Module Logging

1. Add module path to `LOG_FOLLOWING_MODULES`:
```rust
const LOG_FOLLOWING_MODULES: &[&str] = &[
    "kernel::syscalls::linux",
];
```

2. Or remove from `DONT_LOG_FOLLOWING_MODULES` if blocked

### Module Path Format

- Full path: `kernel::processes::scheduler`
- Partial prefix: `kernel::processes` (matches all submodules)
- Root: `kernel` (matches everything)

## Debug Statements

Leave debug! statements in code - they're disabled by default:

```rust
fn my_function() {
    debug!("Entering my_function");
    // ... code ...
    debug!("Result: {:?}", result);
}
```

## State Dump (Ctrl+D)

Press Ctrl+D in QEMU to dump current state:

**File:** `kernel/src/debugging/mod.rs`

```rust
pub fn dump_current_state() {
    // Heap allocation stats
    info!("Heap allocated: {:.2} MiB", allocated_size_heap);

    // Page allocator stats
    info!("Page allocator {} / {} used", used_heap_pages, total_heap_pages);

    // Process table dump
    process_table::THE.try_with_lock(|pt| pt.dump());

    // Current thread info
    Cpu::current_thread().try_with_lock(|t| {
        info!("Current Thread: {}", *t);
    });
}
```

## Backtrace

**File:** `kernel/src/debugging/backtrace.rs`

Stack unwinding on panic using DWARF debug info.

### Initialize

Called in `kernel_init()`:
```rust
backtrace::init();
```

### Get Backtrace

```rust
backtrace::print_backtrace();
```

### Symbol Resolution

**File:** `kernel/src/debugging/symbols.rs`

Debug symbols are embedded in kernel binary during build (see BUILD.md).

```rust
pub fn resolve_address(addr: usize) -> Option<&'static str>
```

## GDB Debugging

### Start Debug Session

```bash
just debug           # Start QEMU + GDB in tmux
just debugf FUNC     # Debug with breakpoint on function
just debuguf BIN FUNC  # Debug userspace binary
```

### QEMU Wrapper Options

The `./qemu_wrapper.sh` script provides flags for debugging:

| Flag | Description |
|------|-------------|
| `--gdb [PORT]` | Enable GDB server (default: dynamic port, written to `.gdb-port`) |
| `--wait` | Pause CPU until GDB attaches |
| `--log` | Log QEMU events to `/tmp/solaya.log` |
| `--net [PORT]` | Enable VirtIO network (default: dynamic port) |
| `--smp` | Enable all CPU cores |
| `--capture` | Capture network traffic to `network.pcap` |

Flags are set in `.cargo/config.toml` for `just run`.

**Note:** When `--net` or `--gdb` is used without a port argument, a dynamic port is allocated automatically, allowing multiple QEMU instances to run simultaneously. The GDB port is written to `.gdb-port` in the project root.

### Manual GDB

Terminal 1:
```bash
cargo run --release -- --wait
```

Terminal 2:
```bash
pwndbg -ex "target remote :$(cat .gdb-port)" target/riscv64gc-unknown-none-elf/release/kernel
```

### Useful GDB Commands

```gdb
# Set breakpoint
hbreak kernel_init
hbreak handle_syscall

# Continue execution
c

# Step instruction
si

# Print registers
info registers

# Print memory
x/10x $sp

# Print backtrace
bt
```

### Address to Line

```bash
just addr2line 0x80001234
# Output: function_name at file.rs:123
```

Or:
```bash
riscv64-unknown-linux-musl-addr2line -f -p -i -C -e \
    target/riscv64gc-unknown-none-elf/release/kernel 0x80001234
```

## Disassembly

```bash
just disassm  # Output to stdout

# Pipe to less for navigation
just disassm | less

# Or save to file
just disassm > kernel.dis
```

## GDB MCP Server (Programmatic Debugging)

A Python MCP server (`gdb_mcp_server/`) exposes GDB debugging as tools for AI assistants like Claude Code.

### Architecture

```
Claude Code <--stdio--> GDB MCP Server <--pygdbmi (MI)--> GDB <--TCP--> QEMU GDB stub
```

### Setup

Dependencies are provided via Nix (`pygdbmi`, `mcp`). The server is configured in `.mcp.json`.

### Usage

1. Start QEMU: `just run`
2. Claude Code automatically starts the MCP server via `.mcp.json`
3. Use `gdb_connect` to attach to the running kernel
4. Debug with `gdb_breakpoint`, `gdb_continue`, `gdb_backtrace`, etc.

### Available Tools

| Tool | Description |
|------|-------------|
| `gdb_connect` | Start GDB, load kernel symbols, connect to QEMU |
| `gdb_disconnect` | Stop GDB session |
| `gdb_backtrace` | Get stack trace |
| `gdb_breakpoint` | Set breakpoint (hardware by default for RISC-V) |
| `gdb_continue` | Resume execution |
| `gdb_step` | Step into |
| `gdb_next` | Step over |
| `gdb_print` | Evaluate expression |
| `gdb_execute` | Run arbitrary GDB CLI command |
| `gdb_registers` | Read CPU registers |
| `gdb_locals` | Get local variables |
| `gdb_examine` | Examine memory |
| `gdb_breakpoint_list` | List breakpoints |
| `gdb_breakpoint_delete` | Delete breakpoint |
| `gdb_interrupt` | Pause running kernel |
| `gdb_finish` | Run until function returns |
| `gdb_threads` | List threads/harts |
| `gdb_select_thread` | Switch hart |
| `gdb_frame` | Select stack frame |

### Manual Testing

```bash
# With MCP inspector
mcp dev "python -m gdb_mcp_server"

# Direct run
python -m gdb_mcp_server
```

## Common Debug Scenarios

### Crash/Panic

1. Check panic message for location
2. Use `just addr2line <address>` for addresses in backtrace
3. Enable debug! for relevant modules
4. Add debug! statements around suspected code

### Syscall Issues

1. Enable `kernel::syscalls` in LOG_FOLLOWING_MODULES
2. Check trap_frame contents in handle_syscall
3. Verify userspace pointer validation

### Scheduler Issues

1. Enable `kernel::processes::scheduler`
2. Check thread states in dump_current_state
3. Verify timer interrupt is firing

### Memory Issues

1. Check page allocator stats via Ctrl+D
2. Verify page table mappings
3. Check brk/mmap allocations

## Key Files

| File | Purpose |
|------|---------|
| sys/src/logging/mod.rs | Log macros (info!, debug!, warn!) |
| sys/src/logging/configuration.rs | Module log config |
| kernel/src/debugging/mod.rs | dump_current_state |
| kernel/src/debugging/backtrace.rs | Stack unwinding logic |
| arch/src/riscv64/backtrace.rs | CalleeSavedRegs, naked backtrace dispatch |
| kernel/src/debugging/symbols.rs | Symbol resolution |
| kernel/src/debugging/unwinder.rs | DWARF unwinding |
| kernel/src/panic.rs | Panic handler |

## Tips

1. **Use debug! freely** - Disabled by default, no runtime cost
2. **Enable modules selectively** - Too much output is hard to read
3. **Use Ctrl+D** - Quick state check without GDB
4. **Check DONT_LOG list** - Some modules are blocked by default
5. **Leave debug! in code** - Useful for future debugging
