.section .text
.global powersave
.align 4
asm_panic:
	# Disable interrupts.
	csrw sie, zero

        # Load emergency stack
	la sp, __stop_kernel_stack

	# Jump to asm_panic_rust such that we still now
	# where we came from via the ra register
        j asm_panic_rust
        
