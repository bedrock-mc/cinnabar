cmake_minimum_required(VERSION 3.24)
project(cinnabar_bds_runtime C ASM)

include(ExternalProject)
find_package(Python3 REQUIRED COMPONENTS Interpreter)
file(READ "${CMAKE_CURRENT_LIST_DIR}/../runtime.json" runtime)
string(JSON revision GET "${runtime}" bds_translation revision)
string(JSON archive_base_url GET "${runtime}" bds_translation archive_base_url)
string(JSON digest GET "${runtime}" bds_translation archive_sha256)
ExternalProject_Add(box64
    URL "${archive_base_url}${revision}"
    URL_HASH "SHA256=${digest}"
    DOWNLOAD_EXTRACT_TIMESTAMP TRUE
    SOURCE_DIR "${CMAKE_BINARY_DIR}/source"
    BINARY_DIR "${CMAKE_BINARY_DIR}/build"
    PATCH_COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_LIST_DIR}/patch.py" <SOURCE_DIR>
    CMAKE_ARGS
        -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=aarch64
        "-DCMAKE_C_COMPILER=${CMAKE_C_COMPILER}"
        "-DCMAKE_CXX_COMPILER=${CINNABAR_BDS_CXX}"
        "-DCMAKE_BUILD_TYPE=${CMAKE_BUILD_TYPE}"
        "-DCMAKE_PROJECT_box64_INCLUDE=${CMAKE_CURRENT_LIST_DIR}/compat.cmake"
        -DCMAKE_POSITION_INDEPENDENT_CODE=ON
        "-DCMAKE_EXE_LINKER_FLAGS=${CMAKE_EXE_LINKER_FLAGS}"
        -DARM64=ON -DARM_DYNAREC=ON -DNOLOADADDR=ON -DNO_LIB_INSTALL=ON -DNOGIT=ON
    BUILD_ALWAYS TRUE
    BUILD_COMMAND "${CMAKE_COMMAND}" --build <BINARY_DIR> --target box64 --parallel "${CINNABAR_BUILD_JOBS}"
    INSTALL_COMMAND ""
    BUILD_BYPRODUCTS "${CMAKE_BINARY_DIR}/build/box64")

ExternalProject_Add_StepDependencies(box64 patch "${CMAKE_CURRENT_LIST_DIR}/patch.py")
ExternalProject_Add_Step(box64 verify_guest
    COMMAND "${CMAKE_COMMAND}" "-DMANIFEST=${CMAKE_CURRENT_LIST_DIR}/../runtime.json"
        "-DSOURCE_DIR=<SOURCE_DIR>" -P "${CMAKE_CURRENT_LIST_DIR}/verify-guest.cmake"
    DEPENDEES patch DEPENDERS configure ALWAYS TRUE)
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_LIST_DIR}/../runtime.json")

set(CINNABAR_BDS_NATIVE_OVERRIDE "" CACHE PATH "Replacement native runtime library directory")
if(NOT CINNABAR_BDS_NATIVE_OVERRIDE)
    string(JSON glibc_url GET "${runtime}" bds_translation native_runtime url)
    string(JSON glibc_digest GET "${runtime}" bds_translation native_runtime sha256)
    ExternalProject_Add(glibc
        URL "${glibc_url}" URL_HASH "SHA256=${glibc_digest}"
        DOWNLOAD_EXTRACT_TIMESTAMP TRUE
        SOURCE_DIR "${CMAKE_BINARY_DIR}/glibc"
        CONFIGURE_COMMAND "" BUILD_COMMAND "" INSTALL_COMMAND "")
    add_dependencies(box64 glibc)
endif()
