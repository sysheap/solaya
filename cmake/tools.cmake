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
# and install Python deps from requirements.txt. Idempotent: re-running is
# a fast no-op when the venv exists and deps are up-to-date.
find_package(Python3 REQUIRED COMPONENTS Interpreter)

add_custom_target(gdb-mcp-server
    COMMAND ${Python3_EXECUTABLE} -m venv ${CMAKE_SOURCE_DIR}/.venv
    COMMAND ${CMAKE_SOURCE_DIR}/.venv/bin/pip install --disable-pip-version-check
            -r ${CMAKE_SOURCE_DIR}/gdb_mcp_server/requirements.txt
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    USES_TERMINAL
    VERBATIM
    COMMENT "Preparing gdb_mcp_server venv + deps"
)

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

# index — regenerate INDEX.md via indxr. Wired as a dependency of solaya-bin
# below so every `make build` keeps the AI-agent codebase index fresh.
#
# The script resolves `indxr` at *build* time (via the shell's PATH lookup)
# instead of having CMake cache an absolute path at configure time — the
# cached path would be unusable when configure and build run as different
# users (e.g. configure as root bakes /root/.cargo/bin/indxr into the
# cache, and a normal user sharing the build dir then hits Permission
# denied). The script also soft-fails when indxr is not installed.
add_custom_target(index
    COMMAND ${CMAKE_SOURCE_DIR}/scripts/regenerate-index.sh
            ${CMAKE_SOURCE_DIR}/INDEX.md
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    VERBATIM
    COMMENT "Regenerating INDEX.md"
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
