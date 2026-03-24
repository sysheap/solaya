# Memory Management

## Overview

Memory management consists of:
1. **Page Allocator** - Physical page allocation
2. **Page Tables** - Virtual-to-physical address translation
3. **Heap** - Kernel heap via global allocator
4. **PinnedHeapPages** - High-level page allocation wrapper

## Constants

```rust
pub const PAGE_SIZE: usize = 4096;  // sys/src/memory/page.rs
```

## Page Allocator

**File:** `sys/src/memory/page_allocator.rs`

### MetadataPageAllocator

Bitmap-based physical page allocator. Uses one byte per page as metadata.

```rust
pub(super) struct MetadataPageAllocator<'a> {
    metadata: &'a mut [PageStatus],      // One byte per page
    pages: Range<*mut MaybeUninit<Page>>, // Actual page storage
}

enum PageStatus {
    FirstUse,  // Never allocated, needs zeroing
    Free,      // Previously used, already zeroed
    Used,      // Currently allocated (not last)
    Last,      // Last page in allocation
}
```

### Allocation Algorithm

1. Scan metadata for contiguous free range
2. Mark pages as Used, last as Last
3. Zero pages if FirstUse (lazy initialization)
4. Return pointer range

### Deallocation

1. Walk from start until Last marker
2. Mark all pages as Free
3. Return count of freed pages

### Initialization

Called from `kernel/src/memory/mod.rs`:

```rust
pub fn init_page_allocator(reserved_areas: &[Range<*const u8>])
```

- Heap size determined from device tree memory node
- Reserved areas (e.g., device tree) marked as used without zeroing
- First N bytes used for metadata, rest for pages

### API

```rust
// Allocate pages
fn alloc(&mut self, number_of_pages: usize) -> Option<Range<NonNull<Page>>>

// Free pages (returns count freed)
fn dealloc(&mut self, page: NonNull<Page>) -> usize

// Statistics
fn total_heap_pages(&self) -> usize
fn used_heap_pages(&self) -> usize
```

## Page Tables

Core types in `sys/src/memory/page_table.rs`, mapping logic in `kernel/src/memory/page_tables.rs`.

### RISC-V Sv39 Format

39-bit virtual addresses, 3-level page tables:

```
Virtual Address (39 bits):
[38:30] VPN[2] - Level 2 index (9 bits)
[29:21] VPN[1] - Level 1 index (9 bits)
[20:12] VPN[0] - Level 0 index (9 bits)
[11:0]  Page offset (12 bits)
```

### RootPageTableHolder

Manages a complete virtual address space:

```rust
pub struct RootPageTableHolder {
    root_table: *mut PageTable,           // Level 2 table
    already_mapped: Vec<MappingEntry>,    // Track mapped ranges
}
```

### Page Table Entry

```rust
struct PageTableEntry(usize);

// Bit layout:
// [0]     Valid
// [1]     Read
// [2]     Write
// [3]     Execute
// [4]     User accessible
// [5]     Global
// [6]     Accessed
// [7]     Dirty
// [53:10] PPN (physical page number)
```

### XWRMode (Permissions)

```rust
pub enum XWRMode {
    ReadOnly,
    ReadWrite,
    Execute,
    ReadExecute,
    ReadWriteExecute,
}
```

### Key Methods

```rust
impl RootPageTableHolder {
    // Create with kernel mappings
    pub fn new_with_kernel_mapping(include_heap: bool) -> Self

    // Map virtual to physical
    pub fn map(
        &mut self,
        virtual_address: usize,
        physical_address: usize,
        size: usize,
        privileges: XWRMode,
        user_accessible: bool,
        name: impl ToString,
    )

    // Unmap range
    pub fn unmap(&mut self, virtual_address: usize, size: usize)

    // Translate virtual to physical
    pub fn translate(&self, virtual_address: usize) -> Option<usize>

    // Activate this page table (write to satp)
    pub fn activate_page_table(&self)

    // Get SATP value for this table
    pub fn get_satp_value_from_page_tables(&self) -> usize
}
```

### Kernel Mapping

`new_with_kernel_mapping()` maps:
- Kernel code (.text) - ReadExecute
- Kernel data (.rodata, .data, .bss) - ReadWrite
- Heap - ReadWrite (if include_heap=true)
- UART MMIO - ReadWrite
- PLIC MMIO - ReadWrite
- Test device - ReadWrite
- Timer device - ReadWrite
- Runtime mappings (PCI) - ReadWrite

## PinnedHeapPages

**File:** `sys/src/memory/page.rs`

High-level wrapper for allocating pages:

