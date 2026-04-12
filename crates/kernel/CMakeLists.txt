# crates/kernel/CMakeLists.txt — CMake-native build of the kernel ELF + binary.
#
# Three targets:
#   solaya-kernel-elf  cargo build -p boot (shares repo target/ with dev flows)
#   patch-symbols      nm + objcopy --update-section; rewrites ELF in-place
#   solaya-bin         objcopy -O binary -> build/kernel/solaya.bin
#
# SOLAYA_NM / SOLAYA_OBJCOPY point at the cross-toolchain binaries produced by
# the toolchain-all target.  They are NOT validated at configure time: on a
# fresh checkout `cmake --preset ...` runs before `toolchain-all`, so the
# binaries don't exist yet.  The paths resolve at build time.

cmake_minimum_required(VERSION 3.25)
project(solaya_kernel LANGUAGES NONE)

set(SOLAYA_NM      "${SOLAYA_TC_BIN}/${SOLAYA_TC_TRIPLE}-nm")
set(SOLAYA_OBJCOPY "${SOLAYA_TC_BIN}/${SOLAYA_TC_TRIPLE}-objcopy")

set(_kernel_target "riscv64gc-unknown-none-elf")
set(_kernel_elf    "${CMAKE_SOURCE_DIR}/target/${_kernel_target}/release/boot")
set(_kernel_bin    "${CMAKE_BINARY_DIR}/kernel/solaya.bin")
set(_symbols_file  "${CMAKE_BINARY_DIR}/kernel/symbols")

solaya_read_kconfig_features(_kernel_features kernel)
set(_features_arg "")
if(_kernel_features)
    set(_features_arg "--features=${_kernel_features}")
endif()

add_custom_target(solaya-kernel-elf
    COMMAND ${CMAKE_COMMAND} -E env
        "SOLAYA_USERSPACE_ARTIFACT_DIR=${SOLAYA_USERSPACE_ARTIFACT_DIR}"
        "SOLAYA_HEADERS_GENERATED=${SOLAYA_HEADERS_GENERATED}"
        ${SOLAYA_CARGO} build --release
            --manifest-path "${CMAKE_SOURCE_DIR}/Cargo.toml"
            --target        "${_kernel_target}"
            -p              boot
            ${_features_arg}
    DEPENDS userspace-all headers-generated
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    VERBATIM
    COMMENT "Building solaya kernel ELF"
)

add_custom_target(patch-symbols
    COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_BINARY_DIR}/kernel"
    COMMAND bash -c
        "${SOLAYA_NM} --demangle --numeric-sort --line-numbers ${_kernel_elf} | grep -e ' t ' -e ' T ' > ${_symbols_file} && printf '\\0' >> ${_symbols_file}"
    COMMAND ${SOLAYA_OBJCOPY} --update-section symbols=${_symbols_file} ${_kernel_elf}
    DEPENDS solaya-kernel-elf
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    VERBATIM
    COMMENT "Patching symbols into boot ELF"
)

add_custom_target(solaya-bin
    COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_BINARY_DIR}/kernel"
    COMMAND ${SOLAYA_OBJCOPY} -O binary ${_kernel_elf} ${_kernel_bin}
    DEPENDS patch-symbols
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    VERBATIM
    COMMENT "Emitting ${_kernel_bin}"
)
