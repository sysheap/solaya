# Architecture

## Overview

Solaya is a RISC-V 64-bit hobby OS kernel written in Rust. Key characteristics:
- Target: riscv64gc-unknown-none-elf (no_std)
- Virtual memory: Sv39 (3-level page tables)
- Multi-core (SMP) support via RISC-V SBI
- Async/await runtime for blocking syscalls
- Linux-compatible syscall interface

## Project Structure

```
arch/src/              # Hardware abstraction layer (no_std crate)
  lib.rs               # cfg(target_arch) dispatch + CpuId type
  riscv64/
    cpu.rs             # CSR read/write, barriers, sfence
    backtrace.rs       # CalleeSavedRegs, naked backtrace dispatch
    linker_symbols.rs  # Linker symbol declarations (extern statics)
    sbi/               # SBI ecall + extensions (timer, IPI, hart state)
    timer.rs           # rdtime, CLINT constants
    trap_cause.rs      # Interrupt/exception cause constants
  stub/                # No-op stubs for non-riscv64 (Kani, miri)

sys/src/               # Self-contained system library (no kernel deps)
  cpu.rs               # CpuBase struct, per-CPU access helpers
  asm/                 # Assembly (boot.S, trap.S, powersave.S, panic.S)
  memory/              # PhysAddr, VirtAddr, Page, PageTable, page allocator, heap
  klibc/               # Spinlock, MMIO, ValidatedPtr, array_vec, sizes, util
  logging/             # Log macros and per-module configuration
  io/                  # UART driver

boot/src/
  main.rs              # #[no_mangle] entry points (calls into kernel)

kernel/src/            # Main kernel logic (#![forbid(unsafe_code)])
  lib.rs               # kernel_init, prepare_for_scheduling
  cpu.rs               # Cpu struct (embeds sys::cpu::CpuBase + scheduler)
  asm/                 # Re-exports from arch
  memory/              # RootPageTableHolder, linker info, runtime mappings
  processes/           # Process, thread, scheduler
  syscalls/            # Linux syscall handlers
  interrupts/          # Trap handling, PLIC
  net/                 # Network stack (UDP, TCP)
  drivers/             # VirtIO drivers, consolidated init_all_pci_devices()
  io/                  # UART extensions, TtyDevice (terminal subsystem)
  pci/                 # PCI enumeration
  klibc/               # Re-exports from sys + kernel-specific utils
  debugging/           # Backtrace, symbols
  logging/             # Re-exports from sys

userspace/src/
  bin/                 # User programs (init, etc.)
  lib.rs               # Syscall wrappers
```

## Boot Sequence

Assembly calls `kernel_init()` in `boot/src/main.rs`, which delegates to `solaya::kernel_init()` in `kernel/src/lib.rs`.

```
boot::kernel_init() -> solaya::kernel_init()
  |
  +-> sys::cpu::STARTING_CPU_ID.init() # Store boot hart ID
  +-> QEMU_UART.init()              # Initialize serial output
  +-> sbi::base_extension::sbi_get_spec_version() # Check SBI version >= 0.2
  +-> sbi::hart_state_extension::get_number_of_harts() # Count CPUs
  +-> symbols::init()               # Load debug symbols
  +-> device_tree::init()           # Parse device tree
  +-> memory::init_page_allocator() # Set up physical page allocator
  +-> backtrace::init()             # Initialize stack unwinding
  +-> timer::init()                 # Initialize timer subsystem
  +-> pci::parse()                  # Parse PCI from device tree
  +-> pci::PCI_ALLOCATOR_64_BIT.init()  # PCI address allocator
  +-> memory::initialize_runtime_mappings()  # Map PCI space
  +-> process_table::init()         # Create init process
  +-> Cpu::init()                   # Initialize boot CPU struct
  +-> Cpu::activate_kernel_page_table()
  +-> plic::init_uart_interrupt()   # Enable UART interrupts
  +-> enumerate_devices()           # Find PCI devices
  +-> drivers::init_all_pci_devices()  # Init all VirtIO drivers
  +-> kernel_tasks::create_worker_thread()  # Kernel async task executor
  +-> start_other_harts()           # Boot other CPUs
  +-> prepare_for_scheduling()      # Enter scheduler loop
```

`prepare_for_scheduling()` in `kernel/src/lib.rs`:
```
prepare_for_scheduling()
  |
  +-> arch::cpu::write_sie(usize::MAX) # Enable all interrupt sources
  +-> arch::cpu::csrs_sstatus(0b10)    # Enable global interrupts
  +-> timer::set_timer(0)              # Trigger immediate timer
  +-> wfi_loop()                       # Wait for interrupt loop
```

## CPU Structure

Per-CPU state split across `sys/src/cpu.rs` (base) and `kernel/src/cpu.rs` (full):

```rust
// sys/src/cpu.rs - Fields accessed by assembly (offsets must be stable)
#[repr(C)]
pub struct CpuBase {
    pub kernel_page_tables_satp_value: usize,  // Kernel SATP for trap entry
    pub trap_frame: TrapFrame,                  // Saved registers on trap
    pub cpu_id: CpuId,                          // Hart ID
}

// kernel/src/cpu.rs - Full per-CPU struct
#[repr(C)]  // CpuBase must be first field for assembly offset compatibility
pub struct Cpu {
    base: sys::cpu::CpuBase,                    // Assembly-visible fields
    scheduler: Spinlock<CpuScheduler>,           // Per-CPU scheduler
    kernel_page_tables: RootPageTableHolder,     // Kernel page tables
    number_cpus: usize,                          // Total CPU count
}
```

