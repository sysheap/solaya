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

set(SOLAYA_COMPILER_RT_URL    "https://github.com/llvm/llvm-project/releases/download/llvmorg-22.1.3/llvm-project-22.1.3.src.tar.xz")
set(SOLAYA_COMPILER_RT_SHA256 "2488c33a959eafba1c44f253e5bbe7ac958eb53fa626298a3a5f4b87373767cd")
# source: github.com/llvm/llvm-project/releases/tag/llvmorg-22.1.3 — the
# release page lists the SHA256 in the download grid.  From LLVM 22 on,
# standalone component tarballs are no longer published, so this is the
# full llvm-project monorepo tarball; the toolchain bootstrap points the
# compiler-rt build script at <SOURCE_DIR>/compiler-rt.

set(SOLAYA_MUSL_URL        "https://musl.libc.org/releases/musl-1.2.6.tar.gz")
set(SOLAYA_MUSL_SHA256     "d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a")
# source: matches pkgver/sha of musl in Alpine aports
#   https://git.alpinelinux.org/aports/plain/main/musl/APKBUILD
#   pkgver=1.2.6

set(SOLAYA_LINUX_HEADERS_URL    "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.18.22.tar.xz")
set(SOLAYA_LINUX_HEADERS_SHA256 "a23c92faf3657385c2c6b5f4edd8f81b808907ebe603fa30699eae224da55f59")
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
set(SOLAYA_DOOMGENERIC_REV   "dcb7a8dbc7a16ce3dda29382ac9aae9d77d21284")

# doom1.wad — the shareware Doom IWAD that ships with the Doom engine as
# demo content.  Fetched at build time with SHA256 verification.
set(SOLAYA_DOOM_WAD_URL      "https://distro.ibiblio.org/slitaz/sources/packages/d/doom1.wad")
set(SOLAYA_DOOM_WAD_SHA256   "1d7d43be501e67d927e415e0b8f3e29c3bf33075e859721816f652a526cac771")
# source: slitaz distro mirror, the same URL the retired nix flake used.

# Buildroot source tarball — pin the exact 2025.02.x point release and its
# SHA256 before enabling the buildroot-all target.  Placeholder values here;
# replace from https://buildroot.org/downloads/ + the accompanying .sha256
# file on the same page (the release SHA is published next to the tarball).
set(SOLAYA_BUILDROOT_URL     "https://buildroot.org/downloads/buildroot-${SOLAYA_BUILDROOT_VERSION}.tar.xz")
set(SOLAYA_BUILDROOT_SHA256  "REPLACE_WITH_ACTUAL_SHA256_FROM_BUILDROOT_ORG")

