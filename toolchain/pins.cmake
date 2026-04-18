# toolchain/pins.cmake — bare version strings for the cross-toolchain bootstrap.
#
# This file is intentionally separate from cmake/checksums.cmake because
# changing a version is a much bigger deal than refreshing a hash; the split
# helps review.  When you bump a version here, you must also update the URL +
# SHA256 pair in cmake/checksums.cmake.

set(SOLAYA_MUSL_VERSION          "1.2.6")
set(SOLAYA_LINUX_HEADERS_VERSION "6.18.22")
set(SOLAYA_DASH_VERSION          "0.5.12")
# Buildroot LTS we consume for busybox init + dash + GNU coreutils + rootfs
# cpio packaging.  Use the latest 2025.02.x point release.
set(SOLAYA_BUILDROOT_VERSION     "2025.02.x")
# compiler-rt builtins: pinned to a released LLVM version (not the host's),
# since builtin ABI is stable across LLVM releases.  Built against the musl
# sysroot with the host's distro clang — no external prebuilt artefacts.
# From LLVM 22 on, upstream stopped publishing a standalone compiler-rt
# tarball; we fetch the full llvm-project source and point the builder at
# its compiler-rt/ subdirectory (see cmake/toolchain_bootstrap.cmake).
set(SOLAYA_COMPILER_RT_VERSION   "22.1.3")
# doomgeneric has no tags; pin by commit hash in cmake/checksums.cmake.
