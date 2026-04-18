# buildroot_save_template.cmake — copy the just-saved minimal
# defconfig (materialized by `make savedefconfig`) back over the
# template .in file, reverting absolute paths to their @PLACEHOLDER@
# form so `configure_file(... @ONLY)` still works on the next
# configure.
#
# Invoked from cmake/buildroot.cmake via:
#   cmake -DMATERIALIZED=... -DTEMPLATE=... -DOVERLAY=... -DPOST_BUILD=...
#         -P cmake/buildroot_save_template.cmake

if(NOT MATERIALIZED OR NOT TEMPLATE OR NOT OVERLAY OR NOT POST_BUILD)
    message(FATAL_ERROR
        "buildroot_save_template.cmake: need MATERIALIZED, TEMPLATE, OVERLAY, POST_BUILD")
endif()

if(NOT EXISTS "${MATERIALIZED}")
    message(FATAL_ERROR
        "buildroot_save_template.cmake: ${MATERIALIZED} does not exist — "
        "did `make savedefconfig` fail?")
endif()

file(READ "${MATERIALIZED}" _content)

# Revert the two concrete paths to their template placeholders so the
# configure_file() call at the top of cmake/buildroot.cmake still
# substitutes them correctly.
string(REPLACE "\"${OVERLAY}\""    "\"@SOLAYA_BUILDROOT_OVERLAY_DIR@\""       _content "${_content}")
string(REPLACE "\"${POST_BUILD}\"" "\"@SOLAYA_BUILDROOT_POST_BUILD_SCRIPT@\"" _content "${_content}")

file(WRITE "${TEMPLATE}" "${_content}")

message(STATUS "buildroot_save_template: wrote ${TEMPLATE}")
message(STATUS
    "  Note: header comments from the previous .in file are NOT preserved — "
    "run `git diff` and restore any context you care about.")
