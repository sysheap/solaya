#!/usr/bin/env bash
# Attach GDB (pwndbg) to a QEMU instance already running with --gdb.
# Reads .gdb-port written by qemu_wrapper.sh to find the open GDB port.

set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
KERNEL="$REPO/target/riscv64gc-unknown-none-elf/release/boot"

if [ ! -f "$REPO/.gdb-port" ]; then
    echo "Error: $REPO/.gdb-port not found. Is QEMU running with --gdb?" >&2
    exit 1
fi

exec pwndbg --nh \
    -iex "add-auto-load-safe-path ." \
    -ex "target remote :$(cat "$REPO/.gdb-port")" \
    "$KERNEL"
