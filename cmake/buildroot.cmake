# cmake/buildroot.cmake — download + build buildroot, produce rootfs.cpio.
#
# Layout:
#   .buildroot/_dl/            cached tarball download (survives `rm -rf build`)
#   .buildroot/src/            extracted buildroot source
#   .buildroot/output/         buildroot's O= build dir (per the LTS conventions)
#   .buildroot/overlay/        staged Rust binaries + /etc skeleton for buildroot
#                              to merge into the rootfs at packaging time
#   .buildroot/output/images/rootfs.cpio   final artifact consumed by QEMU -initrd
#
# Why ${CMAKE_SOURCE_DIR}/.buildroot (outside build/): cold-build cost is
# ~20 min for gcc + musl + coreutils + busybox.  Keeping the output outside
# build/ means `rm -rf build` doesn't force a rebuild, matching the
# .toolchain/ policy introduced in commit c94bf499.

include(ExternalProject)
# Order matters: checksums.cmake references SOLAYA_BUILDROOT_VERSION from
# pins.cmake when building the tarball URL.
include(${CMAKE_SOURCE_DIR}/toolchain/pins.cmake)
include(${CMAKE_SOURCE_DIR}/cmake/checksums.cmake)

# Tarball SHA256 gate.  Configure-time check — if the pin is still the
# placeholder string, emit a stub `buildroot-all` that fails loudly at
# build time rather than a cryptic ExternalProject parser error at
# configure.  User workflow: bump SOLAYA_BUILDROOT_VERSION +
# SOLAYA_BUILDROOT_SHA256 in cmake/checksums.cmake, then reconfigure.
if(SOLAYA_BUILDROOT_SHA256 MATCHES "^[a-f0-9]+$" AND NOT SOLAYA_BUILDROOT_SHA256 STREQUAL "")
    string(LENGTH "${SOLAYA_BUILDROOT_SHA256}" _br_sha_len)
    if(_br_sha_len EQUAL 64)
        set(_br_pin_ready ON)
    endif()
endif()

if(NOT _br_pin_ready)
    message(STATUS
        "buildroot: pin not set (${SOLAYA_BUILDROOT_VERSION} / "
        "SHA256=${SOLAYA_BUILDROOT_SHA256}). `buildroot-all` will error "
        "at build time; update cmake/checksums.cmake to enable.")
    add_custom_target(buildroot-all
        COMMAND ${CMAKE_COMMAND} -E cmake_echo_color --red --bold
                "buildroot-all: SOLAYA_BUILDROOT_SHA256 is a placeholder; "
                "paste a real 64-char SHA256 into cmake/checksums.cmake."
        COMMAND false
    )
    return()
endif()

set(_br_root    "${CMAKE_SOURCE_DIR}/.buildroot")
set(_br_dl      "${_br_root}/_dl")
set(_br_src     "${_br_root}/src/buildroot-${SOLAYA_BUILDROOT_VERSION}")
set(_br_out     "${_br_root}/output")
set(_br_overlay "${_br_root}/overlay")
set(_br_cpio    "${_br_out}/images/rootfs.cpio")

set(SOLAYA_BUILDROOT_OVERLAY_DIR        "${_br_overlay}")
set(SOLAYA_BUILDROOT_POST_BUILD_SCRIPT  "${CMAKE_SOURCE_DIR}/scripts/buildroot-post-build.sh")

# Materialize the defconfig template into the build dir; absolute paths
# (overlay, post-build script, busybox config) get resolved here.
set(_br_defconfig "${CMAKE_BINARY_DIR}/solaya_buildroot.defconfig")
configure_file(
    "${CMAKE_SOURCE_DIR}/configs/solaya_riscv64_buildroot_defconfig.in"
    "${_br_defconfig}"
    @ONLY
)

# Fetch + extract buildroot source. No configure/build/install — that's
# driven by add_custom_command below so we can depend on overlay staging.
ExternalProject_Add(buildroot-src
    URL               ${SOLAYA_BUILDROOT_URL}
    URL_HASH          SHA256=${SOLAYA_BUILDROOT_SHA256}
    DOWNLOAD_DIR      "${_br_dl}"
    SOURCE_DIR        "${_br_src}"
    CONFIGURE_COMMAND ""
    BUILD_COMMAND     ""
    INSTALL_COMMAND   ""
    BUILD_BYPRODUCTS  "${_br_src}/Makefile"
)

# Stage Solaya's Rust binaries into the buildroot overlay.  Runs every
# build to pick up userspace changes; stage_overlay.cmake is idempotent.
add_custom_target(buildroot-overlay
    COMMAND ${CMAKE_COMMAND} -E make_directory "${_br_overlay}/bin"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${_br_overlay}/etc/init.d"
    COMMAND ${CMAKE_COMMAND}
            -DSRC=${SOLAYA_USERSPACE_ARTIFACT_DIR}
            -DDST=${_br_overlay}/bin
            -P ${CMAKE_SOURCE_DIR}/cmake/stage_overlay.cmake
    COMMAND ${CMAKE_COMMAND} -E copy
            "${CMAKE_SOURCE_DIR}/configs/overlay/etc/inittab"
            "${_br_overlay}/etc/inittab"
    COMMAND ${CMAKE_COMMAND} -E copy
            "${CMAKE_SOURCE_DIR}/configs/overlay/etc/init.d/rcS"
            "${_br_overlay}/etc/init.d/rcS"
    DEPENDS userspace-rust
    COMMENT "Staging Solaya Rust binaries + /etc into buildroot overlay"
    VERBATIM
)

# Run buildroot.  `make defconfig BR2_DEFCONFIG=…` applies our template;
# `make` builds the whole rootfs.  `-j` left implicit — buildroot's own
# per-package parallelism plus CMake's job server handles it.
add_custom_command(
    OUTPUT "${_br_cpio}"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${_br_out}"
    COMMAND make -C "${_br_src}" O=${_br_out} defconfig BR2_DEFCONFIG=${_br_defconfig}
    COMMAND make -C "${_br_src}" O=${_br_out}
    DEPENDS buildroot-src buildroot-overlay
            "${_br_defconfig}"
            "${SOLAYA_BUILDROOT_POST_BUILD_SCRIPT}"
            "${CMAKE_SOURCE_DIR}/configs/overlay/etc/inittab"
            "${CMAKE_SOURCE_DIR}/configs/overlay/etc/init.d/rcS"
    COMMENT "Building buildroot rootfs.cpio (cold: ~20 min; warm: seconds)"
    USES_TERMINAL
    VERBATIM
)

add_custom_target(buildroot-all
    DEPENDS "${_br_cpio}"
    COMMENT "Buildroot rootfs cpio ready at ${_br_cpio}"
)

# Export the cpio path so qemu.cmake / tests.cmake / etc. can pass it
# via -initrd without hardcoding.
set(SOLAYA_BUILDROOT_CPIO "${_br_cpio}" CACHE INTERNAL "Path to buildroot rootfs.cpio")
