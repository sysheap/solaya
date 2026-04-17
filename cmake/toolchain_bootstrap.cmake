# cmake/toolchain_bootstrap.cmake — riscv64-linux-musl sysroot bootstrap.
#
# The actual compiler chain (clang / lld / llvm-*) is whatever LLVM >= 18
# the host distro ships; cmake/llvm_tools.cmake probes PATH and
# cmake/clang_wrapper.cmake emits thin `riscv64-linux-musl-*` shell wrappers
# under ${SOLAYA_CROSS_BIN}.  This file just stages a sysroot at
# ${SOLAYA_TC_PREFIX}/${SOLAYA_TC_TRIPLE}/ so clang --sysroot= finds musl
# headers/libraries plus the Linux UAPI.
#
# Dependency chain (for ${SOLAYA_ARCH}=riscv64):
#
#   linux-headers  ──▶  musl  ──▶  toolchain-all
#
# qemu-system-riscv64 is NOT bootstrapped here — it's taken from the host
# (see README.md host prerequisites).  See cmake/toolchain/riscv64.cmake for
# the PATH lookup.
#
# Install layout (prefix = ${SOLAYA_TC_ROOT}/${SOLAYA_ARCH}):
#
#   <prefix>/${SOLAYA_TC_TRIPLE}/usr/include   Linux UAPI + musl headers
#   <prefix>/${SOLAYA_TC_TRIPLE}/usr/lib       musl static + shared libs
#   <prefix>/${SOLAYA_TC_TRIPLE}/src/musl/     preserved musl sources (GDB)

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
        "today (got '${SOLAYA_ARCH}')."
    )
endif()

if(NOT DEFINED SOLAYA_CROSS_BIN)
    message(FATAL_ERROR
        "toolchain_bootstrap.cmake: SOLAYA_CROSS_BIN not defined. "
        "cmake/clang_wrapper.cmake must run before add_subdirectory(toolchain)."
    )
endif()

set(SOLAYA_TC_TRIPLE  "riscv64-unknown-linux-musl")
set(SOLAYA_TC_PREFIX  "${SOLAYA_TC_ROOT}/${SOLAYA_ARCH}")
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
    DOWNLOAD_DIR               "${SOLAYA_TC_ROOT}/_dl"
    USES_TERMINAL_DOWNLOAD     ON
    USES_TERMINAL_CONFIGURE    ON
    USES_TERMINAL_BUILD        ON
    USES_TERMINAL_INSTALL      ON
)

