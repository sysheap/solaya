# Solaya top-level build file.
#
# User entry points (invoke via `make X` or `cmake --build build --target X`):
#
#   solaya           default — build kernel ELF + patch symbols
#   solaya-bin       emit the flat boot binary (build/kernel/solaya.bin)
#   userspace-all    build userspace rust bins + coreutils
#   headers-generated  run bindgen-driver -> build/headers/generated/
#   toolchain-all    build cross-toolchain from source (~1h first time)
#   run, run-fb      run in QEMU
#   debug, attach    tmux + GDB flows (see scripts/debug.sh for arg forms)
#   disasm           objdump the boot ELF
#   test-unit, test-system, miri, clippy, fmt-check, ci
#   menuconfig, savedefconfig, olddefconfig   Kconfig UI
#   mcp-server, fetch-deps
#   tftp-deploy, reboot-hw, picocom           hardware deploy flows

cmake_minimum_required(VERSION 3.25)
project(solaya LANGUAGES NONE)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_SOURCE_DIR}/cmake")

include(kconfig)
include(cargo_targets)

# Derive arch + toolchain prefix/triple from .config so every subdirectory
# (toolchain, userspace, kernel) agrees on where the cross-toolchain lives
# without having to re-parse Kconfig.  toolchain/CMakeLists.txt populates the
# tree under ${SOLAYA_TC_PREFIX}/bin via ExternalProject; kernel and userspace
# consume binaries from there.
include(arch)

include(bindgen)
include(tests)
include(qemu)
include(tools)

add_subdirectory(toolchain)
add_subdirectory(userspace)
add_subdirectory(crates/kernel)

# `solaya` (default target) — just an alias for solaya-bin so bare
# `cmake --build build` builds the kernel.
add_custom_target(solaya ALL DEPENDS solaya-bin)
