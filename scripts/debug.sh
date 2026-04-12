#!/usr/bin/env bash
# Spawn a tmux session with Solaya running in QEMU (paused via --wait) on
# the top pane and GDB (pwndbg) attached on the bottom pane. Replaces the
# inline tmux logic of the old `just debug` / `just debugf` / `just debuguf`
# recipes.
#
# Usage:
#   scripts/debug.sh                  # plain debug session
#   scripts/debug.sh FUNC             # set hbreak on FUNC before continue
#   scripts/debug.sh USERBIN FUNC     # debug inside a userspace binary USERBIN
#                                     # (staged under build/userspace/artifacts/,
#                                     # override with SOLAYA_USERSPACE_ARTIFACT_DIR)

set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO"

KERNEL="$REPO/target/riscv64gc-unknown-none-elf/release/boot"
USERSPACE_DIR="${SOLAYA_USERSPACE_ARTIFACT_DIR:-$REPO/build/userspace/artifacts}"
GDB='pwndbg --nh -iex "add-auto-load-safe-path ."'
RUN='cargo run --release -- --wait'

BRK=""
BIN="$KERNEL"

case $# in
    0) ;;
    1) BRK="-ex \"hbreak $1\" -ex c" ;;
    2) BRK="-ex \"hbreak $2\" -ex c"; BIN="$USERSPACE_DIR/$1" ;;
    *) echo "usage: $0 [FUNC | USERBIN FUNC]" >&2; exit 1 ;;
esac

GDB_CMD="while [ ! -f $REPO/.gdb-port ]; do sleep 0.1; done; $GDB -ex \"target remote :\$(cat $REPO/.gdb-port)\" $BRK $BIN"

exec tmux new-session -d "$RUN" \; split-window -v "$GDB_CMD" \; attach
