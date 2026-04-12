# cmake/dash.cmake — build a static riscv64-musl dash and expose it as both
# `dash` and `sh` in the userspace artifact dir.
#
# Previously provided by the nix flake's shellHook (symlinking a nix-store
# dash into kernel/compiled_userspace_nix/).  Now the tarball is fetched +
# cross-compiled against the bootstrapped gcc/musl chain.
#
# The init process (userspace/src/bin/init.rs) hard-codes `spawn("dash", …)`,
# so without this target `cmake --build build --target run` and every
# system-tests invocation fail at the first spawn with ENOENT.
#
# Emits one target:  `dash` — drops the static ELF at
#                    ${SOLAYA_USERSPACE_ARTIFACT_DIR}/dash and symlinks
#                    ${SOLAYA_USERSPACE_ARTIFACT_DIR}/sh -> dash.

include(ExternalProject)
include(${CMAKE_SOURCE_DIR}/cmake/checksums.cmake)

if(NOT DEFINED SOLAYA_TC_PREFIX)
    message(FATAL_ERROR
        "cmake/dash.cmake: SOLAYA_TC_PREFIX not defined. "
        "cmake/arch.cmake must run before include(dash)."
    )
endif()

set(_dash_prefix   "${CMAKE_BINARY_DIR}/userspace/dash-prefix")
set(_dash_install  "${_dash_prefix}/install")
set(_dash_bin      "${_dash_install}/bin/dash")
set(_dash_cc       "${SOLAYA_TC_BIN}/${SOLAYA_TC_TRIPLE}-gcc")

# Build flags: static link against musl. --host= gets dash's autotools to
# produce a riscv64 binary; --with-libs="" avoids pulling in host libs.
ExternalProject_Add(dash-build
    URL       "${SOLAYA_DASH_URL}"
    URL_HASH  SHA256=${SOLAYA_DASH_SHA256}
    DOWNLOAD_EXTRACT_TIMESTAMP OFF
    DOWNLOAD_DIR               "${CMAKE_BINARY_DIR}/toolchain/_dl"
    USES_TERMINAL_DOWNLOAD     ON
    USES_TERMINAL_CONFIGURE    ON
    USES_TERMINAL_BUILD        ON
    USES_TERMINAL_INSTALL      ON
    DEPENDS                    gcc-stage2 musl
    CONFIGURE_COMMAND
        ${CMAKE_COMMAND} -E env
            "CC=${_dash_cc}"
            "CFLAGS=-static -Os"
            "LDFLAGS=-static"
        <SOURCE_DIR>/configure
            --host=${SOLAYA_TC_TRIPLE}
            --prefix=${_dash_install}
            --enable-static
    BUILD_COMMAND     make
    INSTALL_COMMAND   make install
)

# Stage the binary into the userspace artifact dir (where crates/kernel/
# build.rs picks it up for include_bytes_align_as!).  Symlink sh -> dash so
# anything that execve's "sh" also works.
add_custom_command(
    OUTPUT  "${SOLAYA_USERSPACE_ARTIFACT_DIR}/dash"
            "${SOLAYA_USERSPACE_ARTIFACT_DIR}/sh"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SOLAYA_USERSPACE_ARTIFACT_DIR}"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${_dash_bin}"
            "${SOLAYA_USERSPACE_ARTIFACT_DIR}/dash"
    COMMAND ${CMAKE_COMMAND} -E create_symlink
            "dash" "${SOLAYA_USERSPACE_ARTIFACT_DIR}/sh"
    DEPENDS dash-build
    COMMENT "Staging dash (+ sh symlink) into userspace artifact dir"
    VERBATIM
)

add_custom_target(dash ALL
    DEPENDS "${SOLAYA_USERSPACE_ARTIFACT_DIR}/dash"
            "${SOLAYA_USERSPACE_ARTIFACT_DIR}/sh"
)
