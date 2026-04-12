# cmake/toolchain_bootstrap.cmake — cross-toolchain bootstrap via ExternalProject.
#
# Dependency chain (for ${SOLAYA_ARCH}=riscv64):
#
#   binutils  ──▶  gcc-stage1  ──▶  linux-headers  ──▶  musl  ──▶  gcc-stage2
#                                                                    │
#                                                                    ▼
#                                                           toolchain-all
#
# qemu-system-riscv64 is NOT bootstrapped here — it's taken from the host
# (see README.md host prerequisites).  See cmake/toolchain/riscv64.cmake for
# the PATH lookup.
#
# Install layout (prefix = ${CMAKE_BINARY_DIR}/toolchain/${SOLAYA_ARCH}):
#
#   <prefix>/bin/                       — binutils + gcc wrappers
#   <prefix>/${SOLAYA_TC_TRIPLE}/       — sysroot (standard gcc layout)
#   <prefix>/${SOLAYA_TC_TRIPLE}/usr/   — linux UAPI headers, musl headers/libs
#   <prefix>/${SOLAYA_TC_TRIPLE}/src/musl/ — preserved musl sources for GDB
#
# Consumers should add `<prefix>/bin` to PATH (or use the generated toolchain
# file at cmake/toolchain/${SOLAYA_ARCH}.cmake, which does this for them).

include(ExternalProject)
include(ProcessorCount)

include(${CMAKE_SOURCE_DIR}/toolchain/pins.cmake)
include(${CMAKE_SOURCE_DIR}/cmake/checksums.cmake)

if(NOT DEFINED SOLAYA_ARCH)
    message(FATAL_ERROR
        "toolchain_bootstrap.cmake: SOLAYA_ARCH not set. "
        "This file is included from toolchain/CMakeLists.txt, which derives "
        "SOLAYA_ARCH from CONFIG_ARCH in build/.config."
    )
endif()

if(NOT SOLAYA_ARCH STREQUAL "riscv64")
    message(FATAL_ERROR
        "toolchain_bootstrap.cmake: only SOLAYA_ARCH=riscv64 is supported "
        "today (got '${SOLAYA_ARCH}').  aarch64 / x86_64 bootstrap is "
        "planned for stage-3 of drifting-gliding-eclipse.md."
    )
endif()

set(SOLAYA_TC_TRIPLE  "riscv64-unknown-linux-musl")
set(SOLAYA_TC_PREFIX  "${CMAKE_BINARY_DIR}/toolchain/${SOLAYA_ARCH}")
set(SOLAYA_TC_SYSROOT "${SOLAYA_TC_PREFIX}/${SOLAYA_TC_TRIPLE}")

# Inner-build parallelism for ExternalProject make invocations.
#
# Top-level Ninja runs ExternalProject as one job per package, so without an
# explicit -jN here each package's inner `make` would be serial.  We pick a
# default from the host's processor count and let users override via
# -DSOLAYA_BUILD_PARALLEL=N on the command line.  Do NOT hard-code `nproc`:
# users in containers with cgroup limits need the override.
if(NOT DEFINED SOLAYA_BUILD_PARALLEL)
    ProcessorCount(_solaya_nproc)
    if(_solaya_nproc EQUAL 0)
        set(_solaya_nproc 1)
    endif()
    set(SOLAYA_BUILD_PARALLEL "${_solaya_nproc}" CACHE STRING
        "Parallelism for inner ExternalProject make invocations."
    )
endif()
set(_J "-j${SOLAYA_BUILD_PARALLEL}")

# Shared ExternalProject options: never rewrite timestamps on extract, keep
# the download cache in the build tree.
set(_EP_COMMON
    DOWNLOAD_EXTRACT_TIMESTAMP OFF
    DOWNLOAD_DIR               "${CMAKE_BINARY_DIR}/toolchain/_dl"
    USES_TERMINAL_DOWNLOAD     ON
    USES_TERMINAL_CONFIGURE    ON
    USES_TERMINAL_BUILD        ON
    USES_TERMINAL_INSTALL      ON
)

