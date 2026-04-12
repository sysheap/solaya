# cmake/bridge_sysroot_lib.cmake — symlink <sysroot>/usr/lib/* into
# <sysroot>/lib/.  Invoked via `cmake -P` from toolchain_bootstrap's musl
# install step: gcc-stage2's libgcc build looks up libc.a/crt*.o under
# <sysroot>/lib, but musl installs them into <sysroot>/usr/lib.  Binutils
# already populated <sysroot>/lib with ldscripts/ so we can't make lib a
# symlink to usr/lib — instead we link each file individually.
#
# Inputs (passed with -D on the cmake -P command line):
#   SOLAYA_SYSROOT_LIB   absolute path to <sysroot>/lib

if(NOT DEFINED SOLAYA_SYSROOT_LIB)
    message(FATAL_ERROR "bridge_sysroot_lib.cmake: SOLAYA_SYSROOT_LIB not set")
endif()

file(GLOB _files "${SOLAYA_SYSROOT_LIB}/../usr/lib/*")
foreach(_src IN LISTS _files)
    get_filename_component(_name "${_src}" NAME)
    set(_dst "${SOLAYA_SYSROOT_LIB}/${_name}")
    # Prefer relative target so the sysroot stays relocatable.
    file(RELATIVE_PATH _rel "${SOLAYA_SYSROOT_LIB}" "${_src}")
    file(CREATE_LINK "${_rel}" "${_dst}" SYMBOLIC)
endforeach()
