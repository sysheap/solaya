#!/usr/bin/env bash
# Copy solaya.bin into the TFTP served dir.
#
# Args:
#   $1  (optional) path to the kernel binary. Defaults to
#       $REPO/build/kernel/solaya.bin, matching the default CMake build dir.
#       CMake's tftp-deploy target passes ${CMAKE_BINARY_DIR}/kernel/solaya.bin
#       so alternate build dirs (e.g. -B build-rel) work too.
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
BIN="${1:-$REPO/build/kernel/solaya.bin}"
TFTP_DIR="$REPO/target/tftp"
mkdir -p "$TFTP_DIR"
cp "$BIN" "$TFTP_DIR/solaya.bin"
echo "Deployed to $TFTP_DIR/solaya.bin"
