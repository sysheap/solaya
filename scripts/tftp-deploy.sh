#!/usr/bin/env bash
# Copy solaya.bin into the TFTP served dir.
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
TFTP_DIR="$REPO/target/tftp"
mkdir -p "$TFTP_DIR"
cp "$REPO/target/solaya.bin" "$TFTP_DIR/solaya.bin"
echo "Deployed to $TFTP_DIR/solaya.bin"