# ----------------------------------------------------------------------------
# 1. linux-headers: UAPI headers for musl to build against.
#    ARCH=riscv is the kernel Makefile's name; it maps to both riscv32 and
#    riscv64 depending on CONFIG_64BIT (which does not affect header layout).
#    We install into <sysroot>/usr so musl's configure finds <linux/*.h>.
# ----------------------------------------------------------------------------
set(_linux_arch "riscv")
ExternalProject_Add(linux-headers
    URL       "${SOLAYA_LINUX_HEADERS_URL}"
    URL_HASH  SHA256=${SOLAYA_LINUX_HEADERS_SHA256}
    # STAMP_DIR lives under the install prefix (cached in CI) so step-done
    # markers survive across builds with a fresh ${CMAKE_BINARY_DIR}; without
    # this, a cache-restored install dir still triggers a full rebuild because
    # the default stamps in build/toolchain/<pkg>-prefix/src/<pkg>-stamp/ are
    # missing. Same rationale for the musl ExternalProject_Add block below.
    STAMP_DIR "${SOLAYA_TC_ROOT}/_stamp/linux-headers"
    ${_EP_COMMON}
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
# 2. musl: static + shared C library that userspace programs link against.
#
#    Built with the distro clang wrapper (cmake/clang_wrapper.cmake), which
#    injects --target=riscv64-linux-musl and --sysroot into every compile.
#    At this stage the sysroot only contains the linux-headers install;
#    musl's configure tolerates a partially populated sysroot because it
#    builds with -nostdinc and uses its own internal headers.
#
#    We preserve the source tree at <sysroot>/src/musl so GDB can resolve
#    file:line for musl frames when debugging userspace inside Solaya.
#
#    --disable-optimize keeps -O0 so debuggability is maximal.  We never
#    invoke strip on musl's installed artefacts.
# ----------------------------------------------------------------------------
set(_musl_cc     "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-clang")
set(_musl_ar     "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-ar")
set(_musl_ranlib "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-ranlib")
ExternalProject_Add(musl
    URL       "${SOLAYA_MUSL_URL}"
    URL_HASH  SHA256=${SOLAYA_MUSL_SHA256}
    STAMP_DIR "${SOLAYA_TC_ROOT}/_stamp/musl"
    ${_EP_COMMON}
    DEPENDS   linux-headers
    CONFIGURE_COMMAND
        ${CMAKE_COMMAND} -E env
            "CC=${_musl_cc}"
            "AR=${_musl_ar}"
            "RANLIB=${_musl_ranlib}"
        <SOURCE_DIR>/configure
            --prefix=${SOLAYA_TC_SYSROOT}/usr
            --target=${SOLAYA_TC_TRIPLE}
            --syslibdir=${SOLAYA_TC_SYSROOT}/usr/lib
            --disable-optimize
            # Static only.  Userspace crates link statically via
            # -C target-feature=+crt-static (see userspace/.cargo/config.toml);
            # building libc.so with clang requires compiler-rt builtins
            # (__addtf3, __floatditf, ...) for riscv64 long-double soft-float
            # helpers that most distros don't package for cross targets.
            # Avoid the dependency by not building the shared libc.
            --disable-shared
            --enable-static
    BUILD_COMMAND   make ${_J}
    INSTALL_COMMAND
        ${CMAKE_COMMAND} -E make_directory ${SOLAYA_TC_SYSROOT}/src/musl
        COMMAND make ${_J} install
        # Preserve source tree for GDB.  `-E copy_directory` is idempotent.
        COMMAND ${CMAKE_COMMAND} -E copy_directory
            <SOURCE_DIR> ${SOLAYA_TC_SYSROOT}/src/musl
)

# ----------------------------------------------------------------------------
# 3. compiler-rt builtins: static library of LLVM's runtime helpers
#    (__addtf3, __floatditf, __muldi3, …) plus clang_rt.crtbegin.o and
#    clang_rt.crtend.o.  Clang's driver emits references to these on every
#    link; distro packages ship them for the host triple only, so we
#    compile them against the musl sysroot for riscv64-linux-musl and stage
#    them under ${SOLAYA_COMPILER_RT_DIR}/lib/<triple>/ where clang's
#    -rtlib=compiler-rt -resource-dir wiring in cmake/clang_wrapper.cmake
#    expects them.
#
#    Source is the compiler-rt standalone tarball from an LLVM release;
#    the version is decoupled from the host's clang because builtin ABI is
#    stable across LLVM releases.
# ----------------------------------------------------------------------------
set(_crt_install "${SOLAYA_COMPILER_RT_DIR}/lib/riscv64-unknown-linux-musl")
set(_crt_legacy  "${SOLAYA_COMPILER_RT_DIR}/lib/linux")

ExternalProject_Add(compiler-rt-builtins
    URL       "${SOLAYA_COMPILER_RT_URL}"
    URL_HASH  SHA256=${SOLAYA_COMPILER_RT_SHA256}
    STAMP_DIR "${SOLAYA_TC_ROOT}/_stamp/compiler-rt"
    ${_EP_COMMON}
    DEPENDS   musl
    CONFIGURE_COMMAND ""
    BUILD_COMMAND     ""
    INSTALL_COMMAND
        ${CMAKE_COMMAND} -E make_directory "${_crt_install}"
        COMMAND bash
            "${CMAKE_SOURCE_DIR}/cmake/build_compiler_rt_builtins.sh"
            "<SOURCE_DIR>"
            "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-clang"
            "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-ar"
            "${_crt_install}"
            "${SOLAYA_BUILD_PARALLEL}"
        # Clang's legacy (pre-per-target) search path is
        # <resource-dir>/lib/linux/libclang_rt.builtins-<arch>.a; populate it
        # too so both lookup schemes resolve.
        COMMAND ${CMAKE_COMMAND} -E make_directory "${_crt_legacy}"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${_crt_install}/libclang_rt.builtins.a"
            "${_crt_legacy}/libclang_rt.builtins-riscv64.a"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${_crt_install}/clang_rt.crtbegin.o"
            "${_crt_legacy}/clang_rt.crtbegin.o"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${_crt_install}/clang_rt.crtend.o"
            "${_crt_legacy}/clang_rt.crtend.o"
)

# ----------------------------------------------------------------------------
# Aggregate target: one entrypoint that builds every sysroot package above.
# ----------------------------------------------------------------------------
add_custom_target(toolchain-all
    DEPENDS linux-headers musl compiler-rt-builtins
    COMMENT "Building Solaya riscv64-musl sysroot + compiler-rt builtins"
)