# ----------------------------------------------------------------------------
# 1. binutils: cross-assembler/linker/ar/ranlib/nm/objcopy/objdump/addr2line.
# ----------------------------------------------------------------------------
ExternalProject_Add(binutils
    URL       "${SOLAYA_BINUTILS_URL}"
    URL_HASH  SHA256=${SOLAYA_BINUTILS_SHA256}
    ${_EP_COMMON}
    CONFIGURE_COMMAND
        <SOURCE_DIR>/configure
            --prefix=${SOLAYA_TC_PREFIX}
            --target=${SOLAYA_TC_TRIPLE}
            --with-sysroot=${SOLAYA_TC_SYSROOT}
            --disable-nls
            --disable-werror
            --disable-multilib
            --enable-deterministic-archives
            # gprofng is host-only tooling we don't need, and its
            # libcollector/dispatcher.c fails to build under GCC 15+ due to
            # incompatible weak-alias declarations vs glibc's prototypes.
            --disable-gprofng
    BUILD_COMMAND   make ${_J}
    INSTALL_COMMAND make ${_J} install
)

# ----------------------------------------------------------------------------
# 2. gcc stage-1: minimal C compiler used to build musl.  --without-headers
#    and --with-newlib together are the conventional flags for the "build a
#    compiler that has no libc yet" case; --disable-shared is required
#    because there's no musl for libgcc_s to link against.
# ----------------------------------------------------------------------------
ExternalProject_Add(gcc-stage1
    URL       "${SOLAYA_GCC_URL}"
    URL_HASH  SHA256=${SOLAYA_GCC_SHA256}
    ${_EP_COMMON}
    DEPENDS   binutils
    # gcc's own `contrib/download_prerequisites` fetches gmp/mpfr/mpc/isl
    # from gcc.gnu.org and extracts them into the source tree, where gcc's
    # configure picks them up via symlinks.  Integrity of those fetches is
    # transitively anchored on the gcc tarball's SHA256 (the script has
    # hardcoded hashes for its downloads).
    PATCH_COMMAND
        ${CMAKE_COMMAND} -E chdir <SOURCE_DIR> ./contrib/download_prerequisites
    CONFIGURE_COMMAND
        <SOURCE_DIR>/configure
            --prefix=${SOLAYA_TC_PREFIX}
            --target=${SOLAYA_TC_TRIPLE}
            --with-sysroot=${SOLAYA_TC_SYSROOT}
            --without-headers
            --with-newlib
            --disable-shared
            --enable-languages=c
            --disable-threads
            --disable-libssp
            --disable-libatomic
            --disable-libgomp
            --disable-libquadmath
            --disable-libvtv
            --disable-libstdcxx
            --disable-nls
            --disable-multilib
            --disable-decimal-float
    BUILD_COMMAND   make ${_J} all-gcc all-target-libgcc
    INSTALL_COMMAND make ${_J} install-gcc install-target-libgcc
)

# ----------------------------------------------------------------------------
# 3. linux-headers: UAPI headers for musl to build against.
#    ARCH=riscv is the kernel Makefile's name; it maps to both riscv32 and
#    riscv64 depending on CONFIG_64BIT (which does not affect header layout).
#    We install into <sysroot>/usr so musl's configure finds <linux/*.h>.
# ----------------------------------------------------------------------------
set(_linux_arch "riscv")
ExternalProject_Add(linux-headers
    URL       "${SOLAYA_LINUX_HEADERS_URL}"
    URL_HASH  SHA256=${SOLAYA_LINUX_HEADERS_SHA256}
    ${_EP_COMMON}
    DEPENDS   gcc-stage1
    CONFIGURE_COMMAND ""
    BUILD_IN_SOURCE   TRUE
    BUILD_COMMAND     ""
    INSTALL_COMMAND
        make ${_J} -C <SOURCE_DIR>
            ARCH=${_linux_arch}
            INSTALL_HDR_PATH=${SOLAYA_TC_SYSROOT}/usr
            headers_install
)

