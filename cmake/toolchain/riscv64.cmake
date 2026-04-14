# cmake/toolchain/riscv64.cmake — CMake toolchain file for RISC-V 64 targets.
#
# Used with `-DCMAKE_TOOLCHAIN_FILE=cmake/toolchain/riscv64.cmake` by
# external CMake consumers (out-of-tree builds).  Expects the clang-wrapper
# scripts and the musl + linux-headers sysroot produced by
# `cmake --build build --target toolchain-all` to already exist.

set(CMAKE_SYSTEM_NAME      Generic)
set(CMAKE_SYSTEM_PROCESSOR riscv64)

set(SOLAYA_TC_TRIPLE "riscv64-unknown-linux-musl")

# CMAKE_BINARY_DIR is not defined when a toolchain file is processed in a
# child project configure phase, so we accept SOLAYA_TC_PREFIX + SOLAYA_CROSS_BIN
# as inputs (envs or -D) and fall back to deterministic defaults relative to
# the source tree.
if(NOT DEFINED SOLAYA_TC_PREFIX)
    if(DEFINED ENV{SOLAYA_TC_PREFIX})
        set(SOLAYA_TC_PREFIX "$ENV{SOLAYA_TC_PREFIX}")
    else()
        # Standard layout: the sysroot lives at <repo-root>/.toolchain/riscv64
        # (moved out of build/ so `rm -rf build` does not nuke minutes of work).
        get_filename_component(_this_dir "${CMAKE_CURRENT_LIST_DIR}" DIRECTORY)
        get_filename_component(_src_root "${_this_dir}"              DIRECTORY)
        set(SOLAYA_TC_PREFIX "${_src_root}/.toolchain/riscv64")
    endif()
endif()

if(NOT DEFINED SOLAYA_CROSS_BIN)
    if(DEFINED ENV{SOLAYA_CROSS_BIN})
        set(SOLAYA_CROSS_BIN "$ENV{SOLAYA_CROSS_BIN}")
    else()
        get_filename_component(_this_dir "${CMAKE_CURRENT_LIST_DIR}" DIRECTORY)
        get_filename_component(_src_root "${_this_dir}"              DIRECTORY)
        set(SOLAYA_CROSS_BIN "${_src_root}/build/toolchain/bin")
    endif()
endif()

set(_cc  "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-clang")

if(NOT EXISTS "${_cc}")
    message(FATAL_ERROR
        "riscv64-linux-musl-clang wrapper not found at ${_cc}. "
        "Run the top-level configure first (cmake --preset riscv64-virt), "
        "then `cmake --build build --target toolchain-all` to stage the musl "
        "sysroot."
    )
endif()

set(CMAKE_C_COMPILER    "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-clang")
set(CMAKE_CXX_COMPILER  "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-clang++")
set(CMAKE_AR            "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-ar"        CACHE FILEPATH "")
set(CMAKE_NM            "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-nm"        CACHE FILEPATH "")
set(CMAKE_RANLIB        "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-ranlib"    CACHE FILEPATH "")
set(CMAKE_OBJCOPY       "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-objcopy"   CACHE FILEPATH "")
set(CMAKE_OBJDUMP       "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-objdump"   CACHE FILEPATH "")
set(CMAKE_ADDR2LINE     "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-addr2line" CACHE FILEPATH "")
set(CMAKE_STRIP         "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-strip"     CACHE FILEPATH "")

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
