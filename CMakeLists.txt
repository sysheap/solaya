# Solaya top-level build file.
#
# User entry points via `just X` (see justfile — thin wrappers over cmake):
#
#   build            default — kernel binary + buildroot rootfs (cmake: solaya)
#   configure        cmake --preset riscv64-virt
#   toolchain        build cross-toolchain (cmake: toolchain-all, ~1h first time)
#   run, run-fb      run in QEMU
#   debug [ARGS]     tmux + GDB (see scripts/debug.sh for arg forms)
#   attach           attach GDB to a running QEMU
#   addr2line ADDR   resolve a kernel address to file:line
#   disasm           objdump the boot ELF
#   test, test-unit, test-system [TEST]
#   miri, clippy, fmt-check, ci
#   menuconfig, savedefconfig, olddefconfig   Kconfig UI
#   mcp-server, gdb-mcp-server, mcp-servers   AI agent MCP servers
#   clean            remove build/
#
# Additional cmake-only targets (invoke via `cmake --build build --target X`):
#
#   solaya                    kernel ELF + buildroot cpio (default of `build`)
#   solaya-bin                kernel ELF only, no rootfs
#   userspace-all             userspace rust bins + dash + doom
#   headers-generated         run bindgen-driver -> build/headers/generated/
#   fetch-deps                prefetch cargo deps
#   tftp-deploy, reboot-hw, picocom   hardware deploy flows

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

add_subdirectory(toolchain)
# doom depends on compiler-rt-builtins from toolchain/, so include it
# only after that subdir has been processed.  dash comes from buildroot
# now (see cmake/buildroot.cmake).
include(doom)
add_subdirectory(userspace)
# buildroot must come before tests/qemu/tools: those reference
# ${SOLAYA_BUILDROOT_CPIO} at parse time (baked into their COMMAND
# strings), so the variable has to be defined first. It also consumes
# SOLAYA_USERSPACE_ARTIFACT_DIR, which add_subdirectory(userspace)
# finalises.
include(buildroot)
include(tests)
include(qemu)
include(tools)
add_subdirectory(crates/kernel)

# `solaya` (default target) — bare `cmake --build build` produces both
# the kernel binary and the buildroot rootfs cpio that QEMU's -initrd
# needs at runtime.  Without this dependency, first-time users had to
# remember to run `--target buildroot-all` separately.
add_custom_target(solaya ALL DEPENDS solaya-bin buildroot-all mcp-servers)
