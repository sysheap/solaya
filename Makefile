# Convenience aliases over `cmake --build build --target X`.
#
# Nothing here is load-bearing — every target is one line of cmake. Kept
# because `make build` is universal muscle memory for OS developers. Edit
# CMakeLists.txt / cmake/*.cmake to change behaviour; this file is pure
# passthrough.
#
# On first checkout:
#   make configure       # cmake --preset riscv64-virt
#   make toolchain       # cmake --build build --target toolchain-all (~1h)
# then:
#   make                 # default: build the kernel binary

BUILD_DIR ?= build

.PHONY: all configure build toolchain clean \
        run run-fb debug attach disasm \
        test test-unit test-system \
        clippy miri fmt-check ci \
        menuconfig savedefconfig olddefconfig \
        mcp-server gdb-mcp-server mcp-servers

all: build

configure:
	cmake --preset riscv64-virt

build:
	cmake --build $(BUILD_DIR) --target solaya-bin

toolchain:
	cmake --build $(BUILD_DIR) --target toolchain-all

run:
	cmake --build $(BUILD_DIR) --target run

run-fb:
	cmake --build $(BUILD_DIR) --target run-fb

debug:
	cmake --build $(BUILD_DIR) --target debug

attach:
	cmake --build $(BUILD_DIR) --target attach

disasm:
	cmake --build $(BUILD_DIR) --target disasm

test: test-unit test-system

test-unit:
	cmake --build $(BUILD_DIR) --target test-unit

test-system:
	cmake --build $(BUILD_DIR) --target test-system

clippy:
	cmake --build $(BUILD_DIR) --target clippy

miri:
	cmake --build $(BUILD_DIR) --target miri

fmt-check:
	cmake --build $(BUILD_DIR) --target fmt-check

ci:
	cmake --build $(BUILD_DIR) --target ci

menuconfig:
	cmake --build $(BUILD_DIR) --target menuconfig

savedefconfig:
	cmake --build $(BUILD_DIR) --target savedefconfig

olddefconfig:
	cmake --build $(BUILD_DIR) --target olddefconfig

mcp-server:
	cmake --build $(BUILD_DIR) --target mcp-server

gdb-mcp-server:
	cmake --build $(BUILD_DIR) --target gdb-mcp-server

mcp-servers:
	cmake --build $(BUILD_DIR) --target mcp-servers

clean:
	rm -rf $(BUILD_DIR)
