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

# Override find_program() before anything else can call it (directly or
# via find_package). Caches bare names instead of absolute paths so the
# same build/ dir works from both the host and the podman devcontainer.
include(portable_find_program)

include(kconfig)
include(cargo_targets)

# Derive arch + toolchain prefix/triple from .config so every subdirectory
# (toolchain, userspace, kernel) agrees on the sysroot layout without having
# to re-parse Kconfig.  toolchain/CMakeLists.txt populates the sysroot at
# ${SOLAYA_TC_PREFIX}/${SOLAYA_TC_TRIPLE}/ (musl + linux-headers) via
# ExternalProject.  The compiler toolchain itself (clang/lld/llvm-*) is
# distro-provided: llvm_tools.cmake probes PATH for LLVM >= 18, and
# clang_wrapper.cmake emits thin `riscv64-linux-musl-*` wrappers under
# ${SOLAYA_CROSS_BIN} (build dir) that consumers invoke by name.
include(arch)
include(llvm_tools)
include(clang_wrapper)

# Shared path for userspace artefacts. Defined at the top level (before the
# tests/qemu includes reference it) so the value is baked into every
# add_custom_target env; userspace/CMakeLists.txt re-declares it with CACHE
# semantics so the value survives reconfigure.
set(SOLAYA_USERSPACE_ARTIFACT_DIR "${CMAKE_BINARY_DIR}/userspace/artifacts"
    CACHE PATH "Where userspace binaries are deposited for kernel embedding."
)

include(bindgen)
include(tests)
include(qemu)
include(tools)

add_subdirectory(toolchain)
# dash + doom depend on gcc-stage2 + musl ExternalProject targets declared
# inside toolchain/, so include them only after that subdir has been
# processed.
include(dash)
include(doom)
add_subdirectory(userspace)
add_subdirectory(crates/kernel)

# `solaya` (default target) — just an alias for solaya-bin so bare
# `cmake --build build` builds the kernel.
add_custom_target(solaya ALL DEPENDS solaya-bin)
