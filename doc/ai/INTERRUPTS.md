# Interrupt Handling

## Overview

Interrupt handling in Solaya:
1. **Trap Entry** - Assembly saves registers, dispatches to handler
2. **PLIC** - Platform Level Interrupt Controller for external interrupts
3. **Timer** - Scheduling timer via SBI
4. **Syscalls** - User ecall traps

Interrupts are disabled while holding a Spinlock (via `InterruptGuard` in `SpinlockGuard`). The kernel worker thread runs with interrupts enabled between lock acquisitions. Kernel-mode threads are preemptible by timer interrupts.

## Trap Flow

```
User/Kernel Code
      |
      v (interrupt/exception)
  stvec -> trap.s
      |
      +-> Save TrapFrame to CPU struct
      +-> Switch to kernel stack
      +-> Dispatch based on scause:
          |
          +-> Timer        -> handle_timer_interrupt()
          +-> External     -> handle_external_interrupt()
          +-> Software     -> handle_supervisor_software_interrupt()
          +-> Syscall      -> handle_exception() -> handle_syscall()
          +-> Other        -> handle_unimplemented()
      |
      +-> Restore TrapFrame
      +-> sret (return to user/kernel)
```

## RISC-V Trap Causes

**File:** `kernel/src/interrupts/trap_cause.rs`

### Interrupts (scause MSB = 1)

| Code | Name | Handler |
|------|------|---------|
| 1 | Supervisor Software Interrupt | handle_supervisor_software_interrupt() |
| 5 | Supervisor Timer Interrupt | handle_timer_interrupt() |
| 9 | Supervisor External Interrupt | handle_external_interrupt() |

### Exceptions (scause MSB = 0)

| Code | Name |
|------|------|
| 0 | Instruction address misaligned |
| 1 | Instruction access fault |
| 2 | Illegal instruction |
| 3 | Breakpoint |
| 4 | Load address misaligned |
| 5 | Load access fault |
| 6 | Store/AMO address misaligned |
| 7 | Store/AMO access fault |
| 8 | Environment call from U-mode (syscall) |
| 9 | Environment call from S-mode |
| 12 | Instruction page fault |
| 13 | Load page fault |
| 15 | Store/AMO page fault |

## Trap Handlers

**File:** `kernel/src/interrupts/trap.rs`

### handle_timer_interrupt()

Called on supervisor timer interrupt:
```rust
fn handle_timer_interrupt() {
    timer::wakeup_wakers();  // Wake sleeping threads
    Cpu::with_scheduler(|mut s| s.schedule());  // Reschedule
}
```

### handle_external_interrupt()

Called on external interrupt. The PLIC claims the next pending IRQ and
dispatches through the typed `Arc<dyn driver_api::IrqHandler>` registered for
that source (see `crates/kernel/src/interrupts/plic.rs`). Each driver's
`IrqHandler::handle()` acknowledges the device-specific ISR and wakes the
bottom-half task; the worker thread polls the readied tasks outside interrupt
context.

```rust
fn handle_external_interrupt() {
    let irq = PLIC.lock().claim()?;
    plic::dispatch_interrupt(irq);  // Arc<dyn IrqHandler>::handle()
    PLIC.lock().complete(irq);
}
```

### handle_supervisor_software_interrupt()

Handles both IPIs (for thread kills) and worker thread sleep requests:
```rust
fn handle_supervisor_software_interrupt() {
    let sleep_requested = kernel_tasks::take_sleep_request();
    Cpu::with_scheduler(|mut s| {
        if sleep_requested && is_current_worker {
            // Save registers, suspend unless wakeup_pending
            t.set_register_state(Cpu::read_trap_frame());
            t.suspend_unless_wakeup_pending();
        }
        s.schedule();  // Always reschedule (handles IPIs too)
    });
}
```

### handle_syscall()

Called on ecall from U-mode:
```rust
fn handle_syscall() {
    let trap_frame = Cpu::read_trap_frame();
    let task = Task::new(async { handler.handle(&trap_frame).await });
    if let Poll::Ready(result) = task.poll(&mut cx) {
        trap_frame[Register::a0] = result;
        sepc += 4;  // Skip ecall
    } else {
        thread.set_syscall_task_and_suspend(task);
        scheduler.schedule();
    }
}
```

