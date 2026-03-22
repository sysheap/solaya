# Disable generation of compressed instructions.
.option norvc

# Define a .text.init section. The .text.init is put at the
# starting address so that the entry _start is put at the RISC-V
# address 0x8000_0000.
.section .text.init

# Execution starts here.
# a0 is hart id
# a1 is pointer to device tree
# Preserve them during startup code
.global _start
_start:

	# Disable linker instruction relaxation for the `la` instruction below.
	# This disallows the assembler from assuming that `gp` is already initialized.
	# This causes the value stored in `gp` to be calculated from `pc`.
	# The job of the global pointer is to give the linker the ability to address
	# memory relative to GP instead of as an absolute address.
.option push
.option norelax
	la		gp, __global_pointer$
.option pop

	# Disable interrupts.
	csrw sie, zero

	# Clear bss segment
	la t0, __start_bss
	la t1, __stop_bss
	bgeu t0, t1, bss_cleared

bss_loop:
	sd zero, (t0)
	addi t0, t0, 8
	bltu t0, t1, bss_loop

bss_cleared:

	la sp, __stop_kernel_stack

	# Load trap vector (direct mode)
	la t0, asm_handle_trap
	csrw stvec, t0

	call kernel_init

	# We should never come here
	j asm_panic_rust

.section .text
.global start_hart
.align 4
start_hart:
.option push
.option norelax
	la		gp, __global_pointer$
.option pop

	# disable interrupts.
	csrw sie, zero

	# Load trap vector (direct mode)
	la t0, asm_handle_trap
	csrw stvec, t0

	# a1 contains the cpu struct
	csrw sscratch, a1

	# Load kernel page tables such that we get the correct stack
	ld t1, {KERNEL_PAGE_TABLES_SATP_OFFSET}(a1)
	csrw satp, t1
	sfence.vma zero, zero

	# Set kernel stack pointer to 0xffffffffffffffff
	li sp, 0
	addi sp, sp, -64

	call prepare_for_scheduling

	# We should never come here
	j asm_panic_rust
