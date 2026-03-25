# Process Management

## Overview

Process management consists of:
1. **Process** - Address space, threads, resources
2. **Thread** - Execution context, registers, state
3. **Scheduler** - Per-CPU round-robin scheduling
4. **Loader** - ELF binary loading

## Process Structure

**File:** `kernel/src/processes/process.rs:31`

```rust
pub struct Process {
    name: Arc<String>,
    page_table: RootPageTableHolder,           // Virtual address space
    mappings: BTreeMap<VirtAddr, Mapping>,      // All page mappings (see Mapping enum)
    free_mmap_address: VirtAddr,               // Next mmap VA (starts 0x2000000000)
    fd_table: Arc<Spinlock<FdTable>>,          // File descriptor table (shared across vfork)
    threads: BTreeMap<Tid, ThreadWeakRef>,
    main_tid: Tid,
    pgid: Tid,                                 // Process group ID
    sid: Tid,                                  // Session ID
    brk: Brk,                                  // Heap break manager
    umask: u32,                                // File creation mask (default 0o022)
    cwd: String,                               // Current working directory (default "/")
}

pub enum Mapping {
    Allocated(PinnedHeapPages),   // ELF-loaded, brk, resolved CoW pages
    Mmap(PinnedHeapPages),        // mmap'd pages
    Cow(CowPageInfo),             // CoW-shared pages (see MEMORY.md)
}
```

### Key Methods

```rust
impl Process {
    // Memory management
    fn mmap_pages(&mut self, num_pages: Pages, perm: XWRMode) -> *mut u8
    fn mmap_pages_with_address(&mut self, num_pages: Pages, addr: usize, perm: XWRMode) -> *mut u8
    fn brk(&mut self, brk: usize) -> usize

    // Userspace pointer access
    fn read_userspace_ptr<T>(&self, ptr: &UserspacePtr<*const T>) -> Result<T, Errno>
    fn write_userspace_ptr<T>(&mut self, ptr: &UserspacePtr<*mut T>, value: T) -> Result<(), Errno>
    fn read_userspace_slice<T>(&self, ptr: &UserspacePtr<*const T>, len: usize) -> Result<Vec<T>, Errno>
    fn write_userspace_slice<T>(&mut self, ptr: &UserspacePtr<*mut T>, data: &[T]) -> Result<(), Errno>

    // File descriptor table
    fn fd_table(&self) -> SpinlockGuard<'_, FdTable>
    fn set_fd_table(&mut self, fd_table: FdTable)

    // Working directory
    fn cwd(&self) -> &str
    fn set_cwd(&mut self, cwd: String)

    // File creation mask
    fn umask(&self) -> u32
    fn set_umask(&mut self, mask: u32)
}
```

## Thread Structure

**File:** `kernel/src/processes/thread.rs:60`

```rust
pub struct Thread {
    tid: Tid,
    parent_tid: Tid,                           // Parent process (Tid::new(0) for init)
    process_name: Arc<String>,
    register_state: TrapFrame,                 // All 32 GP registers
    program_counter: usize,                    // Current PC
    state: ThreadState,                        // Running/Runnable/Waiting
    in_kernel_mode: bool,                      // Kernel vs user mode
    process: ProcessRef,                       // Owning process
    clear_child_tid: Option<UserspacePtr<*mut c_int>>,
    signal_state: SignalState,                 // Signal handlers, mask, altstack
    syscall_task: Option<SyscallTask>,         // Pending async syscall
}
```

### Thread States

```rust
pub enum ThreadState {
    Running { cpu_id: CpuId },  // Currently executing on specified CPU
    Runnable,                   // Ready to run, in run queue
    Waiting,                    // Blocked (sleeping, waiting for I/O)
    Zombie(u8),                 // Exited with exit code, awaiting reaping by parent
}
```

The `cpu_id` in `Running` is critical for multi-CPU correctness. It ensures:
- A thread can only be scheduled on one CPU at a time
- The scheduler atomically claims threads by setting `Running { cpu_id }`
- Race conditions between CPUs are prevented (thread woken by waker on CPU1
  while CPU0 is about to return to userspace with it)

### Thread Creation

**From ELF:**
```rust
Thread::from_elf(elf_file: &ElfFile, name: &str, args: &[&str], parent_tid: Tid)
    -> Result<Arc<Spinlock<Thread>>, LoaderError>
```

**Powersave thread (idle):**
```rust
Thread::create_powersave_thread() -> Arc<Spinlock<Thread>>
```

## Scheduler

**File:** `kernel/src/processes/scheduler.rs:22`

