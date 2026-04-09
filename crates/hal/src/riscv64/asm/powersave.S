.section .text
.global powersave
.align 4
powersave:
        wfi
        j powersave
