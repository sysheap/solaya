# cmake/bindgen.cmake — invoke tools/bindgen-driver from CMake.
#
# Builds tools/bindgen-driver as a host binary and exposes a
# `headers-generated` aggregate target that produces:
#
#   ${SOLAYA_HEADERS_GENERATED}/syscalls.rs
#   ${SOLAYA_HEADERS_GENERATED}/syscall_types.rs
#   ${SOLAYA_HEADERS_GENERATED}/errno.rs
#   ${SOLAYA_HEADERS_GENERATED}/socket_types.rs
#   ${SOLAYA_HEADERS_GENERATED}/fs_types.rs
#   ${SOLAYA_HEADERS_GENERATED}/sysinfo_types.rs
#
# crates/headers/build.rs is a thin copy-shim that reads from this dir
# (hardcoded relative to its CARGO_MANIFEST_DIR, matching the binaryDir
# pinned by CMakePresets.json) and stages the files into OUT_DIR so
# crates/headers/src/lib.rs can include! them.

set(SOLAYA_HEADERS_GENERATED "${CMAKE_BINARY_DIR}/headers/generated"
    CACHE PATH "Output directory for bindgen-generated Rust bindings."
)

set(SOLAYA_BINDGEN_DRIVER_DIR    "${CMAKE_SOURCE_DIR}/tools/bindgen-driver")
set(SOLAYA_BINDGEN_DRIVER_TARGET "${SOLAYA_BINDGEN_DRIVER_DIR}/target/x86_64-unknown-linux-gnu/release/bindgen-driver")

# Default header sources: the bootstrapped toolchain's sysroot, populated by
# the linux-headers + musl ExternalProjects.  Both linux UAPI and musl land
# under the same <sysroot>/usr/include/ dir, so the two variables point at
# the same path by default.
if(NOT DEFINED SOLAYA_BINDGEN_LINUX_HEADERS)
    set(SOLAYA_BINDGEN_LINUX_HEADERS "${SOLAYA_TC_PREFIX}/${SOLAYA_TC_TRIPLE}/usr/include"
        CACHE PATH "Directory containing asm/, asm-generic/, linux/ UAPI headers."
    )
endif()
if(NOT DEFINED SOLAYA_BINDGEN_MUSL_HEADERS)
    set(SOLAYA_BINDGEN_MUSL_HEADERS "${SOLAYA_TC_PREFIX}/${SOLAYA_TC_TRIPLE}/usr/include"
        CACHE PATH "Directory containing musl libc headers (sys/, netinet/, dirent.h, ...)."
    )
endif()

# Step 1: build the bindgen-driver binary.
add_custom_command(
    OUTPUT  "${SOLAYA_BINDGEN_DRIVER_TARGET}"
    COMMAND ${SOLAYA_CARGO} build --release
                --manifest-path "${SOLAYA_BINDGEN_DRIVER_DIR}/Cargo.toml"
    WORKING_DIRECTORY "${SOLAYA_BINDGEN_DRIVER_DIR}"
    COMMENT "Building tools/bindgen-driver (host)"
    VERBATIM
)
add_custom_target(bindgen-driver
    DEPENDS "${SOLAYA_BINDGEN_DRIVER_TARGET}"
)

# Step 2: run it against the configured header dirs.  We declare individual
# OUTPUT files so downstream targets can DEPENDS on the exact artifact they
# need.  The driver emits all six files in one pass; declaring them all on
# one custom_command avoids re-running it per dependent.
set(_generated_files
    "${SOLAYA_HEADERS_GENERATED}/syscalls.rs"
    "${SOLAYA_HEADERS_GENERATED}/syscall_types.rs"
    "${SOLAYA_HEADERS_GENERATED}/errno.rs"
    "${SOLAYA_HEADERS_GENERATED}/socket_types.rs"
    "${SOLAYA_HEADERS_GENERATED}/fs_types.rs"
    "${SOLAYA_HEADERS_GENERATED}/sysinfo_types.rs"
)

add_custom_command(
    OUTPUT  ${_generated_files}
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SOLAYA_HEADERS_GENERATED}"
    COMMAND "${SOLAYA_BINDGEN_DRIVER_TARGET}"
            --out-dir        "${SOLAYA_HEADERS_GENERATED}"
            --linux-headers  "${SOLAYA_BINDGEN_LINUX_HEADERS}"
            --musl-headers   "${SOLAYA_BINDGEN_MUSL_HEADERS}"
    DEPENDS "${SOLAYA_BINDGEN_DRIVER_TARGET}"
    COMMENT "Generating Rust header bindings via bindgen-driver"
    VERBATIM
)

add_custom_target(headers-generated
    DEPENDS ${_generated_files} linux-headers musl
    COMMENT "bindgen-driver output (${SOLAYA_HEADERS_GENERATED})"
)
