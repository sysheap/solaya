# userspace/CMakeLists.txt — build userspace rust binaries + coreutils.
#
# Outputs land in ${SOLAYA_USERSPACE_ARTIFACT_DIR} (build/userspace/artifacts
# by default). kernel/build.rs reads this dir via the
# SOLAYA_USERSPACE_ARTIFACT_DIR env var and include_bytes!'s the contents
# into the kernel ELF.

cmake_minimum_required(VERSION 3.25)
project(solaya_userspace LANGUAGES NONE)

set(SOLAYA_USERSPACE_ARTIFACT_DIR "${CMAKE_BINARY_DIR}/userspace/artifacts"
    CACHE PATH "Where userspace binaries are deposited for kernel embedding."
)

solaya_read_kconfig_features(_us_features userspace)
set(_us_features_arg "")
if(_us_features)
    set(_us_features_arg "--features=${_us_features}")
endif()

# Rust target for userspace binaries. Hardcoded today (riscv64 only); when
# aarch64 / x86_64 land this moves to CONFIG_RUST_TARGET_USERSPACE derived
# from Kconfig.
set(_rust_target "riscv64gc-unknown-linux-musl")

# The userspace linker resolves `riscv64-unknown-linux-musl-gcc` by name (via
# userspace/.cargo/config.toml [target.*] linker = ...); prepend the cross
# toolchain's bin dir to PATH for every cargo invocation below so that gcc
# resolves without the user having to do anything.
set(_path_with_tc "${SOLAYA_TC_BIN}:$ENV{PATH}")

# -----------------------------------------------------------------------------
# userspace-rust: `cargo build --bins` for the workspace at userspace/.
# -----------------------------------------------------------------------------
add_custom_target(userspace-rust
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SOLAYA_USERSPACE_ARTIFACT_DIR}"
    COMMAND ${CMAKE_COMMAND} -E env
        "PATH=${_path_with_tc}"
        "CARGO_TARGET_DIR=${CMAKE_BINARY_DIR}/userspace/target"
        "SOLAYA_HEADERS_GENERATED=${SOLAYA_HEADERS_GENERATED}"
        ${SOLAYA_CARGO} build
            --bins
            --release
            --target        "${_rust_target}"
            --artifact-dir  "${SOLAYA_USERSPACE_ARTIFACT_DIR}"
            -Z unstable-options
            ${_us_features_arg}
    # Run from userspace/ so cargo picks up userspace/.cargo/config.toml
    # (which sets the riscv64 linker).  With --manifest-path from a parent
    # dir cargo finds only the parent's .cargo/config.toml and the linker
    # override is lost.
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}/userspace"
    DEPENDS headers-generated
    USES_TERMINAL
    VERBATIM
    COMMENT "Building userspace Rust binaries (${_rust_target})"
)

# -----------------------------------------------------------------------------
# coreutils: one multicall binary, symlinked for each applet name.
# Feature list matches the justfile's build-coreutils recipe verbatim.
# -----------------------------------------------------------------------------
set(_coreutils_features "cat,echo,false,ls,mkdir,pwd,rm,touch,true")
set(_coreutils_root     "${CMAKE_BINARY_DIR}/userspace/coreutils-root")

string(REPLACE "," ";" _coreutils_applets "${_coreutils_features}")

# Build the symlink commands as a list so CMake emits one COMMAND per applet.
set(_symlink_commands "")
foreach(_applet IN LISTS _coreutils_applets)
    list(APPEND _symlink_commands
        COMMAND ${CMAKE_COMMAND} -E create_symlink
            "coreutils"
            "${SOLAYA_USERSPACE_ARTIFACT_DIR}/${_applet}"
    )
endforeach()

add_custom_target(coreutils
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SOLAYA_USERSPACE_ARTIFACT_DIR}"
    COMMAND ${CMAKE_COMMAND} -E env
        "PATH=${_path_with_tc}"
        # `cargo install` compiles under ~/.cargo/registry/..., outside the
        # project tree, so rustup can't find rust-toolchain.toml there — pin
        # the toolchain explicitly.
        "RUSTUP_TOOLCHAIN=${SOLAYA_RUST_CHANNEL}"
        "CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_MUSL_LINKER=riscv64-unknown-linux-musl-gcc"
        "CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_MUSL_RUSTFLAGS=-C target-feature=+crt-static"
        ${SOLAYA_CARGO} install coreutils@0.7.0
            --target      ${_rust_target}
            --no-default-features
            --features    "${_coreutils_features}"
            --root        "${_coreutils_root}"
            --target-dir  "${_coreutils_root}/build"
            --force
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${_coreutils_root}/bin/coreutils"
        "${SOLAYA_USERSPACE_ARTIFACT_DIR}/coreutils"
    ${_symlink_commands}
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    VERBATIM
    COMMENT "Building coreutils (${_coreutils_features})"
)

# -----------------------------------------------------------------------------
# Aggregate: everything a kernel build needs from userspace.
# -----------------------------------------------------------------------------
add_custom_target(userspace-all
    DEPENDS userspace-rust coreutils dash
    COMMENT "Userspace artifact set (rust bins + coreutils + dash/sh)"
)
