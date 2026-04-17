# cmake/portable_find_program.cmake — make find_program() cache portable.
#
# CMake's stock find_program caches the absolute path returned by PATH
# lookup at configure time and bakes it into CMakeCache.txt + build.ninja.
# When the same build/ dir is shared between the host and the rootful
# podman devcontainer (which the project supports), the absolute paths
# diverge — e.g. host has /home/<user>/.nix-profile/bin/git, container
# has /usr/bin/git — and the next build on the other side fails with
# ENOENT or "Failed to get the hash for HEAD" out of ExternalProject's
# generated git scripts.
#
# We shadow the builtin so the cached value is the bare basename instead.
# CMake resolves bare names against $PATH at build time via execvp, which
# is what we want. The shadow chains via the documented underscore-prefix
# escape (CMake >= 3.18) so we can still call the original.
#
# Limitations:
#   - CMAKE_MAKE_PROGRAM is set by CMake during bootstrap, before any
#     CMakeLists.txt runs, so this override does not catch it. If it
#     ever diverges across environments, pass -DCMAKE_MAKE_PROGRAM=<name>
#     in the preset.
#   - find_package(Foo) modules that call find_program and then check
#     if(EXISTS ${X_EXECUTABLE}) will see the bare name and fail. None
#     of the current callers do this; if a future one does, fall back
#     to a per-tool helper (see cmake/llvm_tools.cmake for the pattern).

function(find_program out_var)
    _find_program(${out_var} ${ARGN})

    set(_val "${${out_var}}")
    if("${_val}" MATCHES "-NOTFOUND$" OR "${_val}" STREQUAL "")
        return()
    endif()

    if(IS_ABSOLUTE "${_val}")
        get_filename_component(_basename "${_val}" NAME)
        if(NOT "${_val}" STREQUAL "${_basename}")
            message(STATUS
                "find_program: caching ${out_var} as bare name "
                "'${_basename}' (was '${_val}') for host/container "
                "portability")
            set(${out_var} "${_basename}" CACHE FILEPATH
                "Path to a program (resolved at build time via PATH)."
                FORCE)
        endif()
    endif()
endfunction()
