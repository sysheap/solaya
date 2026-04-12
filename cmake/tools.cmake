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
