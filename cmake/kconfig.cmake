# cmake/kconfig.cmake — invoke mkconfig.py and expose Kconfig outputs.
#
# Behaviour:
#   * On first configure (no build/.config), seed .config from configs/${SOLAYA_DEFCONFIG}.
#   * Always (re)run mkconfig.py to regenerate kconfig.h (future C consumers).
#   * Re-run CMake configure when any Kconfig input or .config changes.
#   * Expose menuconfig / savedefconfig / olddefconfig custom targets.

find_program(PYTHON3_EXECUTABLE python3 REQUIRED)

set(SOLAYA_KCONFIG_ROOT      "${CMAKE_SOURCE_DIR}/Kconfig")
set(SOLAYA_KCONFIG_DOTCONFIG "${CMAKE_BINARY_DIR}/.config")
set(SOLAYA_KCONFIG_OUTPUT    "${CMAKE_BINARY_DIR}/kconfig")
set(SOLAYA_MKCONFIG          "${CMAKE_SOURCE_DIR}/scripts/mkconfig.py")
set(SOLAYA_KCONFIGLIB        "${CMAKE_SOURCE_DIR}/tools/kconfiglib")

if(NOT DEFINED SOLAYA_DEFCONFIG)
    set(SOLAYA_DEFCONFIG "riscv64_virt_defconfig" CACHE STRING
        "Filename (under configs/) used to seed build/.config on first configure."
    )
endif()

set(_defconfig_path "${CMAKE_SOURCE_DIR}/configs/${SOLAYA_DEFCONFIG}")
if(NOT EXISTS "${_defconfig_path}")
    message(FATAL_ERROR "defconfig not found: ${_defconfig_path}")
endif()

if(NOT EXISTS "${SOLAYA_KCONFIG_DOTCONFIG}")
    configure_file("${_defconfig_path}" "${SOLAYA_KCONFIG_DOTCONFIG}" COPYONLY)
    message(STATUS "Kconfig: seeded .config from configs/${SOLAYA_DEFCONFIG}")
endif()

execute_process(
    COMMAND ${PYTHON3_EXECUTABLE} "${SOLAYA_MKCONFIG}"
            --kconfig    "${SOLAYA_KCONFIG_ROOT}"
            --config     "${SOLAYA_KCONFIG_DOTCONFIG}"
            --out-dir    "${SOLAYA_KCONFIG_OUTPUT}"
            --source-dir "${CMAKE_SOURCE_DIR}"
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    RESULT_VARIABLE _mk_rc
    OUTPUT_VARIABLE _mk_out
    ERROR_VARIABLE  _mk_err
)
if(NOT _mk_rc EQUAL 0)
    message(FATAL_ERROR "mkconfig.py failed:\n${_mk_err}")
endif()

# Rerun CMake configure when inputs change.
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
    "${SOLAYA_KCONFIG_ROOT}"
    "${SOLAYA_KCONFIG_DOTCONFIG}"
    "${SOLAYA_MKCONFIG}"
)

# Interactive Kconfig targets. All inherit KCONFIG_CONFIG so build/.config is
# the file being edited, not a stray source-tree .config.
add_custom_target(menuconfig
    COMMAND ${CMAKE_COMMAND} -E env
            "KCONFIG_CONFIG=${SOLAYA_KCONFIG_DOTCONFIG}"
            ${PYTHON3_EXECUTABLE} "${SOLAYA_KCONFIGLIB}/menuconfig.py"
            "${SOLAYA_KCONFIG_ROOT}"
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    COMMENT "Launching menuconfig"
    VERBATIM
)

add_custom_target(savedefconfig
    COMMAND ${CMAKE_COMMAND} -E env
            "KCONFIG_CONFIG=${SOLAYA_KCONFIG_DOTCONFIG}"
            ${PYTHON3_EXECUTABLE} "${SOLAYA_KCONFIGLIB}/savedefconfig.py"
            --kconfig "${SOLAYA_KCONFIG_ROOT}"
            --out     "${CMAKE_BINARY_DIR}/savedefconfig"
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    COMMENT "Minimal defconfig written to ${CMAKE_BINARY_DIR}/savedefconfig"
    VERBATIM
)

add_custom_target(olddefconfig
    COMMAND ${CMAKE_COMMAND} -E env
            "KCONFIG_CONFIG=${SOLAYA_KCONFIG_DOTCONFIG}"
            ${PYTHON3_EXECUTABLE} "${SOLAYA_KCONFIGLIB}/olddefconfig.py"
            "${SOLAYA_KCONFIG_ROOT}"
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
    COMMENT "Updating .config with new defaults"
    VERBATIM
)
