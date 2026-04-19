# cmake/qemu.cmake — run/debug/attach/disasm targets for the QEMU workflow.
#
# `run` and `run-fb` go through `cargo run --release`, which hits the
# [target.riscv64gc-unknown-none-elf] `runner` in .cargo/config.toml —
# i.e., qemu_wrapper.sh with the usual --gdb/--net/--smp/--block flags.
# Once qemu_wrapper.sh is retired (stage 10), this file invokes the
# bootstrapped qemu-system-riscv64 directly.

add_custom_target(run
    COMMAND ${CMAKE_COMMAND} -E env SOLAYA_INITRD=${SOLAYA_BUILDROOT_CPIO}
            ${SOLAYA_CARGO} run --release
    DEPENDS solaya-bin buildroot-all
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Running solaya in QEMU"
)

add_custom_target(run-fb
    COMMAND ${CMAKE_COMMAND} -E env SOLAYA_INITRD=${SOLAYA_BUILDROOT_CPIO}
            ${SOLAYA_CARGO} run --release -- --fb
    DEPENDS solaya-bin buildroot-all
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Running solaya with framebuffer"
)

add_custom_target(debug
    COMMAND ${CMAKE_SOURCE_DIR}/scripts/debug.sh
    DEPENDS solaya-bin buildroot-all
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Launching QEMU (paused) + GDB in tmux; scripts/debug.sh for args"
)

add_custom_target(attach
    COMMAND ${CMAKE_SOURCE_DIR}/scripts/attach.sh
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Attaching GDB to the currently running QEMU"
)

add_custom_target(disasm
    COMMAND ${SOLAYA_CROSS_BIN}/riscv64-linux-musl-objdump
        -d --demangle
        --disassembler-color=on
        ${CMAKE_SOURCE_DIR}/target/riscv64gc-unknown-none-elf/release/boot
    DEPENDS solaya-bin
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Disassembling boot ELF"
)
