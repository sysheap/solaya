# toolchain/pins.cmake — bare version strings for the cross-toolchain bootstrap.
#
# This file is intentionally separate from cmake/checksums.cmake because
# changing a version is a much bigger deal than refreshing a hash; the split
# helps review.  When you bump a version here, you must also update the URL +
# SHA256 pair in cmake/checksums.cmake.

set(SOLAYA_BINUTILS_VERSION      "2.43.1")
set(SOLAYA_GCC_VERSION           "14.2.0")
set(SOLAYA_MUSL_VERSION          "1.2.5")
set(SOLAYA_LINUX_HEADERS_VERSION "6.12.7")
set(SOLAYA_DASH_VERSION          "0.5.12")
