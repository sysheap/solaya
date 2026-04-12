# cmake/cargo_targets.cmake — helpers for invoking cargo from CMake.
#
# Used by the various subdir CMakeLists that build Rust crates. Bare-metal
# crates (boot, kernel) will grow their own helper in stage 6; for now this
# file only exposes shared lookup functions.

find_program(SOLAYA_CARGO cargo REQUIRED)

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