Per-CPU scheduler with round-robin scheduling:

```rust
pub struct CpuScheduler {
    current_thread: ThreadRef,
    powersave_thread: ThreadRef,   // Idle thread (TID 0)
}
```

### Schedule Loop

`schedule()` is called on timer interrupt:

1. Save current thread state (PC, registers)
2. Set current thread to Runnable (if Running), push to run queue
3. Pop next runnable from run queue (stale entries discarded)
4. If thread has pending syscall task:
   - Poll the async task
   - If ready: write result to a0, skip ecall, return to userspace
   - If pending: thread stays in Waiting, try next
5. Load thread state (PC, registers)
6. Set timer (10ms normal, 50ms powersave)
7. Return to userspace via sret

### Timer Quantum

- Normal process: 10ms
- Powersave (idle): 50ms

### Key Scheduler Methods

```rust
impl CpuScheduler {
    // Get current thread/process
    fn get_current_thread(&self) -> &ThreadRef
    fn get_current_process(&self) -> ProcessRef

    // Schedule next process
    fn schedule(&mut self)

    // Kill current process
    fn kill_current_process(&mut self)

    // Handle Ctrl+C
    fn send_ctrl_c(&mut self)
}
```

## Process Table

**File:** `kernel/src/processes/process_table.rs`

Global registry of all threads, with a children index and a separate run queue:

```rust
pub static THE: RuntimeInitializedData<Spinlock<ProcessTable>>;
pub static RUN_QUEUE: Spinlock<VecDeque<ThreadRef>>;  // Separate from ProcessTable
static LIVE_THREAD_COUNT: AtomicUsize;                // Tracks non-zombie threads

struct ProcessTable {
    threads: BTreeMap<Tid, ThreadRef>,       // All threads including zombies
    children: BTreeMap<Tid, Vec<Tid>>,       // parent_tid -> [child_tids] index
    wait_wakers: Vec<Waker>,                 // Wakers for blocked wait4 callers
}
```

### Key Methods

```rust
impl ProcessTable {
    fn init()                                    // Create init process
    fn add_thread(&mut self, thread: ThreadRef)  // Also updates children index + run queue
    fn kill(&mut self, tid: Tid)                 // Set thread to Zombie, reparent orphans, wake waiters
    fn take_zombie(parent_tid, pid) -> Option<(Tid, i32)>  // Reap a zombie child (uses children index)
    fn has_any_child_of(parent_tid) -> bool      // O(1) children index lookup
    fn register_wait_waker(waker)                // Register waker for wait4
}

// Free function (uses LIVE_THREAD_COUNT atomic, no lock needed)
pub fn is_empty() -> bool
```

### Run Queue Design

The run queue (`RUN_QUEUE`) is separate from `ProcessTable` to avoid holding the process table lock during scheduling. Lock ordering: `ProcessTable -> Thread -> RUN_QUEUE`.

Threads are pushed to the run queue when:
- Created via `add_thread`
- Woken from Waiting via `ThreadWaker::wake()`
- Preempted by timer (set back to Runnable in `queue_current_process_back`)

Stale entries (killed/waiting threads) are filtered on pop — the scheduler discards any thread not in Runnable state.

## Parent-Child Relationships

Every thread tracks its parent via `parent_tid: Tid` (stored on Thread, not Process):

- **Init** (first process): `parent_tid = Tid::new(0)` (no real parent)
- **Powersave** (idle): `parent_tid = POWERSAVE_TID`
- **All others**: `parent_tid` = caller's `main_tid` at `clone` time

### wait4 / waitpid enforcement

`wait4(pid, ...)` uses zombie tracking: when a child exits, its thread stays in the process table with `ThreadState::Zombie(exit_code)`. The `WaitChild` future checks for threads in Zombie state under the process_table lock. Reaping removes the thread from the table. Returns `ECHILD` if the caller has no children at all.

### Orphan reparenting

When a process dies in `ProcessTable::kill()`, orphans (children of the dying process) are reparented to init (`Tid::new(1)`). The `children` index is updated in bulk.

### getppid syscall

Linux syscall 173 (`getppid`) returns `thread.parent_tid()` (read from Thread, not Process).

## Process Groups and Sessions

Each process has a **process group ID** (`pgid`) and **session ID** (`sid`), stored on `Process`.

### Initialization
- **Init process** (`Thread::from_elf`): `pgid = tid, sid = tid` (session and group leader)
- **Powersave thread**: `pgid = 0, sid = 0`
- **vfork child** (`clone_vfork`): inherits parent's `pgid`, `sid`, and `cwd`
- **execve**: preserves the old process's `pgid`, `sid`, and `cwd`

