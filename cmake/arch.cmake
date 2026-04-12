# cmake/arch.cmake — derive SOLAYA_ARCH / toolchain prefix / triple once at the
# top level so every subdirectory (toolchain, userspace, kernel) can reference
# the same paths.
#
# Runs after kconfig.cmake (which materialises ${CMAKE_BINARY_DIR}/.config).
# Outputs, visible in every subdirectory:
#
#   SOLAYA_ARCH        riscv64 (only arch supported today)
#   SOLAYA_TC_TRIPLE   riscv64-unknown-linux-musl
#   SOLAYA_TC_ROOT     where the cross-toolchain installs land. Default
#                      ${CMAKE_SOURCE_DIR}/.toolchain so `rm -rf build` does
#                      NOT discard the ~1h toolchain build. Override with
#                      -DSOLAYA_TC_ROOT=/path/to/shared/toolchains.
#   SOLAYA_TC_PREFIX   ${SOLAYA_TC_ROOT}/${SOLAYA_ARCH}
#   SOLAYA_TC_BIN      ${SOLAYA_TC_PREFIX}/bin
#   SOLAYA_RUST_CHANNEL  channel string from rust-toolchain.toml
#
# Paths are NOT validated here — toolchain-all has not run yet on first
# configure.  Consumers reference the paths; cargo / objcopy / nm invocations
# only need them to exist at build time.

set(_dotconfig "${CMAKE_BINARY_DIR}/.config")
if(NOT EXISTS "${_dotconfig}")
    message(FATAL_ERROR
        "cmake/arch.cmake: ${_dotconfig} not found.  kconfig.cmake must be "
        "included before arch.cmake."
    )
endif()

file(STRINGS "${_dotconfig}" _arch_lines REGEX "^CONFIG_ARCH=")
if(_arch_lines)
    list(GET _arch_lines 0 _arch_line)
    string(REGEX REPLACE "^CONFIG_ARCH=\"?([^\"]+)\"?$" "\\1" SOLAYA_ARCH "${_arch_line}")
else()
    file(STRINGS "${_dotconfig}" _arch_bool REGEX "^CONFIG_ARCH_[A-Z0-9_]+=y")
    if(_arch_bool MATCHES "CONFIG_ARCH_RISCV64=y")
        set(SOLAYA_ARCH "riscv64")
    else()
        message(FATAL_ERROR
            "cmake/arch.cmake: no CONFIG_ARCH selection found in ${_dotconfig}."
        )
    endif()
endif()

if(NOT SOLAYA_ARCH STREQUAL "riscv64")
    message(FATAL_ERROR
        "cmake/arch.cmake: only SOLAYA_ARCH=riscv64 is wired up today "
        "(got '${SOLAYA_ARCH}')."
    )
endif()
set(SOLAYA_TC_TRIPLE "riscv64-unknown-linux-musl")

if(NOT DEFINED SOLAYA_TC_ROOT)
    set(SOLAYA_TC_ROOT "${CMAKE_SOURCE_DIR}/.toolchain" CACHE PATH
        "Root of the bootstrapped cross-toolchain installs (outside build/)"
    )
endif()
set(SOLAYA_TC_PREFIX "${SOLAYA_TC_ROOT}/${SOLAYA_ARCH}")
set(SOLAYA_TC_BIN    "${SOLAYA_TC_PREFIX}/bin")

# rust-toolchain.toml is respected by rustup only when the CWD is inside the
# project tree.  `cargo install <crate>` builds under the cargo registry dir,
# where rustup can't find it — so we read the channel here and export
# RUSTUP_TOOLCHAIN into cargo invocations that escape the project tree.
file(STRINGS "${CMAKE_SOURCE_DIR}/rust-toolchain.toml" _rt_lines REGEX "^channel")
if(NOT _rt_lines)
    message(FATAL_ERROR
        "cmake/arch.cmake: could not parse channel from rust-toolchain.toml."
    )
endif()
list(GET _rt_lines 0 _rt_line)
string(REGEX REPLACE "^channel[ \t]*=[ \t]*\"([^\"]+)\".*$" "\\1"
       SOLAYA_RUST_CHANNEL "${_rt_line}")
