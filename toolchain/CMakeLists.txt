# toolchain/CMakeLists.txt — entry point for the cross-toolchain bootstrap.
#
# Wired in from the top-level CMakeLists.txt via `add_subdirectory(toolchain)`.
# SOLAYA_ARCH, SOLAYA_TC_TRIPLE, SOLAYA_TC_PREFIX are populated by
# cmake/arch.cmake at the parent scope and inherited here; all bootstrap
# logic lives in cmake/toolchain_bootstrap.cmake.

cmake_minimum_required(VERSION 3.25)
project(solaya_toolchain LANGUAGES NONE)

if(NOT DEFINED SOLAYA_ARCH)
    message(FATAL_ERROR
        "toolchain/CMakeLists.txt: SOLAYA_ARCH not defined. "
        "cmake/arch.cmake must run before add_subdirectory(toolchain)."
    )
endif()

message(STATUS "Toolchain: bootstrapping for SOLAYA_ARCH=${SOLAYA_ARCH}")

include(${CMAKE_SOURCE_DIR}/cmake/toolchain_bootstrap.cmake)
