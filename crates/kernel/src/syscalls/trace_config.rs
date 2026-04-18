pub const TRACED_PROCESSES: &[&str] = &["prog2"];

/// Returns true if `name` is in [`TRACED_PROCESSES`]. Used by the
/// syscall tracer *and* by code that would otherwise pay per-process
/// cost only to support tracing (e.g. retaining ELF bytes for
/// userspace backtrace symbolication).
pub fn is_traced(name: &str) -> bool {
    TRACED_PROCESSES.contains(&name)
}
