/*
 Linker script for Solaya RISC-V kernel.
 Kernel is loaded at 0x80200000 (SBI convention: DRAM_BASE + 2MB).
*/

OUTPUT_ARCH( "riscv" )

ENTRY( _start )

MEMORY_START = 0x80200000;


SECTIONS
{
  . = MEMORY_START;

  text : ALIGN(4K) {
    PROVIDE(__executable_start = .);
    *(.text.init) *(.text .text.*)
    PROVIDE(__etext = .);
  }

  rodata : ALIGN(4K) {
    *(.rodata .rodata.* .srodata .srodata.*)
  }

  eh_frame : ALIGN(4K) {
    PROVIDE(__eh_frame = .);
    KEEP (*(.eh_frame)) *(.eh_frame.*)
  }

  data : ALIGN(4K) {
    PROVIDE(__global_pointer$ = . + 0x800);
    *(.sdata .sdata.*) *(.data .data.*)
  }

  bss (NOLOAD) : ALIGN(4K) {
    *(.sbss .sbss.*) *(.bss .bss.*)
  }

  kernel_stack (NOLOAD) : ALIGN(4K) {
    . = . + 512K;
  }

  # This section will contain the symbols of the binary, patched in after
  # the link by `patch-symbols` (see crates/kernel/CMakeLists.txt).  The
  # heap starts directly after this section.
  #
  # The reservation below is the single source of truth for the section
  # size: `patch-symbols` reads it back from the linked ELF and pads the
  # nm output to match before `llvm-objcopy --update-section` (llvm-objcopy,
  # unlike GNU objcopy, refuses to grow sections that are part of a LOAD
  # segment).  Bump this value if the demangled symbol table outgrows it.
  # The runtime treats the contents as a null-terminated string, so
  # trailing zero padding is invisible to readers
  # (crates/kernel/src/debugging/symbols.rs).
  symbols : ALIGN(4K) {
    . = . + 16M;
  }

  PROVIDE(_end = .);
}
