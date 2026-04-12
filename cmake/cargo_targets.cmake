# cmake/cargo_targets.cmake — helpers for invoking cargo from CMake.
#
# Used by the various subdir CMakeLists that build Rust crates. Bare-metal
# crates (boot, kernel) will grow their own helper in stage 6; for now this
# file only exposes shared lookup functions.

# Resolve cargo at build time, not configure time. When the same build/ dir
# is shared between a podman devcontainer and the host, baking an absolute
# cargo path (e.g. /root/.cargo/bin/cargo) into build.ninja breaks the
# "other" environment with exit 126 (Permission denied). Emit the bare
# command name so /bin/sh resolves it via $PATH on whichever side runs ninja.
execute_process(
    COMMAND cargo --version
    RESULT_VARIABLE _solaya_cargo_rc
    OUTPUT_QUIET ERROR_QUIET
)
if(NOT _solaya_cargo_rc EQUAL 0)
    message(FATAL_ERROR "cargo not found on PATH — install the Rust toolchain")
endif()

# Drop any absolute path cached by earlier versions of this file so
# CMakeCache.txt doesn't retain dead state across reconfigures.
unset(SOLAYA_CARGO CACHE)
set(SOLAYA_CARGO cargo)

# solaya_read_kconfig_features(out_var crate_name)
#
# Reads build/kconfig/cargo-features.txt (produced by scripts/mkconfig.py)
# and returns the comma-separated feature list for the given crate in
# out_var, or "" if the crate has no features enabled.
function(solaya_read_kconfig_features out_var crate_name)
    set(_path "${CMAKE_BINARY_DIR}/kconfig/cargo-features.txt")
    if(NOT EXISTS "${_path}")
        set(${out_var} "" PARENT_SCOPE)
        return()
    endif()
    file(STRINGS "${_path}" _lines)
    set(_result "")
    foreach(_line IN LISTS _lines)
        if(_line MATCHES "^${crate_name}:(.*)$")
            set(_result "${CMAKE_MATCH_1}")
            break()
        endif()
    endforeach()
    set(${out_var} "${_result}" PARENT_SCOPE)
endfunction()
