# cmake/tools.cmake — non-kernel host tool targets.

# mcp-server — separate workspace, x86_64 host binary.
add_custom_target(mcp-server
    COMMAND ${SOLAYA_CARGO} build --release
            --manifest-path ${CMAKE_SOURCE_DIR}/mcp-server/Cargo.toml
            --target x86_64-unknown-linux-gnu
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Building MCP server"
)

# gdb-mcp-server — create project-local venv at ${CMAKE_SOURCE_DIR}/.venv
# and install Python deps from requirements.txt. Re-runs only when
# requirements.txt changes; a build-tree stamp file gates the work.
find_package(Python3 REQUIRED COMPONENTS Interpreter)

set(_gdb_mcp_venv_stamp "${CMAKE_BINARY_DIR}/gdb-mcp-server.stamp")
add_custom_command(
    OUTPUT "${_gdb_mcp_venv_stamp}"
    COMMAND ${Python3_EXECUTABLE} -m venv ${CMAKE_SOURCE_DIR}/.venv
    COMMAND ${CMAKE_SOURCE_DIR}/.venv/bin/pip install --disable-pip-version-check
            -r ${CMAKE_SOURCE_DIR}/gdb_mcp_server/requirements.txt
    COMMAND ${CMAKE_COMMAND} -E touch "${_gdb_mcp_venv_stamp}"
    DEPENDS "${CMAKE_SOURCE_DIR}/gdb_mcp_server/requirements.txt"
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Preparing gdb_mcp_server venv + deps"
    VERBATIM
)

add_custom_target(gdb-mcp-server DEPENDS "${_gdb_mcp_venv_stamp}")

# mcp-servers — umbrella: build both MCP servers in one shot.
add_custom_target(mcp-servers
    DEPENDS mcp-server gdb-mcp-server
    COMMENT "Building/preparing both MCP servers"
)

# fetch-deps — cargo fetch across every workspace, for offline/CI prewarm.
add_custom_target(fetch-deps
    COMMAND ${SOLAYA_CARGO} fetch
    COMMAND ${SOLAYA_CARGO} fetch --manifest-path ${CMAKE_SOURCE_DIR}/system-tests/Cargo.toml
    COMMAND ${SOLAYA_CARGO} fetch --manifest-path ${CMAKE_SOURCE_DIR}/mcp-server/Cargo.toml
    COMMAND ${SOLAYA_CARGO} fetch --manifest-path ${CMAKE_SOURCE_DIR}/tools/bindgen-driver/Cargo.toml
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Prefetching cargo deps for every workspace"
)

# Hardware-deploy targets delegate to small scripts in scripts/.
add_custom_target(tftp-deploy
    COMMAND ${CMAKE_SOURCE_DIR}/scripts/tftp-deploy.sh
            ${CMAKE_BINARY_DIR}/kernel/solaya.bin
    DEPENDS solaya-bin
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Deploying solaya.bin to the TFTP dir"
)

add_custom_target(reboot-hw
    COMMAND ${CMAKE_SOURCE_DIR}/scripts/reboot-hw.sh
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Sending reboot sequence over serial"
)

add_custom_target(picocom
    COMMAND ${CMAKE_SOURCE_DIR}/scripts/picocom.sh
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Opening picocom on /dev/ttyUSB0"
)
