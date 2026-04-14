# cmake/doom.cmake — build the Solaya port of doomgeneric.
#
# Replaces the nix flake's doom-riscv derivation.  Cross-compiles
# github.com/ozkl/doomgeneric with distro clang (--target=riscv64-linux-musl)
# via the cmake/clang_wrapper.cmake shims, embeds doom1.wad as a binary
# object using `ld.lld -r -b binary`, and stages the resulting static ELF
# as ${SOLAYA_USERSPACE_ARTIFACT_DIR}/doom.
#
# doomgeneric provides the renderer abstraction; Solaya supplies two custom
# translation units (userspace/doom/{dg_solaya.c,i_video_solaya.c}) that
# bind doomgeneric's video/input hooks to Solaya's framebuffer + keyboard
# syscalls.  The list of source files below mirrors exactly what the nix
# derivation compiled, so if doomgeneric's source tree grows (or shrinks)
# the pin has to be bumped + this list updated together.
#
# The doom_starts system test depends on the staged binary, so without this
# target `cmake --build build --target test-system` fails at doom start.

include(ExternalProject)
include(${CMAKE_SOURCE_DIR}/cmake/checksums.cmake)

if(NOT DEFINED SOLAYA_CROSS_BIN)
    message(FATAL_ERROR
        "cmake/doom.cmake: SOLAYA_CROSS_BIN not defined. "
        "cmake/clang_wrapper.cmake must run before include(doom)."
    )
endif()

set(_doom_prefix   "${CMAKE_BINARY_DIR}/userspace/doom-prefix")
set(_doom_src      "${_doom_prefix}/src/doom-src")
set(_doom_build    "${_doom_prefix}/build")
set(_doom_bin      "${_doom_build}/doom")
set(_doom_cc       "${SOLAYA_CROSS_BIN}/riscv64-linux-musl-clang")
set(_wad_file      "${SOLAYA_TC_ROOT}/_dl/doom1.wad")

# Stage 1: fetch doomgeneric source (pinned commit) into _doom_src.
ExternalProject_Add(doom-src
    GIT_REPOSITORY    "${SOLAYA_DOOMGENERIC_REPO}"
    GIT_TAG           "${SOLAYA_DOOMGENERIC_REV}"
    SOURCE_DIR        "${_doom_src}"
    USES_TERMINAL_DOWNLOAD ON
    CONFIGURE_COMMAND ""
    BUILD_COMMAND     ""
    INSTALL_COMMAND   ""
)

# Stage 2: fetch doom1.wad.  Use a custom_command (not an ExternalProject)
# because we only need a file download with hash verification, and we want
# the file placed in the predictable _dl dir used by other tarball fetches.
#
# CMake's file(DOWNLOAD) supports EXPECTED_HASH, but runs at configure time
# and produces no build-graph node.  A custom_command here lets the download
# happen at build time, which keeps `cmake --preset` fast on fresh clones.
add_custom_command(
    OUTPUT  "${_wad_file}"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SOLAYA_TC_ROOT}/_dl"
    COMMAND ${CMAKE_COMMAND}
        -D "URL=${SOLAYA_DOOM_WAD_URL}"
        -D "OUT=${_wad_file}"
        -D "EXPECTED_SHA256=${SOLAYA_DOOM_WAD_SHA256}"
        -P "${CMAKE_SOURCE_DIR}/cmake/fetch_file.cmake"
    COMMENT "Fetching doom1.wad"
    VERBATIM
)
add_custom_target(doom-wad DEPENDS "${_wad_file}")

# Stage 3: compile doomgeneric + Solaya adapter files against the
# cross-toolchain.  This script keeps parity with the nix buildPhase.
set(_doom_srcs
    dummy.c am_map.c doomdef.c doomstat.c dstrings.c
    d_event.c d_items.c d_iwad.c d_loop.c d_main.c d_mode.c d_net.c
    f_finale.c f_wipe.c g_game.c hu_lib.c hu_stuff.c info.c
    i_cdmus.c i_endoom.c i_joystick.c i_scale.c i_sound.c i_system.c
    i_timer.c memio.c m_argv.c m_bbox.c m_cheat.c m_config.c
    m_controls.c m_fixed.c m_menu.c m_misc.c m_random.c
    p_ceilng.c p_doors.c p_enemy.c p_floor.c p_inter.c p_lights.c
    p_map.c p_maputl.c p_mobj.c p_plats.c p_pspr.c p_saveg.c
    p_setup.c p_sight.c p_spec.c p_switch.c p_telept.c p_tick.c
    p_user.c r_bsp.c r_data.c r_draw.c r_main.c r_plane.c r_segs.c
    r_sky.c r_things.c sha1.c sounds.c statdump.c st_lib.c st_stuff.c
    s_sound.c tables.c v_video.c wi_stuff.c w_checksum.c w_file.c
    w_main.c w_wad.c z_zone.c w_file_stdc.c i_input.c i_video.c
    mus2mid.c doomgeneric.c dg_solaya.c
)