### handle_unhandled_exception()

Handles unexpected exceptions:
- Userspace fault: Log and kill process
- Kernel fault: Panic

## PLIC (Platform Level Interrupt Controller)

**File:** `kernel/src/interrupts/plic.rs`

### Constants

```rust
pub const PLIC_BASE: usize = 0x0c00_0000;
pub const PLIC_SIZE: usize = 0x1000_0000;
const UART_INTERRUPT_NUMBER: u32 = 10;
```

### PLIC Structure

```rust
pub struct Plic {
    priority_register_base: MMIO<u32>,
    enable_register: MMIO<u32>,
    threshold_register: MMIO<u32>,
    claim_complete_register: MMIO<u32>,
}
```

### Initialization

```rust
pub fn init_uart_interrupt(hart_id: usize) {
    PLIC.initialize(Spinlock::new(Plic::new(PLIC_BASE, hart_id)));
    plic.set_threshold(0);
    plic.enable(UART_INTERRUPT_NUMBER);
    plic.set_priority(UART_INTERRUPT_NUMBER, 1);
}
```

### PLIC Methods

```rust
impl Plic {
    fn enable(&mut self, interrupt_id: u32)
    fn set_priority(&mut self, interrupt_id: u32, priority: u32)  // 0-7
    fn set_threshold(&mut self, threshold: u32)  // 0-7
    pub fn get_next_pending(&mut self) -> Option<InterruptSource>
    pub fn complete_interrupt(&mut self, source: InterruptSource)
}
```

## Timer

**File:** `kernel/src/processes/timer.rs`

### Constants

```rust
pub const CLINT_BASE: usize = 0x2000000;
pub const CLINT_SIZE: usize = 0x10000;
```

### Timer Functions

```rust
// Set timer interrupt (milliseconds from now)
pub fn set_timer(milliseconds: u64) {
    let next = current_clocks + CLOCKS_PER_NANO * 1000 * milliseconds;
    sbi::set_timer(next);
    Cpu::enable_timer_interrupt();
}

// Wake threads whose sleep time has passed
pub fn wakeup_wakers() {
    let current = get_current_clocks();
    let threads = WAKEUP_QUEUE.lock().split_off_lower_than(&current);
    for waker in threads.into_values() {
        waker.wake();
    }
}
```

### Sleep Future

```rust
pub struct Sleep {
    wakeup_time: u64,
    registered: bool,
}

impl Future for Sleep {
    fn poll(self, cx: &mut Context) -> Poll<()> {
        if current_clocks >= self.wakeup_time {
            return Poll::Ready(());
        }
        if !self.registered {
            WAKEUP_QUEUE.lock().insert(self.wakeup_time, cx.waker().clone());
            self.registered = true;
        }
        Poll::Pending
    }
}
```

## Key CSRs

| CSR | Purpose |
|-----|---------|
| stvec | Trap vector base address |
| sepc | Exception program counter |
| scause | Trap cause |
| stval | Trap value (bad address) |
| sstatus | Supervisor status (SIE, SPP) |
| sie | Supervisor interrupt enable |
| sip | Supervisor interrupt pending |

### SIE Bits

| Bit | Name | Purpose |
|-----|------|---------|
| 1 | SSIE | Software interrupt enable |
| 5 | STIE | Timer interrupt enable |
| 9 | SEIE | External interrupt enable |

## Key Files

| File | Purpose |
|------|---------|
| kernel/src/asm/trap.s | Assembly trap entry/exit |
| kernel/src/interrupts/trap.rs | Trap dispatch handlers |
| kernel/src/interrupts/trap_cause.rs | Trap cause definitions |
| kernel/src/interrupts/plic.rs | PLIC driver |
| kernel/src/processes/timer.rs | Timer and sleep |

## Common Operations

### Enable All Interrupts
```rust
Cpu::write_sie(usize::MAX);  // Enable all sources
Cpu::csrs_sstatus(0b10);     // Set SIE bit (global enable)
```

### Set Timer for Scheduling
```rust
timer::set_timer(10);  // 10ms quantum
```

### Add Wakeup
```rust
WAKEUP_QUEUE.lock().insert(wakeup_time, waker);
```
