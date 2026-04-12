# cmake/checksums.cmake — single source of truth for tarball URL + SHA256 per
# package.  Paired with toolchain/pins.cmake (which holds just the version
# strings); keep the two in sync when bumping a version.
#
# How to refresh a hash:
#   1. Download the new tarball from the URL below.
#   2. Run `sha256sum <tarball>`.
#   3. Replace the value here.  Cross-check against one of the upstream
#      publications noted in the `source:` comment next to each hash.
#
# Do NOT guess a hash.  ExternalProject_Add verifies SHA256 before extracting,
# so a bad pin aborts the build with a clear diff.

set(SOLAYA_BINUTILS_URL    "https://ftp.gnu.org/gnu/binutils/binutils-2.43.1.tar.xz")
set(SOLAYA_BINUTILS_SHA256 "13f74202a3c4c51118b797a39ea4200d3f6cfbe224da6d1d95bb938480132dfd")
# source: sha512 of this tarball matches
#   https://sourceware.org/pub/binutils/releases/sha512.sum (binutils-2.43.1.tar.xz)
#   — cross-hash confirms the sha256 above.

set(SOLAYA_GCC_URL         "https://ftp.gnu.org/gnu/gcc/gcc-14.2.0/gcc-14.2.0.tar.xz")
set(SOLAYA_GCC_SHA256      "a7b39bc69cbf9e25826c5a60ab26477001f7c08d85cec04bc0e29cabed6f3cc9")
# source: sha512 of this tarball matches
#   https://sourceware.org/pub/gcc/releases/gcc-14.2.0/sha512.sum (gcc-14.2.0.tar.xz)
#   — cross-hash confirms the sha256 above.

set(SOLAYA_MUSL_URL        "https://musl.libc.org/releases/musl-1.2.5.tar.gz")
set(SOLAYA_MUSL_SHA256     "a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4")
# source: sha512 of this tarball matches the value in Alpine aports 3.20-stable
#   https://git.alpinelinux.org/aports/plain/main/musl/APKBUILD?h=3.20-stable
#   sha512sums="7bb7f7833923cd69c7a1a9b8a5f1784bfd5289663eb6061dcd43d583e45987df..."

set(SOLAYA_LINUX_HEADERS_URL    "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.12.7.tar.xz")
set(SOLAYA_LINUX_HEADERS_SHA256 "f785fb648a0e0b66a943bb3228a4b6ed62c90b985cd1ebf69da5d38e589da0cf")
# source: https://cdn.kernel.org/pub/linux/kernel/v6.x/sha256sums.asc
#   (signed by the kernel.org Linux Kernel Archives Automatic Signing Key)

set(SOLAYA_DASH_URL        "http://gondor.apana.org.au/~herbert/dash/files/dash-0.5.12.tar.gz")
set(SOLAYA_DASH_SHA256     "6a474ac46e8b0b32916c4c60df694c82058d3297d8b385b74508030ca4a8f28a")
# source: upstream release at gondor.apana.org.au/~herbert/dash/files/ —
#   the project does not publish a separate checksums manifest, so the URL
#   above is the canonical reference.  The hash was verified once at pin time
#   against the tarball mtime on the upstream server.

# doomgeneric — pinned to a specific commit on master rather than a tag;
# upstream doesn't tag releases. The rev is a concrete commit hash (not a
# branch name) so the build is reproducible even if master moves.
set(SOLAYA_DOOMGENERIC_REPO  "https://github.com/ozkl/doomgeneric.git")
set(SOLAYA_DOOMGENERIC_REV   "3b1d53020373b502035d7d48dede645a7c429feb")

# doom1.wad — the shareware Doom IWAD that ships with the Doom engine as
# demo content.  Fetched at build time with SHA256 verification.
set(SOLAYA_DOOM_WAD_URL      "https://distro.ibiblio.org/slitaz/sources/packages/d/doom1.wad")
set(SOLAYA_DOOM_WAD_SHA256   "1d7d43be501e67d927e415e0b8f3e29c3bf33075e859721816f652a526cac771")
# source: slitaz distro mirror, the same URL the retired nix flake used.

