# cmake/toolchain/riscv64.cmake — CMake toolchain file for RISC-V 64 targets.
#
# Used with `-DCMAKE_TOOLCHAIN_FILE=cmake/toolchain/riscv64.cmake` by stage-6+
# consumers (kernel, userspace, out-of-tree builds).  Expects the cross
# toolchain to already be installed under ${SOLAYA_TC_ROOT}/riscv64 (default
# ${CMAKE_SOURCE_DIR}/.toolchain/riscv64) by
# `cmake --build build --target toolchain-all`.

set(CMAKE_SYSTEM_NAME      Generic)
set(CMAKE_SYSTEM_PROCESSOR riscv64)

set(SOLAYA_TC_TRIPLE "riscv64-unknown-linux-musl")

# CMAKE_BINARY_DIR is not defined when a toolchain file is processed in a
# child project configure phase, so we accept SOLAYA_TC_PREFIX as an input
# and fall back to a deterministic default relative to the source tree.
if(NOT DEFINED SOLAYA_TC_PREFIX)
    if(DEFINED ENV{SOLAYA_TC_PREFIX})
        set(SOLAYA_TC_PREFIX "$ENV{SOLAYA_TC_PREFIX}")
    else()
        # Standard layout: the toolchain lives at <repo-root>/.toolchain/riscv64
        # (moved out of build/ so `rm -rf build` does not nuke ~1h of work).
        get_filename_component(_this_dir "${CMAKE_CURRENT_LIST_DIR}" DIRECTORY)
        get_filename_component(_src_root "${_this_dir}"              DIRECTORY)
        set(SOLAYA_TC_PREFIX "${_src_root}/.toolchain/riscv64")
    endif()
endif()

set(_bin "${SOLAYA_TC_PREFIX}/bin")
set(_cc  "${_bin}/${SOLAYA_TC_TRIPLE}-gcc")

if(NOT EXISTS "${_cc}")
    message(FATAL_ERROR
        "riscv64 cross-toolchain not found at ${_cc}. "
        "Run `cmake --build build --target toolchain-all` first "
        "(≈1h build time on first invocation)."
    )
endif()

set(CMAKE_C_COMPILER    "${_bin}/${SOLAYA_TC_TRIPLE}-gcc")
set(CMAKE_CXX_COMPILER  "${_bin}/${SOLAYA_TC_TRIPLE}-g++")
set(CMAKE_AR            "${_bin}/${SOLAYA_TC_TRIPLE}-ar"        CACHE FILEPATH "")
set(CMAKE_NM            "${_bin}/${SOLAYA_TC_TRIPLE}-nm"        CACHE FILEPATH "")
set(CMAKE_RANLIB        "${_bin}/${SOLAYA_TC_TRIPLE}-ranlib"    CACHE FILEPATH "")
set(CMAKE_OBJCOPY       "${_bin}/${SOLAYA_TC_TRIPLE}-objcopy"   CACHE FILEPATH "")
set(CMAKE_OBJDUMP       "${_bin}/${SOLAYA_TC_TRIPLE}-objdump"   CACHE FILEPATH "")
set(CMAKE_ADDR2LINE     "${_bin}/${SOLAYA_TC_TRIPLE}-addr2line" CACHE FILEPATH "")
set(CMAKE_STRIP         "${_bin}/${SOLAYA_TC_TRIPLE}-strip"     CACHE FILEPATH "")

set(CMAKE_SYSROOT       "${SOLAYA_TC_PREFIX}/${SOLAYA_TC_TRIPLE}")
set(CMAKE_FIND_ROOT_PATH "${CMAKE_SYSROOT}")

set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# qemu-system-riscv64 is taken from the host PATH (not bootstrapped).  See
# README.md for the install hint (Debian: qemu-system-misc; Fedora:
# qemu-system-riscv).
find_program(SOLAYA_QEMU qemu-system-riscv64
    DOC "QEMU system emulator for Solaya's target architecture."
)
if(NOT SOLAYA_QEMU)
    message(FATAL_ERROR
        "qemu-system-riscv64 not found on PATH.  Install it via your "
        "package manager (Debian/Ubuntu: qemu-system-misc; Fedora: "
        "qemu-system-riscv) — see README.md."
    )
endif()

# Cargo picks up the linker from CARGO_TARGET_<TARGET>_LINKER env vars.
# Setting them in the toolchain file propagates them into every CMake
# invocation that inherits this file via execute_process/add_custom_target,
# so consumers that call cargo through CMake get the cross-linker for free.
set(ENV{CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_MUSL_LINKER} "${CMAKE_C_COMPILER}")
set(ENV{CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_MUSL_RUNNER} "${SOLAYA_QEMU}")
set(ENV{CC_riscv64gc_unknown_linux_musl}                   "${CMAKE_C_COMPILER}")
set(ENV{AR_riscv64gc_unknown_linux_musl}                   "${CMAKE_AR}")
