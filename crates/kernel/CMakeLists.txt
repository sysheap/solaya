# crates/kernel/CMakeLists.txt — CMake-native build of the kernel ELF + binary.
#
# Three targets:
#   solaya-kernel-elf  cargo build -p boot (shares repo target/ with dev flows)
#   patch-symbols      nm + objcopy --update-section; rewrites ELF in-place
#   solaya-bin         objcopy -O binary -> build/kernel/solaya.bin
#
# The ELF and solaya.bin are Ninja-tracked outputs, so a no-op `just build`
# runs cargo (which is a fast cache hit, preserving ELF mtime) and stops
# there — patch-symbols and objcopy are skipped by Ninja's mtime check.
#
# SOLAYA_NM / SOLAYA_OBJCOPY point at the clang_wrapper.cmake-generated
# shells under ${SOLAYA_CROSS_BIN}.  The wrappers exist at configure time
# (clang_wrapper.cmake runs before add_subdirectory(crates/kernel)), but
# they forward to distro `llvm-nm-N` / `llvm-objcopy-N` binaries that must
# be on $PATH at build time.

cmake_minimum_required(VERSION 3.25)
project(solaya_kernel LANGUAGES NONE)

set(SOLAYA_NM      "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-nm")
set(SOLAYA_OBJCOPY "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-objcopy")

set(_kernel_target "riscv64gc-unknown-none-elf")
set(_kernel_elf    "${CMAKE_SOURCE_DIR}/target/${_kernel_target}/release/boot")
set(_kernel_bin    "${CMAKE_BINARY_DIR}/kernel/solaya.bin")
set(_symbols_file  "${CMAKE_BINARY_DIR}/kernel/symbols")
set(_patch_stamp   "${CMAKE_BINARY_DIR}/kernel/patch-symbols.stamp")

# cargo is authoritative for Rust dep tracking, so the target stays phony —
# cargo runs on every invocation and decides whether to re-link the ELF.
# BYPRODUCTS lets Ninja restat the ELF after cargo exits: an unchanged ELF
# (cargo preserves mtime on cache hits) keeps downstream rules up-to-date.
add_custom_target(solaya-kernel-elf
    COMMAND ${SOLAYA_CARGO} build --release
            --manifest-path "${CMAKE_SOURCE_DIR}/Cargo.toml"
            --target        "${_kernel_target}"
            -p              boot
    BYPRODUCTS ${_kernel_elf}
    DEPENDS userspace-all headers-generated
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    VERBATIM
    COMMENT "Building solaya kernel ELF"
)

# Rewrite the ELF's `symbols` section in place, but only when nm output
# differs from the current section contents — otherwise the ELF is left
# untouched. The stamp file drives Ninja's mtime check: an unchanged ELF
# stays older than the stamp, so the script is skipped on no-op rebuilds.
#
# We compare the new nm output against the ELF's current `symbols` section
# contents (not a cached sidecar file): a cached file would go stale
# whenever cargo re-links the ELF, which reverts the section to the 1-byte
# placeholder from the linker script while the cache still matches the new
# nm output — causing objcopy to be skipped and symbol resolution to
# silently break.
add_custom_command(
    OUTPUT ${_patch_stamp}
    COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_BINARY_DIR}/kernel"
    COMMAND bash -c
        "set -e; \
         ${SOLAYA_NM} --demangle --numeric-sort --line-numbers ${_kernel_elf} \
            | grep -e ' t ' -e ' T ' > ${_symbols_file}.new; \
         printf '\\0' >> ${_symbols_file}.new; \
         ${SOLAYA_OBJCOPY} --dump-section symbols=${_symbols_file}.current ${_kernel_elf}; \
         section_size=$(wc -c < ${_symbols_file}.current); \
         nm_bytes=$(wc -c < ${_symbols_file}.new); \
         if [ $nm_bytes -gt $section_size ]; then \
             echo \"error: symbol table ($nm_bytes bytes) exceeds reserved\" \
                  \"section size ($section_size); bump the reservation\" \
                  \"in crates/kernel/riscv64.ld\" >&2; \
             exit 1; \
         fi; \
         pad=$(( section_size - nm_bytes )); \
         head -c $pad /dev/zero >> ${_symbols_file}.new; \
         if cmp -s ${_symbols_file}.new ${_symbols_file}.current; then \
             rm ${_symbols_file}.new ${_symbols_file}.current; \
         else \
             mv ${_symbols_file}.new ${_symbols_file}; \
             rm -f ${_symbols_file}.current; \
             ${SOLAYA_OBJCOPY} --update-section symbols=${_symbols_file} ${_kernel_elf}; \
         fi"
    COMMAND ${CMAKE_COMMAND} -E touch ${_patch_stamp}
    DEPENDS ${_kernel_elf} solaya-kernel-elf
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    VERBATIM
    COMMENT "Patching symbols into boot ELF"
)

add_custom_target(patch-symbols DEPENDS ${_patch_stamp})

add_custom_command(
    OUTPUT ${_kernel_bin}
    COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_BINARY_DIR}/kernel"
    COMMAND ${SOLAYA_OBJCOPY} -O binary ${_kernel_elf} ${_kernel_bin}
    DEPENDS ${_patch_stamp}
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    VERBATIM
    COMMENT "Emitting ${_kernel_bin}"
)

add_custom_target(solaya-bin DEPENDS ${_kernel_bin})
