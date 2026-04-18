# cmake/stage_overlay.cmake — runs at build time (via cmake -P) to copy
# Solaya's Rust userspace binaries into buildroot's overlay tree under
# /bin, where they end up at /bin/<name> in the final cpio.
#
# Driven from add_custom_target(buildroot-overlay) in cmake/buildroot.cmake.
#
# Usage: cmake -DSRC=<artifact_dir> -DDST=<overlay_dir> -P stage_overlay.cmake

if(NOT DEFINED SRC OR NOT DEFINED DST)
    message(FATAL_ERROR "stage_overlay.cmake: need -DSRC=<artifact_dir> -DDST=<overlay_dir>")
endif()

file(MAKE_DIRECTORY "${DST}")

# Copy each artifact.  The overlay goes into /bin regardless of what's in
# the artifact dir; buildroot applies this on top of its own /bin.  GNU
# coreutils lands in /usr/bin (single-binary-symlinks), so there's no
# name collision with our Rust daemons under /bin.
file(GLOB _entries LIST_DIRECTORIES false RELATIVE "${SRC}" "${SRC}/*")
foreach(_name IN LISTS _entries)
    set(_from "${SRC}/${_name}")
    set(_to "${DST}/${_name}")
    if(IS_SYMLINK "${_from}")
        file(READ_SYMLINK "${_from}" _target)
        file(REMOVE "${_to}")
        execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink "${_target}" "${_to}")
    else()
        # file(COPY) preserves the executable bit on POSIX; unlike
        # configure_file(COPYONLY) which resets permissions.
        file(COPY "${_from}" DESTINATION "${DST}" FILE_PERMISSIONS
            OWNER_READ OWNER_WRITE OWNER_EXECUTE
            GROUP_READ GROUP_EXECUTE
            WORLD_READ WORLD_EXECUTE)
    endif()
endforeach()

list(LENGTH _entries _count)
message(STATUS "stage_overlay: ${SRC} -> ${DST} (${_count} entries)")