### Syscalls
- `getpgid(pid)` — returns PGID of `pid` (0 = self)
- `getsid(pid)` — returns SID of `pid` (0 = self)
- `setpgid(pid, pgid)` — set PGID; can only set own or child's. `pid=0` means self, `pgid=0` means use `pid`
- `setsid()` — create new session (sets `pgid = pid, sid = pid`). Fails with `EPERM` if already a group leader

### wait4 with process groups
`wait4` supports all Linux `pid` modes:
- `pid > 0` — wait for specific child
- `pid == -1` — wait for any child
- `pid == 0` — wait for any child in caller's process group
- `pid < -1` — wait for any child in process group `abs(pid)`

## ELF Loader

**File:** `kernel/src/processes/loader.rs`

Loads ELF binaries and sets up process address space:

```rust
pub fn load_elf(elf: &ElfFile, name: &str, args: &[&str])
    -> Result<LoadedElf, LoaderError>

pub struct LoadedElf {
    entry_address: usize,
    page_tables: RootPageTableHolder,
    allocated_pages: Vec<PinnedHeapPages>,
    args_start: usize,                    // Stack pointer with args
    brk: Brk,                             // Heap break manager
}
```

### User Memory Layout

```
0x10000             Entry point (ELF load address)
  .text             Code (RX)
  .data             Data (RW)
  .bss              BSS (RW, zeroed)
  brk_start         Heap start (grows up)
  ...
STACK_START         Stack grows down from here
STACK_END           Bottom of stack region
```

## Async Syscall Model

**File:** `kernel/src/processes/task.rs`

Blocking syscalls use Rust async/await:

```rust
pub type SyscallTask = Task<Result<isize, Errno>>;

pub struct Task<T> {
    future: Pin<Box<dyn Future<Output = T> + Send>>,
}
```

### Flow

1. Syscall handler creates async task
2. Scheduler polls task with ThreadWaker
3. If `Poll::Pending`: thread suspended, waker registered
4. When ready (timer, I/O): waker called, thread marked Runnable
5. Scheduler polls again, gets `Poll::Ready(result)`
6. Result written to a0, thread returns to userspace

### ThreadWaker

**File:** `kernel/src/processes/waker.rs`

```rust
impl ThreadWaker {
    pub fn new_waker(thread: ThreadWeakRef) -> Waker
}
```

When woken:
1. Upgrade weak reference to thread
2. Call `wake_up()` — if it returns true (Waiting → Runnable), push to `RUN_QUEUE`
3. Thread will be scheduled when popped from run queue

## Brk (Heap Management)

**File:** `kernel/src/processes/brk.rs`

Manages process heap via brk syscall:

```rust
pub struct Brk {
    current: usize,   // Current break
    initial: usize,   // Initial break (after BSS)
}

impl Brk {
    pub fn brk(&mut self, new_brk: usize) -> usize
}
```

## UserspacePtr

**File:** `kernel/src/processes/userspace_ptr.rs`

Safe wrapper for userspace pointers:

```rust
pub struct UserspacePtr<P: Pointer>(P);

impl<P: Pointer> UserspacePtr<P> {
    pub fn new(ptr: P) -> Self             // Mark as userspace pointer
    pub unsafe fn get(&self) -> P          // Get raw pointer (unsafe)
}
```

Used with Process methods to safely read/write userspace memory.

## Key Files

| File | Purpose |
|------|---------|
| kernel/src/processes/process.rs | Process struct and methods |
| kernel/src/processes/thread.rs | Thread struct, state machine |
| kernel/src/processes/scheduler.rs | CpuScheduler, scheduling |
| kernel/src/processes/process_table.rs | Global process registry |
| kernel/src/processes/loader.rs | ELF loading |
| kernel/src/processes/task.rs | Async task wrapper |
| kernel/src/processes/waker.rs | ThreadWaker for async |
| kernel/src/processes/brk.rs | Heap break management |
| kernel/src/processes/timer.rs | Timer interrupt handling |
| kernel/src/processes/userspace_ptr.rs | Userspace pointer wrapper |

## Common Operations

### Access Current Thread/Process
```rust
// Get current thread
let thread = Cpu::current_thread();

// Work with current process
Cpu::with_current_process(|process| {
    process.mmap_pages(Pages::new(1), XWRMode::ReadWrite);
});
```

### Create Async Syscall
```rust
async fn my_syscall(thread: ThreadWeakRef) -> Result<isize, Errno> {
    // Do async work
    timer::sleep(Duration::from_secs(1)).await;
    Ok(0)
}
```
