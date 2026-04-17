# Convenience recipes over `cmake --build build --target X` plus a few
# arg-taking helpers that are awkward to express as CMake targets.
#
# Nothing here is load-bearing — CMake owns the build graph. Edit
# CMakeLists.txt / cmake/*.cmake for behaviour changes; this file is
# passthrough.
#
# On first checkout:
#   just configure      # cmake --preset riscv64-virt
#   just toolchain      # cmake --build build --target toolchain-all (~2 min)
# then:
#   just                # default: build the kernel binary
#
# Use `just --list` for a listing of recipes.

BUILD_DIR := env_var_or_default("BUILD_DIR", "build")
KERNEL_ELF := "target/riscv64gc-unknown-none-elf/release/boot"

default: build

configure:
    cmake --preset riscv64-virt

build:
    cmake --build {{BUILD_DIR}} --target solaya-bin

toolchain:
    cmake --build {{BUILD_DIR}} --target toolchain-all

run:
    cmake --build {{BUILD_DIR}} --target run

run-fb:
    cmake --build {{BUILD_DIR}} --target run-fb

disasm:
    cmake --build {{BUILD_DIR}} --target disasm

# Debug session with optional [FUNC] or [USERBIN FUNC] breakpoints — see
# scripts/debug.sh for the full arg grammar.
debug *ARGS:
    ./scripts/debug.sh {{ARGS}}

attach:
    ./scripts/attach.sh

# Resolve a kernel address to file:line via the cross-toolchain's
# llvm-addr2line wrapper.  Useful when triaging a panic backtrace.
addr2line ADDR:
    {{BUILD_DIR}}/toolchain/bin/riscv64-linux-musl-addr2line -e {{KERNEL_ELF}} -f -p -i {{ADDR}}

test: test-unit test-system

test-unit:
    cmake --build {{BUILD_DIR}} --target test-unit

# With no TEST, runs the whole suite via CMake.  With a TEST argument,
# forwards it to cargo-nextest so you can iterate on a single test
# (restores the loop-system-test ergonomics from the old justfile).
test-system *TEST:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -z "{{TEST}}" ]; then
        cmake --build {{BUILD_DIR}} --target test-system
    else
        cargo nextest run --release \
            --manifest-path system-tests/Cargo.toml \
            --target x86_64-unknown-linux-gnu \
            {{TEST}}
    fi

clippy:
    cmake --build {{BUILD_DIR}} --target clippy

shellcheck:
    cmake --build {{BUILD_DIR}} --target shellcheck

miri:
    cmake --build {{BUILD_DIR}} --target miri

fmt-check:
    cmake --build {{BUILD_DIR}} --target fmt-check

ci:
    cmake --build {{BUILD_DIR}} --target ci

menuconfig:
    cmake --build {{BUILD_DIR}} --target menuconfig

savedefconfig:
    cmake --build {{BUILD_DIR}} --target savedefconfig

olddefconfig:
    cmake --build {{BUILD_DIR}} --target olddefconfig

mcp-server:
    cmake --build {{BUILD_DIR}} --target mcp-server

gdb-mcp-server:
    cmake --build {{BUILD_DIR}} --target gdb-mcp-server

mcp-servers:
    cmake --build {{BUILD_DIR}} --target mcp-servers

clean:
    rm -rf {{BUILD_DIR}}
