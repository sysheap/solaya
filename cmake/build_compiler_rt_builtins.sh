#!/usr/bin/env bash
# Build libclang_rt.builtins.a + clang_rt.crtbegin.o + clang_rt.crtend.o
# from compiler-rt source, targeting riscv64-linux-musl via the clang
# wrapper defined by cmake/clang_wrapper.cmake.
#
# We avoid compiler-rt's own CMake entrypoint because it requires the full
# llvm-project monorepo (cmake/ shared modules) to resolve transitive
# includes like ExtendPath / GetClangResourceDir.  Globbing the file list
# directly is simpler and about 30× faster.
#
# Exit codes: 0 success, !=0 any step failed.
#
# Args:
#   $1  path to extracted compiler-rt source tree
#   $2  riscv64-linux-musl-clang wrapper
#   $3  riscv64-linux-musl-ar wrapper
#   $4  output dir (resource-dir/lib/<triple>/)
set -euo pipefail

if [ $# -ne 4 ]; then
    echo "usage: $0 <compiler-rt-src> <clang> <ar> <outdir>" >&2
    exit 64
fi

SRC="$1/lib/builtins"
CC="$2"
AR="$3"
OUT="$4"

if [ ! -d "$SRC" ]; then
    echo "error: compiler-rt source not found: $SRC" >&2
    exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$OUT"

# Files to skip:
#   - host-specific runtime glue we don't use (gcc_personality_v0 pulls in
#     libunwind, emutls / trampoline_setup need dynamic loader hooks,
#     atomic.c needs <atomic.h>, clear_cache uses BSD syscalls).
#   - `*xf*` / `*xc*` — these implement 80-bit extended-precision float
#     helpers that only x86 long-double uses; they reference x86-only
#     `xf_float` / `xc_float` typedefs and don't compile on riscv64.
#   - crtbegin / crtend — compiled separately below, not part of the .a.
SKIP_RE='^(gcc_personality_v0|emutls|enable_execute_stack|eprintf|apple_versioning|clear_cache|trampoline_setup|atomic|cpu_model|crtbegin|crtend|.*xf.*|.*xc.*)$'

# Common flags for every translation unit.  -fPIC lets the resulting .a be
# linked into both -static and PIE final binaries.  -DVISIBILITY_HIDDEN
# matches compiler-rt's upstream build (keeps internal helpers out of the
# final executable's dynamic symbol table where applicable).
CFLAGS="-fPIC -O2 -DVISIBILITY_HIDDEN -Wno-unused-command-line-argument"

compile_one() {
    # $1 = src file (relative to $SRC), $2 = output .o path.  Uses `||` so
    # a single file's failure doesn't take down all the background jobs;
    # we audit the final object count against the source count afterwards.
    "$CC" $CFLAGS -c "$SRC/$1" -o "$2" || {
        echo "FAIL: $1" >&2
        return 1
    }
}

FAILED_LIST="$WORK/.failed"
: > "$FAILED_LIST"

echo "compiler-rt-builtins: compiling generic builtins in $SRC"
compiled=0
for f in "$SRC"/*.c; do
    base="$(basename "${f%.c}")"
    if [[ "$base" =~ $SKIP_RE ]]; then
        continue
    fi
    compiled=$((compiled + 1))
    ( compile_one "${f#$SRC/}" "$WORK/${base}.o" || echo "$base" >> "$FAILED_LIST" ) &
done

echo "compiler-rt-builtins: compiling riscv builtins"
for f in "$SRC/riscv"/*.S "$SRC/riscv"/*.c; do
    [ -e "$f" ] || continue
    base="$(basename "${f%.*}")"
    compiled=$((compiled + 1))
    ( compile_one "riscv/$(basename "$f")" "$WORK/riscv_${base}.o" || echo "riscv/$base" >> "$FAILED_LIST" ) &
done

wait

if [ -s "$FAILED_LIST" ]; then
    echo "error: $(wc -l < "$FAILED_LIST") compiler-rt builtins failed to build:" >&2
    cat "$FAILED_LIST" >&2
    exit 1
fi

echo "compiler-rt-builtins: compiling crtbegin / crtend"
compile_one "crtbegin.c" "$OUT/clang_rt.crtbegin.o"
compile_one "crtend.c"   "$OUT/clang_rt.crtend.o"

objcount=$(ls "$WORK" | grep -v '^\.' | wc -l)
echo "compiler-rt-builtins: archiving $objcount objects (of $compiled attempted) into libclang_rt.builtins.a"
rm -f "$OUT/libclang_rt.builtins.a"
"$AR" rcs "$OUT/libclang_rt.builtins.a" "$WORK"/*.o

echo "compiler-rt-builtins: done ($OUT)"
