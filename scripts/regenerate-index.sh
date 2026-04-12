#!/bin/sh
# regenerate-index.sh — invoked by the CMake `index` target to regenerate
# INDEX.md via indxr.  Resolves indxr from the *build-time* PATH so the
# tool location is not baked into CMakeCache.txt at configure time — which
# otherwise breaks when configure and build run as different users (e.g.
# configure as root → cached /root/.cargo/bin/indxr → Permission denied
# for a normal user sharing the same build dir).
#
# Soft-fails when indxr is not installed so fresh clones without the tool
# still build.
set -eu

OUTPUT=${1:?usage: regenerate-index.sh OUTPUT}

if command -v indxr >/dev/null 2>&1; then
    exec indxr -q -o "$OUTPUT"
fi

echo "indxr not installed; skipping INDEX.md regeneration" >&2
