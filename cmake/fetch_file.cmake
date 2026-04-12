# cmake/fetch_file.cmake — one-shot helper invoked via `cmake -P`.
#
# Downloads ${URL} to ${OUT} and verifies its SHA256 matches
# ${EXPECTED_SHA256}.  Short-circuits if the file already exists with the
# expected hash (idempotent, cheap to re-run).  Aborts with a clear error
# on hash mismatch.
#
# Called from custom_commands that need a hash-pinned file download at
# build time, where file(DOWNLOAD) at configure time would be wrong.

if(NOT DEFINED URL)
    message(FATAL_ERROR "fetch_file.cmake: URL not set")
endif()
if(NOT DEFINED OUT)
    message(FATAL_ERROR "fetch_file.cmake: OUT not set")
endif()
if(NOT DEFINED EXPECTED_SHA256)
    message(FATAL_ERROR "fetch_file.cmake: EXPECTED_SHA256 not set")
endif()

if(EXISTS "${OUT}")
    file(SHA256 "${OUT}" _existing_sha)
    if(_existing_sha STREQUAL EXPECTED_SHA256)
        return()
    endif()
endif()

message(STATUS "fetch_file: downloading ${URL} -> ${OUT}")
file(DOWNLOAD "${URL}" "${OUT}"
    EXPECTED_HASH "SHA256=${EXPECTED_SHA256}"
    SHOW_PROGRESS
    STATUS _dl_status
)
list(GET _dl_status 0 _dl_code)
list(GET _dl_status 1 _dl_msg)
if(NOT _dl_code EQUAL 0)
    file(REMOVE "${OUT}")
    message(FATAL_ERROR "fetch_file: download failed (${_dl_code}): ${_dl_msg}")
endif()
