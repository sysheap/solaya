# Disable generation of compressed instructions.
.option norvc

# Define a .text.init section. The .text.init is put at the
# starting address so that the entry _start is put at the RISC-V
# address 0x8000_0000.
.section .text.init

# RISC-V Linux Image header (64 bytes).
# Required for U-Boot's booti command on real hardware.
.global _start
_start:
	j _start_kernel			/* code0: branch to kernel start */
	nop				/* code1 */
	.dword 0x200000			/* text_offset from start of RAM */
	.dword _end - _start		/* image_size */
	.dword 0			/* flags */
	.word 0x2			/* header version */
	.word 0				/* res1 */
	.dword 0			/* res2 */
	.dword 0x5643534952		/* magic: "RISCV\0\0\0" (upstream U-Boot) */
	.word 0x05435352		/* magic2: "RSC\x05" (StarFive U-Boot) */
	.word 0				/* res3 */

# Actual kernel entry point.
# a0 is hart id
# a1 is pointer to device tree
# Preserve them during startup code
_start_kernel:

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
