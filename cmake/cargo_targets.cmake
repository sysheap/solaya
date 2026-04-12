# cmake/cargo_targets.cmake — helpers for invoking cargo from CMake.
#
# Used by the various subdir CMakeLists that build Rust crates. Bare-metal
# crates (boot, kernel) will grow their own helper in stage 6; for now this
# file only exposes shared lookup functions.

# Validate a previously cached SOLAYA_CARGO before reusing it. When the build
# dir is shared between host and a podman/devcontainer (e.g. configure ran in
# the container and stored `/root/.cargo/bin/cargo`, which the host user can't
# execute), the cached path must be re-detected instead of silently failing
# every cargo-driven target with exit 126.
if(SOLAYA_CARGO)
    execute_process(
        COMMAND "${SOLAYA_CARGO}" --version
        RESULT_VARIABLE _solaya_cargo_check
        OUTPUT_QUIET ERROR_QUIET
    )
    if(NOT _solaya_cargo_check EQUAL 0)
        message(STATUS
            "Cached SOLAYA_CARGO=${SOLAYA_CARGO} is not executable in this "
            "environment; re-detecting.")
        unset(SOLAYA_CARGO CACHE)
    endif()
endif()

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
