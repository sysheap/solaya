# Testing

## Overview

Two testing approaches:
1. **Unit Tests** - Kernel unit tests with custom framework
2. **System Tests** - Integration tests via QEMU (preferred for AI iteration)

## Quick Commands

```bash
just test           # Run all tests (unit + system)
just unit-test      # Run only unit tests
just system-test    # Run only system tests

# Run specific system test
cargo nextest run --release --manifest-path system-tests/Cargo.toml \
    --target x86_64-unknown-linux-gnu test_name

# Loop test until failure (for flaky tests)
just loop-system-test test_name
```

## System Tests (Preferred)

**Location:** `system-tests/src/tests/`

System tests run on x86_64 and interact with the OS running in QEMU. Better for AI iteration because:
- No need to recompile kernel for test changes
- Can interact with the OS interactively
- Easier to debug failures

### QemuInstance

**File:** `system-tests/src/infra/qemu.rs`

```rust
pub struct QemuInstance {
    instance: Child,
    stdin: ChildStdin,
    stdout: ReadAsserter<ChildStdout>,
}

impl QemuInstance {
    // Start with default options (SMP enabled)
    pub async fn start() -> anyhow::Result<Self>

    // Start with custom options
    pub async fn start_with(options: QemuOptions) -> anyhow::Result<Self>

    // Run a program and get output
    pub async fn run_prog(&mut self, prog_name: &str) -> anyhow::Result<String>

    // Run program and wait for specific output
    pub async fn run_prog_waiting_for(&mut self, prog: &str, wait: &str)
        -> anyhow::Result<String>

    // Send Ctrl+C and wait for prompt
    pub async fn ctrl_c_and_assert_prompt(&mut self) -> anyhow::Result<String>

    // Wait for QEMU to exit
    pub async fn wait_for_qemu_to_exit(self) -> anyhow::Result<ExitStatus>

    // Access stdin/stdout directly
    pub fn stdin(&mut self) -> &mut ChildStdin
    pub fn stdout(&mut self) -> &mut ReadAsserter<ChildStdout>
}
```

### QemuOptions

```rust
pub struct QemuOptions {
    add_network_card: bool,  // Enable VirtIO network
    use_smp: bool,           // Enable multi-core (default: true)
    enable_gdb: bool,        // Enable GDB server (auto-set by SOLAYA_ENABLE_GDB env var)
}

// Usage
QemuInstance::start_with(
    QemuOptions::default()
        .add_network_card(true)
        .use_smp(false)
        .enable_gdb(true)
).await?
```

### ReadAsserter

**File:** `qemu-infra/src/read_asserter.rs`

```rust
impl ReadAsserter<R> {
    pub async fn assert_read_until(&mut self, needle: &str) -> Result<Vec<u8>, ReadError>
}
```

Reads from stdout until finding the needle string.

### Boot Sequence

QemuInstance::start() automatically waits for:
1. "Hello World from Solaya!"
2. "kernel_init done!"
3. "dhcpd: configured ip" (only when the test enables networking)
4. Shell prompt ("$ ") — emitted by dash once busybox init's
   `console::respawn` entry has spawned the interactive shell.

### Example Tests

**Basic program execution:**
```rust
#[tokio::test]
async fn execute_program() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let output = solaya.run_prog("prog1").await?;
    assert_eq!(output, "Hello from Prog1\n");
    Ok(())
}
```

**Time-based test:**
```rust
#[tokio::test]
async fn sleep() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    let start = Instant::now();
    solaya.run_prog("sleep 1").await?;
    assert!(start.elapsed() >= Duration::from_secs(1));
    Ok(())
}
```

**Network test:**
```rust
#[file_serial]  // Prevent concurrent network tests
#[tokio::test]
async fn udp() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start_with(
        QemuOptions::default().add_network_card(true)
    ).await?;

    solaya.run_prog_waiting_for("udp", "Listening").await?;

    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    socket.connect("127.0.0.1:1234").await?;
    socket.send(b"test").await?;

    solaya.stdout().assert_read_until("test").await?;
    Ok(())
}
```

**Signal handling:**
```rust
#[tokio::test]
async fn ctrl_c() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    solaya.run_prog_waiting_for("loop", "looping").await?;
    solaya.ctrl_c_and_assert_prompt().await?;
    Ok(())
}
```

### Writing Throw-Away Tests

Add to `system-tests/src/tests/basics.rs`:

```rust
#[tokio::test]
async fn my_quick_test() -> anyhow::Result<()> {
    let mut solaya = QemuInstance::start().await?;
    // Your test code here
    let output = solaya.run_prog("myprogram").await?;
    println!("Output: {}", output);
    Ok(())
}
```

Run: `cargo nextest run --release --manifest-path system-tests/Cargo.toml --target x86_64-unknown-linux-gnu my_quick_test`

### Test Files

| File | Contents |
|------|----------|
| system-tests/src/tests/basics.rs | Basic boot, shutdown, program execution |
| system-tests/src/tests/sleep.rs | Sleep syscall tests |
| system-tests/src/tests/signals.rs | Signal handling (Ctrl+C) |
| system-tests/src/tests/net.rs | UDP networking tests |
| system-tests/src/tests/coreutils.rs | GNU coreutils tests |
| system-tests/src/tests/panic.rs | Panic handling |
| system-tests/src/tests/connect4.rs | Connect 4 game tests |

## Unit Tests

**Location:** Scattered throughout `kernel/src/`

Uses Rust's `custom_test_frameworks` feature with `#[test_case]` macro.

### Test Framework

