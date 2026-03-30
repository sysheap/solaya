/*
 Linker script for outputting to RISC-V QEMU "virt" machine.
 Based on Stephen Marz
 6 October 2019
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

  # This section will contain the symbols of the binary
  # We patch them in after the binary is built via objcopy
  # The heap start directly after this section
  symbols : ALIGN(4K) {
    BYTE(0);
  }
}