Access current CPU: `Cpu::current()` (via sscratch CSR, read through `arch::cpu::read_sscratch()`)

## Key Data Structures

### Process (kernel/src/processes/process.rs)
```rust
pub struct Process {
    name: String,
    page_table: RootPageTableHolder,        # Virtual address space
    allocated_pages: Vec<PinnedHeapPages>,  # Physical memory
    threads: BTreeMap<Tid, ThreadWeakRef>,  # Process threads
    brk: usize,                             # Heap break pointer
    free_mmap_address: usize,               # Next mmap address
    fd_table: FdTable,                     # File descriptor table
}
```

### Thread (kernel/src/processes/thread.rs)
```rust
pub struct Thread {
    tid: Tid,
    trap_frame: TrapFrame,                  # Saved registers
    program_counter: usize,                 # Current PC
    state: ThreadState,                     # Running/Runnable/Waiting
    process: ProcessRef,                    # Parent process
    syscall_task: Option<Task>,             # Async syscall task
    signal_state: SignalState,              # Signal handlers, mask, altstack
}
```

### TrapFrame (common/src/syscalls/trap_frame.rs)
All 32 general-purpose registers saved on trap/syscall.

## Subsystem Interactions

```
              Timer/External Interrupt
                      |
                      v
              interrupts/trap.rs
              (handle_interrupt)
                      |
          +-----------+-----------+
          |           |           |
          v           v           v
     Timer Int    UART Int    Syscall
          |           |           |
          v           v           v
    scheduler    tty_device  syscalls/
    .schedule()  .push_input handler.rs
          |           |           |
          v           v           v
    Context      read()      Process/
    Switch       wakes       Thread
                             state
```

## Memory Layout

### Kernel Virtual Address Space
- Kernel code/data: Identity-mapped from linker script
- Heap: After kernel image, size from device tree
- PCI ranges: Runtime-mapped from device tree
- Per-CPU kernel stack: Top of address space (0xFFFF...)

### User Virtual Address Space
- Code: 0x10000 (ELF load address)
- Stack: Top of address space growing down
- Heap (brk): After BSS, grows up
- mmap regions: Between heap and stack

## RISC-V Specifics

### CSRs Used
All CSR access is through `arch::cpu` functions (e.g., `arch::cpu::read_sepc()`, `arch::cpu::write_satp()`).

| CSR | Purpose |
|-----|---------|
| satp | Page table base register |
| sstatus | Supervisor status (interrupts, SPP) |
| sepc | Exception program counter |
| scause | Trap cause |
| stval | Trap value (bad address) |
| sscratch | Points to Cpu struct |
| sie | Interrupt enable bits |
| sip | Interrupt pending bits |

### SBI Interface
All SBI calls are through `arch::sbi` (implemented via ecall in `arch/src/riscv64/sbi/`).

- `arch::sbi::extensions::timer_extension::sbi_set_timer()` - Schedule timer interrupt
- `arch::sbi::extensions::hart_state_extension::start_hart()` - Boot other CPUs
- `arch::sbi::extensions::ipi_extension::sbi_send_ipi()` - Inter-processor interrupt

### Page Table Format
Sv39: 39-bit virtual addresses, 3-level page tables
- VPN[2]: 9 bits (level 2)
- VPN[1]: 9 bits (level 1)
- VPN[0]: 9 bits (level 0)
- Page offset: 12 bits (4KB pages)

## Async Syscall Model

Blocking syscalls use Rust async/await:

1. Syscall invoked from userspace
2. Handler creates `Task` (async future)
3. Task polled in scheduler loop
4. If not ready: thread suspended, `Poll::Pending`
5. Waker registered (timer, I/O event)
6. Event occurs: waker called, thread marked runnable
7. Task polled again, returns `Poll::Ready`
8. Result returned to userspace

Key files:
- `kernel/src/processes/task.rs` - Task wrapper
- `kernel/src/processes/waker.rs` - ThreadWaker
- `kernel/src/syscalls/handler.rs` - SyscallHandler trait

## Key Files Quick Reference

| Purpose | File |
|---------|------|
| Boot entry points | boot/src/main.rs |
| Kernel init | kernel/src/lib.rs |
| CPU struct (base) | sys/src/cpu.rs |
| CPU struct (full) | kernel/src/cpu.rs |
| CSR access | arch/src/riscv64/cpu.rs |
| SBI calls | arch/src/riscv64/sbi/ |
| Assembly (boot, trap) | sys/src/asm/ |
| Trap handler | kernel/src/interrupts/trap.rs |
| Scheduler | kernel/src/processes/scheduler.rs |
| Process struct | kernel/src/processes/process.rs |
| Thread struct | kernel/src/processes/thread.rs |
| Syscall dispatch | kernel/src/syscalls/handler.rs |
| Page table types | sys/src/memory/page_table.rs |
| Page table mapping | kernel/src/memory/page_tables.rs |
| Driver init | kernel/src/drivers/mod.rs |
