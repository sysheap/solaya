# cmake/dash.cmake — build a static riscv64-musl dash and expose it as both
# `dash` and `sh` in the userspace artifact dir.
#
# Previously provided by the nix flake's shellHook (symlinking a nix-store
# dash into kernel/compiled_userspace_nix/).  Now the tarball is fetched +
# cross-compiled against distro clang via the cmake/clang_wrapper.cmake
# shims that target riscv64-linux-musl and link with lld.
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

if(NOT DEFINED SOLAYA_CROSS_BIN)
    message(FATAL_ERROR
        "cmake/dash.cmake: SOLAYA_CROSS_BIN not defined. "
        "cmake/clang_wrapper.cmake must run before include(dash)."
    )
endif()

set(_dash_prefix   "${CMAKE_BINARY_DIR}/userspace/dash-prefix")
set(_dash_install  "${_dash_prefix}/install")
set(_dash_bin      "${_dash_install}/bin/dash")
set(_dash_cc       "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-clang")

# Build flags: static link against musl. --host= gets dash's autotools to
# produce a riscv64 binary; --with-libs="" avoids pulling in host libs.
ExternalProject_Add(dash-build
    URL       "${SOLAYA_DASH_URL}"
    URL_HASH  SHA256=${SOLAYA_DASH_SHA256}
    # Preserve the tarball's mtimes so make sees aclocal.m4 / Makefile.in as
    # newer than configure.ac and skips the autoreconf regen path — otherwise
    # the `missing` script demands the exact upstream automake (aclocal-1.16)
    # even if the host ships a newer one.
    DOWNLOAD_EXTRACT_TIMESTAMP ON
    DOWNLOAD_DIR               "${SOLAYA_TC_ROOT}/_dl"
    USES_TERMINAL_DOWNLOAD     ON
    USES_TERMINAL_CONFIGURE    ON
    USES_TERMINAL_BUILD        ON
    USES_TERMINAL_INSTALL      ON
    DEPENDS                    musl compiler-rt-builtins
    CONFIGURE_COMMAND
        ${CMAKE_COMMAND} -E env
            "CC=${_dash_cc}"
            "CFLAGS=-static -Os"
            "LDFLAGS=-static"
        <SOURCE_DIR>/configure
            --host=${SOLAYA_CROSS_TRIPLE}
            --prefix=${_dash_install}
            --enable-static
    BUILD_COMMAND     make -j${SOLAYA_BUILD_PARALLEL}
    INSTALL_COMMAND   make -j${SOLAYA_BUILD_PARALLEL} install
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