```rust
pub struct PinnedHeapPages {
    allocation: Box<[Page]>,
}

impl PinnedHeapPages {
    pub fn new(number_of_pages: usize) -> Self
    pub fn new_pages(pages: Pages) -> Self
    pub fn fill(&mut self, data: &[u8], offset: usize)
    pub fn addr(&self) -> usize
    pub fn size(&self) -> usize
}
```

Used by:
- Process for code/data pages
- CPU for kernel stack

## Runtime Mappings

**File:** `kernel/src/memory/runtime_mappings.rs`

Stores mappings determined at runtime (e.g., PCI ranges):

```rust
pub fn initialize(mappings: &[MappingDescription])
pub fn get_runtime_mappings() -> &'static [MappingDescription]
```

## Heap Allocator

Core allocator in `sys/src/memory/heap.rs`, kernel-specific setup in `kernel/src/memory/heap.rs`.

Global allocator implementation using the page allocator.

## Copy-on-Write (CoW) Fork

**Files:** `kernel/src/processes/process.rs`, `kernel/src/interrupts/trap.rs`

When a process forks, pages are shared instead of copied. Writable pages are marked read-only in both parent and child page tables. When either process writes to a shared page, the hardware generates a Store/AMO page fault (RISC-V cause 15), which triggers CoW resolution.

### Data Structures

```rust
// Per-page CoW tracking on Process
struct CowPageInfo {
    phys_addr: PhysAddr,              // Physical address of shared page
    original_perm: XWRMode,           // Permission before CoW downgrade
    _backing: Arc<PinnedHeapPages>,   // Shared ownership (freed when last ref drops)
}

// Process fields for page tracking:
// allocated_pages: BTreeMap<VirtAddr, PinnedHeapPages>  -- privately owned pages
// cow_pages: BTreeMap<VirtAddr, CowPageInfo>            -- CoW-shared pages
```

### CoW Resolution Flow

1. Store page fault → `handle_store_page_fault()` in `trap.rs`
2. Looks up faulting VA in process's `cow_pages`
3. Allocates new `PinnedHeapPages(1)`, copies 4K via `page_slice_at_phys()`
4. Remaps PTE to new physical page with original writable permissions
5. Moves entry from `cow_pages` to `allocated_pages`
6. TLB flushed automatically on trap return (`sfence.vma` in `trap.S`)

### Kernel Writes to CoW Pages

`write_userspace_ptr/slice` (which take `&mut self`) resolve CoW for writable pages via `ensure_cow_resolved_for_write()` before the write validation, preventing EFAULT on read-only CoW pages.

## Memory Layout

### Kernel Space
```
0x80000000          Kernel load address (QEMU virt)
  .text             Code section (RX)
  .rodata           Read-only data (R)
  .data             Initialized data (RW)
  .bss              Uninitialized data (RW)
  heap_start        Heap begins (RW)
  ...
  heap_end          End of physical RAM
```

### User Space (per process)
```
0x10000             User code load address
  .text             Code (RX)
  .data             Data (RW)
  .bss              BSS (RW)
  brk               Heap (RW, grows up)
  ...
  stack             Stack (RW, grows down)
0xFFFFFFFF...       Top of address space
```

## Key Files

| File | Purpose |
|------|---------|
| sys/src/memory/address.rs | PhysAddr, VirtAddr types |
| sys/src/memory/page.rs | Page, Pages, PinnedHeapPages |
| sys/src/memory/page_allocator.rs | MetadataPageAllocator |
| sys/src/memory/page_table.rs | PageTable, PageTableEntry, OwnedPageTable |
| sys/src/memory/heap.rs | Core heap allocator (SpinlockHeap) |
| kernel/src/memory/mod.rs | Re-exports from sys, init functions |
| kernel/src/memory/page_tables.rs | RootPageTableHolder, kernel mapping logic |
| kernel/src/memory/page_table_entry.rs | Kernel-specific PTE extensions |
| kernel/src/memory/heap.rs | Kernel global allocator setup |
| kernel/src/memory/linker_information.rs | Linker symbols |
| kernel/src/memory/runtime_mappings.rs | Runtime MMIO mappings |

## Common Operations

### Allocate Pages for Process
```rust
let pages = PinnedHeapPages::new(num_pages);
process.page_table.map(
    virtual_addr,
    pages.addr(),
    pages.size(),
    XWRMode::ReadWrite,
    true,  // user accessible
    "user data",
);
process.allocated_pages.push(pages);
```

### Translate User Address
```rust
let phys = process.page_table.translate(virtual_addr)?;
```

### Check Heap Usage
```rust
let total = PAGE_ALLOCATOR.lock().total_heap_pages();
let used = PAGE_ALLOCATOR.lock().used_heap_pages();
```