# ----------------------------------------------------------------------------
# 4. musl: static C library that userspace programs link against.
#
#    We preserve the source tree at <sysroot>/src/musl so GDB can resolve
#    file:line for musl frames when debugging userspace inside Solaya.  This
#    replicates the nix overlay's `postPatch` behaviour.
#
#    --disable-optimize keeps -O0 so debuggability is maximal.  We never
#    invoke strip on musl's installed artefacts (no INSTALL_COMMAND variant
#    that strips).
# ----------------------------------------------------------------------------
set(_musl_cc "${SOLAYA_TC_PREFIX}/bin/${SOLAYA_TC_TRIPLE}-gcc")
ExternalProject_Add(musl
    URL       "${SOLAYA_MUSL_URL}"
    URL_HASH  SHA256=${SOLAYA_MUSL_SHA256}
    ${_EP_COMMON}
    DEPENDS   gcc-stage1 linux-headers
    CONFIGURE_COMMAND
        ${CMAKE_COMMAND} -E env
            "CC=${_musl_cc}"
            "AR=${SOLAYA_TC_PREFIX}/bin/${SOLAYA_TC_TRIPLE}-ar"
            "RANLIB=${SOLAYA_TC_PREFIX}/bin/${SOLAYA_TC_TRIPLE}-ranlib"
        <SOURCE_DIR>/configure
            --prefix=${SOLAYA_TC_SYSROOT}/usr
            --target=${SOLAYA_TC_TRIPLE}
            --syslibdir=${SOLAYA_TC_SYSROOT}/usr/lib
            --disable-optimize
            # Both static and shared.  Userspace crates link statically via
            # -C target-feature=+crt-static (see userspace/.cargo/config.toml),
            # but gcc-stage2's libgcc_s.so build needs libc.so to exist.
            --enable-shared
            --enable-static
    BUILD_COMMAND   make ${_J}
    INSTALL_COMMAND
        ${CMAKE_COMMAND} -E make_directory ${SOLAYA_TC_SYSROOT}/src/musl
        COMMAND make ${_J} install
        # Preserve source tree for GDB.  `-E copy_directory` is idempotent.
        COMMAND ${CMAKE_COMMAND} -E copy_directory
            <SOURCE_DIR> ${SOLAYA_TC_SYSROOT}/src/musl
        # gcc's stage2 libgcc build looks for headers/libs at the canonical
        # sysroot layout (<sysroot>/include, <sysroot>/lib), but musl +
        # linux-headers installed into <sysroot>/usr/{include,lib}.  Bridge
        # the layouts: <sysroot>/include is a symlink to usr/include, and
        # each file in usr/lib gets a symlink inside <sysroot>/lib (which
        # already exists — binutils populates it with ldscripts).
        COMMAND ${CMAKE_COMMAND} -E create_symlink
            usr/include ${SOLAYA_TC_SYSROOT}/include
        COMMAND ${CMAKE_COMMAND} -E make_directory ${SOLAYA_TC_SYSROOT}/lib
        COMMAND ${CMAKE_COMMAND}
            -D "SOLAYA_SYSROOT_LIB=${SOLAYA_TC_SYSROOT}/lib"
            -P "${CMAKE_SOURCE_DIR}/cmake/bridge_sysroot_lib.cmake"
)

# ----------------------------------------------------------------------------
# 5. gcc stage-2: full cross-compiler with libstdc++ and shared-library
#    support, built against the sysroot populated by stage-1 + musl + linux
#    headers.  Reinstalls into the same prefix, overwriting stage-1 wrappers.
# ----------------------------------------------------------------------------
ExternalProject_Add(gcc-stage2
    URL       "${SOLAYA_GCC_URL}"
    URL_HASH  SHA256=${SOLAYA_GCC_SHA256}
    ${_EP_COMMON}
    DEPENDS   binutils musl linux-headers
    PATCH_COMMAND
        ${CMAKE_COMMAND} -E chdir <SOURCE_DIR> ./contrib/download_prerequisites
    CONFIGURE_COMMAND
        <SOURCE_DIR>/configure
            --prefix=${SOLAYA_TC_PREFIX}
            --target=${SOLAYA_TC_TRIPLE}
            --with-sysroot=${SOLAYA_TC_SYSROOT}
            --enable-languages=c,c++
            --enable-shared
            --enable-threads=posix
            --enable-tls
            --disable-nls
            --disable-multilib
            --disable-libsanitizer
    BUILD_COMMAND   make ${_J}
    INSTALL_COMMAND make ${_J} install
)

# ----------------------------------------------------------------------------
# Aggregate target: one entrypoint that builds every package above.
# ----------------------------------------------------------------------------
add_custom_target(toolchain-all
    DEPENDS binutils gcc-stage1 linux-headers musl gcc-stage2
    COMMENT "Building full Solaya cross-toolchain (${SOLAYA_ARCH})"
)
