# Review Notes — `starfive-visionfive-2-improvements`

High-level patterns observed while working through the inline review comments
on this branch. Each item is a habit worth applying automatically in future
work so the reviewer does not have to leave the same comment twice.

## 1. Reach for a typed wrapper before raw bit fields

`IsaExtensions { bits: u32 }` was a flat bitmask with a `has_extension(char)`
API that accepted any character and silently returned `false` for invalid
letters. The fix was to introduce an `Extension` newtype whose only
constructors are the validated `from_letter(u8)` and the `A..=Z` associated
constants. The API now rejects non-letters at compile time instead of at
runtime.

Rule of thumb: if a value is drawn from a known, small set of variants, model
it as a newtype / enum and let the type system reject garbage. Don't expose
`char` or `u32` at the boundary when the set is closed.

## 2. Always use the `MMIO` wrapper for device I/O

`arch::cache::flush_range` was hand-rolling `core::ptr::write_volatile` plus
explicit `fence rw, rw` barriers around every write. The project's `MMIO<T>`
wrapper already encapsulates volatile access and emits the right fences on
write/read, so the manual version was both duplicate and error-prone.

Rule of thumb: any access to a hardware register goes through `MMIO<T>`.
Seeing `write_volatile` or inline `fence` asm in a driver is a smell — check
whether the `MMIO` type already does what's needed.

## 3. Hardware busy-wait timeouts are fatal, not log-and-continue

Several DWMAC/JH7110 paths used to poll hardware for completion and then
`info!("… timed out")` once the loop exhausted. On real hardware that means
the driver silently proceeds with a half-initialized device and fails
mysteriously later. Every such loop is now an `assert!` / `panic!` — aligned
with the "Fail fast with assertions" rule in `CLAUDE.md`.

Rule of thumb: a bounded poll loop on hardware state must either succeed or
crash. `info!` on the timeout branch is never the right answer in the kernel.

## 4. Globals that are always touched together belong in one struct

`plic.rs` had four parallel `RuntimeInitializedData` statics (`PLIC_BASE`,
`PLIC_SIZE`, `PLIC_S_MODE_CONTEXT`, `PLIC_NUM_SOURCES`) that were populated in
the same device-tree scan and read only alongside each other. They are now
fields on the `Plic` struct, constructed once from the device tree in
`discover_from_device_tree`, and exposed through `base()` / `size()`
accessors for the memory mapping code.

Rule of thumb: when you see N sibling `static`s that are initialized together
and only read together, that's a struct waiting to happen. Prefer one
initialized object over N parallel lazy statics.

## 5. Unit tests live next to the code under test

ISA parser tests lived in `kernel/src/test/isa.rs` while the implementation
was in `arch/src/isa.rs`. They are now co-located via `#[cfg(test)] mod
tests` in `arch/src/isa.rs`.

Rule of thumb: put `#[cfg(test)] mod tests` in the same file as the
implementation. Don't create a dedicated `kernel/src/test/<topic>.rs` unless
the tests span multiple modules.

## 6. Think about crate layering before adding a primitive

Moving cache flush to use `MMIO` required relocating `MMIO` from
`sys/klibc/` into `arch` because `sys` already depends on `arch` (so `arch`
cannot import from `sys`). `MMIO` is the lowest-level abstraction over a raw
pointer plus a fence — it belongs in `arch`, not in the higher-level `sys`
layer.

Rule of thumb: when adding a shared primitive, put it in the lowest crate
that anyone who needs it can depend on. When an existing primitive is stuck
in the wrong layer, move it rather than duplicating it or hoisting the dep
graph the wrong way.