**File:** `kernel/src/test/mod.rs`

```rust
pub fn test_runner(tests: &[&dyn Testable]) {
    // Initialize kernel systems needed for tests
    // Run each test
    // Exit QEMU with success/failure
}

pub trait Testable {
    fn run(&self);
}
```

### Writing Unit Tests

```rust
#[cfg(test)]
mod tests {
    #[test_case]
    fn my_test() {
        assert_eq!(2 + 2, 4);
    }
}
```

### Test-Only Methods Convention

Test-only helper methods on kernel types must live inside `#[cfg(test)] mod tests` as a separate `impl` block, not as `#[cfg(test)]`-annotated methods on the main `impl`. This works because child modules can access private fields of parent types.

```rust
// GOOD: test-only method inside mod tests
#[cfg(test)]
mod tests {
    impl super::MyStruct {
        pub fn helper(&self) -> bool {
            !self.private_field.is_empty()
        }
    }
}

// BAD: #[cfg(test)] on a method in the main impl
impl MyStruct {
    #[cfg(test)]
    pub fn helper(&self) -> bool { ... }
}
```

### Run Unit Tests

```bash
just unit-test
# or
cargo test --release
```

## Test Dependencies

System tests use:
- `tokio` - Async runtime
- `anyhow` - Error handling
- `serial_test` - Test synchronization (`#[file_serial]`)
- `cargo-nextest` - Test runner

## Key Files

| File | Purpose |
|------|---------|
| justfile | Test commands |
| system-tests/Cargo.toml | System test dependencies |
| system-tests/src/lib.rs | Test module registration |
| system-tests/src/infra/qemu.rs | QEMU instance management |
| system-tests/src/infra/read_asserter.rs | Stdout assertion helper |
| kernel/src/test/mod.rs | Unit test framework |
| kernel/src/test/qemu_exit.rs | QEMU exit signaling |

## Debugging Flaky Tests

System tests can fail intermittently due to kernel race conditions. Use the deadlock-hunt infrastructure to reproduce and diagnose.

### Quick Commands

```bash
just deadlock-hunt       # Loop ALL tests with GDB enabled, 1hr timeout, sequential
just loop-system-test X  # Loop a specific test until failure
just stress-system-test  # Run all tests 5x
```

### How It Works

- **Env var `SOLAYA_ENABLE_GDB=1`** makes all `QemuInstance::start()` calls pass `--gdb` to QEMU and use a 1-hour `ReadAsserter` timeout (instead of 30s)
- **Nextest profile `deadlock-hunt`** (`system-tests/.config/nextest.toml`) sets 1-hour slow-timeout and `test-threads = 1` (sequential execution required — `.gdb-port` file is shared)
- **`.gdb-port`** is written by `qemu_wrapper.sh` when `--gdb` is passed. The GDB MCP server reads this file automatically.

### Procedure

1. **Run `just deadlock-hunt`** — each iteration takes ~13s normally
2. **When an iteration stalls** (>15s with no progress), the kernel is stuck
3. **Attach GDB** via MCP: `gdb_connect` (reads `.gdb-port`)
4. **List CPU harts**: `gdb_threads`
5. **Get backtraces**: `gdb_select_thread N` + `gdb_backtrace` for each hart

### Diagnosing the Hang

| All CPUs in `powersave` | Lost wakeup — all threads are Waiting, no one will wake them |
|---|---|
| CPUs in `Spinlock::lock()` | Classic deadlock — identify which locks and who holds them |
| One CPU stuck, rest idle | Single thread in infinite loop or waiting on I/O |

### Key Lock Addresses (for GDB)

| Lock | File |
|------|------|
| `QEMU_UART` | `kernel/src/io/uart.rs` |
| `CONSOLE_TTY` | `kernel/src/io/tty_device.rs` |
| `PLIC` | `kernel/src/interrupts/plic.rs` |
| `WAKEUP_QUEUE` | `kernel/src/processes/timer.rs` |
| `ProcessTable (THE)` | `kernel/src/processes/process_table.rs` |
| Per-CPU scheduler | `kernel/src/cpu.rs` (Cpu.scheduler field) |

### Known Race Condition Patterns

**Lost wakeup in async syscalls:** The waker fires between `task.poll()` returning `Pending` and `set_syscall_task_and_suspend()` setting state to `Waiting`. Since `wake_up()` only transitions `Waiting → Runnable`, the wakeup is dropped. Fixed with `wakeup_pending` flag in `kernel/src/processes/thread.rs`.

**Interrupt during lock hold:** Spinlocks don't disable interrupts. If an interrupt handler tries to acquire a lock already held on the same CPU, self-deadlock occurs.

### Key Files

| File | Purpose |
|------|---------|
| `system-tests/.config/nextest.toml` | Timeout profiles (default + deadlock-hunt) |
| `qemu-infra/src/read_asserter.rs` | Configurable per-read timeout |
| `qemu-infra/src/qemu.rs` | QemuOptions with `enable_gdb` and env var support |
| `kernel/src/processes/thread.rs` | Thread state, `wake_up()`, `wakeup_pending` flag |
| `kernel/src/interrupts/trap.rs` | Syscall handler with race window between poll and suspend |

## Tips for AI

1. **Prefer system tests** - Easier to iterate without kernel recompilation
2. **Use throw-away tests** - Write quick tests in basics.rs, clean up later
3. **Use `#[file_serial]`** - For tests that conflict (network, state)
4. **Check boot sequence** - If tests hang, boot may have failed
5. **Use `run_prog_waiting_for`** - When you need specific output before continuing