set(_solaya_doom_adapter "${CMAKE_SOURCE_DIR}/userspace/doom")
set(_doom_build_script   "${_doom_build}/build.sh")

# Generate the build script at configure time; it runs at build time.
# Keeping the commands in a shell script (rather than a long chain of
# COMMAND ... COMMAND ...) avoids hitting CMake's COMMAND argument limits
# on platforms with small argv caps.
file(WRITE "${_doom_build_script}.in"
"#!/usr/bin/env bash
set -euo pipefail
SRC_DIR=\"$<1:${_doom_src}>/doomgeneric\"
ADAPTER=\"${_solaya_doom_adapter}\"
WAD=\"${_wad_file}\"
OUT_DIR=\"${_doom_build}\"
CC=\"${_doom_cc}\"
CFLAGS=\"-static -O3 -DNORMALUNIX -DLINUX -D_DEFAULT_SOURCE -I.\"

rm -rf \"$OUT_DIR/obj\" \"$OUT_DIR/wad_obj\"
mkdir -p \"$OUT_DIR/obj\" \"$OUT_DIR/wad_obj\"
cd \"$OUT_DIR/obj\"

# Copy sources + Solaya's adapter translation units into a flat working
# dir (the nix build does the same; it keeps the per-file -c invocations
# tractable and lets i_video_solaya.c override the doomgeneric i_video.c).
for f in $SRC_DIR/*.{c,h}; do cp \"$f\" .; done
cp \"$ADAPTER/dg_solaya.c\"      ./dg_solaya.c
cp \"$ADAPTER/i_video_solaya.c\" ./i_video.c
cp \"$WAD\" ./doom1.wad
# Embed the WAD as a relocatable object.  `llvm-objcopy -I binary` produces
# an ELF with no RISC-V FP-ABI property, which lld refuses to mix with
# lp64d objects; `ld.lld -r -b binary` has the same gap.  Assembling a tiny
# .S file with .incbin lets the assembler stamp the matching ABI flags.
cat > doom1_wad.S <<WADEOF
.section .rodata
.globl _binary_doom1_wad_start
.globl _binary_doom1_wad_end
.globl _binary_doom1_wad_size
.type  _binary_doom1_wad_start, @object
.type  _binary_doom1_wad_end,   @object
_binary_doom1_wad_start:
.incbin \"doom1.wad\"
_binary_doom1_wad_end:
.set _binary_doom1_wad_size, _binary_doom1_wad_end - _binary_doom1_wad_start
WADEOF
$CC -c doom1_wad.S -o ../wad_obj/doom1_wad.o

")

# Append the per-file compile loop.
set(_compile_loop "")
foreach(_f IN LISTS _doom_srcs)
    string(REPLACE ".c" ".o" _o "${_f}")
    set(_compile_loop "${_compile_loop}$CC $CFLAGS -c ${_f} -o ${_o}\n")
endforeach()
file(APPEND "${_doom_build_script}.in" "${_compile_loop}")

# Final link.
file(APPEND "${_doom_build_script}.in"
"$CC $CFLAGS -o \"$OUT_DIR/doom\" *.o ../wad_obj/doom1_wad.o -lm
")

# file(GENERATE) expands generator expressions (<1:...>) and emits the
# final script once per build configuration.
file(GENERATE OUTPUT "${_doom_build_script}"
    INPUT "${_doom_build_script}.in"
)

add_custom_command(
    OUTPUT  "${_doom_bin}"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${_doom_build}"
    COMMAND bash "${_doom_build_script}"
    DEPENDS doom-src doom-wad musl compiler-rt-builtins
            "${_solaya_doom_adapter}/dg_solaya.c"
            "${_solaya_doom_adapter}/i_video_solaya.c"
            "${_doom_build_script}"
    COMMENT "Compiling doomgeneric (riscv64-musl static)"
    VERBATIM
)

# Stage 4: copy the binary into the userspace artifact dir.
add_custom_command(
    OUTPUT  "${SOLAYA_USERSPACE_ARTIFACT_DIR}/doom"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SOLAYA_USERSPACE_ARTIFACT_DIR}"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${_doom_bin}" "${SOLAYA_USERSPACE_ARTIFACT_DIR}/doom"
    DEPENDS "${_doom_bin}"
    COMMENT "Staging doom into userspace artifact dir"
    VERBATIM
)

add_custom_target(doom ALL
    DEPENDS "${SOLAYA_USERSPACE_ARTIFACT_DIR}/doom"
)
