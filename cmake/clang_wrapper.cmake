# cmake/clang_wrapper.cmake — emit `riscv64-linux-musl-*` wrapper scripts
# under the build dir that consumers (cargo linker, dash/doom configure, the
# kernel ELF post-processing step) can invoke by a single name.
#
# Each wrapper is a tiny POSIX shell script that `exec`s the corresponding
# distro LLVM tool discovered by cmake/llvm_tools.cmake, preloading the
# riscv64-linux-musl target + sysroot + lld linker-flavor flags for the
# compiler drivers.  The bare tool name (not absolute path) is embedded in
# the wrapper so /bin/sh resolves it via $PATH at run time — this keeps
# build/ dirs shared between host and podman container working, same as the
# treatment of `cargo` in commit ed010412.
#
# Outputs:
#   ${CMAKE_BINARY_DIR}/toolchain/bin/riscv64-linux-musl-{clang,clang++,
#       ld,ar,nm,ranlib,objcopy,objdump,strip,readelf,addr2line}
#
# Downstream consumers reference these paths via ${SOLAYA_CROSS_BIN}, which
# is defined here and exported to parent scope.
#
# Compiler-rt handling.  Distro packages don't ship libclang_rt.builtins for
# cross targets, so toolchain_bootstrap.cmake builds one against the riscv64
# sysroot and stages it under ${SOLAYA_COMPILER_RT_DIR}.  The clang wrapper
# passes `-rtlib=compiler-rt -unwindlib=none -resource-dir=${RT_DIR}` so
# linker invocations find it.  The `include/` subdir is symlinked to the
# host clang's real resource-dir include so built-in headers (stdalign.h,
# stdatomic.h, intrinsics) remain available.

if(NOT DEFINED SOLAYA_TC_PREFIX)
    message(FATAL_ERROR
        "cmake/clang_wrapper.cmake: SOLAYA_TC_PREFIX not defined. "
        "cmake/arch.cmake must run first."
    )
endif()
if(NOT DEFINED SOLAYA_TC_TRIPLE)
    message(FATAL_ERROR
        "cmake/clang_wrapper.cmake: SOLAYA_TC_TRIPLE not defined."
    )
endif()
if(NOT DEFINED SOLAYA_CLANG)
    message(FATAL_ERROR
        "cmake/clang_wrapper.cmake: SOLAYA_CLANG not defined. "
        "cmake/llvm_tools.cmake must run first."
    )
endif()

set(SOLAYA_CROSS_BIN        "${CMAKE_BINARY_DIR}/toolchain/bin"              CACHE INTERNAL "")
set(SOLAYA_CROSS_TRIPLE     "riscv64-linux-musl"                             CACHE INTERNAL "")
set(SOLAYA_CROSS_SYSROOT    "${SOLAYA_TC_PREFIX}/${SOLAYA_TC_TRIPLE}"        CACHE INTERNAL "")
# Compiler-rt lives alongside the musl sysroot in ${SOLAYA_TC_PREFIX} (which
# is ${SOLAYA_TC_ROOT}/<arch>, outside build/) so `rm -rf build` doesn't
# force a rebuild of the ~30s compiler-rt stage.
set(SOLAYA_COMPILER_RT_DIR  "${SOLAYA_TC_PREFIX}/rt"                         CACHE INTERNAL "")

file(MAKE_DIRECTORY "${SOLAYA_CROSS_BIN}")
file(MAKE_DIRECTORY "${SOLAYA_COMPILER_RT_DIR}")
file(MAKE_DIRECTORY "${SOLAYA_COMPILER_RT_DIR}/lib")

# Symlink include/ so clang's built-in headers (stdalign.h, stdatomic.h,
# intrinsics) keep working even though we override resource-dir.  Re-running
# configure refreshes the link if the distro's clang resource-dir changes.
execute_process(
    COMMAND ${SOLAYA_CLANG} -print-resource-dir
    OUTPUT_VARIABLE _real_rd
    OUTPUT_STRIP_TRAILING_WHITESPACE
    RESULT_VARIABLE _rc
)
if(NOT _rc EQUAL 0)
    message(FATAL_ERROR "clang -print-resource-dir failed (rc=${_rc})")
endif()
file(REMOVE "${SOLAYA_COMPILER_RT_DIR}/include")
file(CREATE_LINK
    "${_real_rd}/include"
    "${SOLAYA_COMPILER_RT_DIR}/include"
    SYMBOLIC
)

# Compiler prelude.  --target + --sysroot pin the cross environment;
# -rtlib=compiler-rt + -unwindlib=none + -resource-dir redirect link-time
# runtime-library lookup to our bootstrapped compiler-rt builtins (built by
# toolchain-all into ${SOLAYA_COMPILER_RT_DIR}).  -fuse-ld=lld is redundant
# with -fuse-ld from the wrapper flags but kept explicit for clarity.
#
# These flags are emitted unconditionally, including on compile-only
# invocations; clang warns `-rtlib`/`-unwindlib` are unused during
# compilation but still produces the object.  The warning is silenced per
# consumer that cares (e.g. dash's CFLAGS).
set(_clang_prelude
    "--target=${SOLAYA_CROSS_TRIPLE} --sysroot=${SOLAYA_CROSS_SYSROOT} -fuse-ld=lld -rtlib=compiler-rt -unwindlib=none -resource-dir=${SOLAYA_COMPILER_RT_DIR}")

function(_solaya_write_wrapper name body)
    set(_path "${SOLAYA_CROSS_BIN}/${name}")
    file(WRITE  "${_path}" "#!/bin/sh\n${body}\n")
    file(CHMOD  "${_path}" PERMISSIONS
        OWNER_READ OWNER_WRITE OWNER_EXECUTE
        GROUP_READ GROUP_EXECUTE
        WORLD_READ WORLD_EXECUTE)
endfunction()

_solaya_write_wrapper("riscv64-linux-musl-clang"
    "exec ${SOLAYA_CLANG} ${_clang_prelude} \"$@\"")
_solaya_write_wrapper("riscv64-linux-musl-clang++"
    "exec ${SOLAYA_CLANGXX} ${_clang_prelude} \"$@\"")

# Direct lld invocation — used by cmake/doom.cmake for `ld -r -b binary` to
# package the WAD as an object file.  -m elf64lriscv tells lld which ELF
# machine type to emit for the relocatable output.
_solaya_write_wrapper("riscv64-linux-musl-ld"
    "exec ${SOLAYA_LLD} -m elf64lriscv \"$@\"")

# Plain LLVM binutils pass through unchanged — their `-compatible-with-GNU`
# CLI means existing flags (--demangle, --dump-section, -O binary, ...) work
# as-is.
foreach(_tool_pair
    "ar;SOLAYA_LLVM_AR"
    "nm;SOLAYA_LLVM_NM"
    "ranlib;SOLAYA_LLVM_RANLIB"
    "objcopy;SOLAYA_LLVM_OBJCOPY"
    "objdump;SOLAYA_LLVM_OBJDUMP"
    "strip;SOLAYA_LLVM_STRIP"
    "readelf;SOLAYA_LLVM_READELF"
    "addr2line;SOLAYA_LLVM_ADDR2LINE"
)
    list(GET _tool_pair 0 _short)
    list(GET _tool_pair 1 _var)
    _solaya_write_wrapper("riscv64-linux-musl-${_short}"
        "exec ${${_var}} \"$@\"")
endforeach()
