# toolchain/pins.cmake — bare version strings for the cross-toolchain bootstrap.
#
# This file is intentionally separate from cmake/checksums.cmake because
# changing a version is a much bigger deal than refreshing a hash; the split
# helps review.  When you bump a version here, you must also update the URL +
# SHA256 pair in cmake/checksums.cmake.

set(SOLAYA_MUSL_VERSION          "1.2.5")
set(SOLAYA_LINUX_HEADERS_VERSION "6.12.7")
set(SOLAYA_DASH_VERSION          "0.5.12")
# compiler-rt builtins: pinned to a released LLVM version (not the host's),
# since builtin ABI is stable across LLVM releases.  Built against the musl
# sysroot with the host's distro clang — no external prebuilt artefacts.
set(SOLAYA_COMPILER_RT_VERSION   "18.1.8")
# doomgeneric has no tags; pin by commit hash in cmake/checksums.cmake.
