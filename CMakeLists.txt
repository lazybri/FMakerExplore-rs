cmake_minimum_required(VERSION 3.21)

project(FMakerExploreGui
    VERSION 0.1.0
    DESCRIPTION "Qt 6 file manager for the FMakerExplore Rust core"
    LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()

find_package(Qt6 6.4 REQUIRED COMPONENTS Widgets Concurrent)
qt_standard_project_setup()

option(FM_STATIC_QT "Link against a static Qt build" OFF)
option(FM_LIBUSB_UDEV "Allow the bundled libusb to use libudev" ON)
option(FM_STATIC_RUNTIME "Statically link the C++ runtime when supported" OFF)
set(FM_RUST_LIB "" CACHE FILEPATH "Prebuilt Rust static library (skips the Cargo build)")
set(FM_RUST_TARGET_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target"
    CACHE PATH "Cargo target directory used to locate the Rust static library")

get_filename_component(FM_RUST_TARGET_DIR "${FM_RUST_TARGET_DIR}" ABSOLUTE
    BASE_DIR "${CMAKE_CURRENT_SOURCE_DIR}")
set(FM_RUST_TARGET_DIR "${FM_RUST_TARGET_DIR}" CACHE PATH
    "Cargo target directory used to locate the Rust static library" FORCE)

if(MSVC)
    set(FM_RUST_STATIC_NAME "fmaker_explore.lib")
else()
    set(FM_RUST_STATIC_NAME "libfmaker_explore.a")
endif()

if(CMAKE_CONFIGURATION_TYPES AND NOT FM_RUST_LIB)
    message(FATAL_ERROR
        "Cargo integration requires a single-config generator (Ninja or Unix Makefiles). "
        "For multi-config generators, build the Rust core first and pass "
        "-DFM_RUST_LIB=<path to the static library>.")
endif()

if(CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(FM_CARGO_PROFILE_DIR "debug")
    set(FM_CARGO_PROFILE_ARG "--profile=dev")
else()
    set(FM_CARGO_PROFILE_DIR "release")
    set(FM_CARGO_PROFILE_ARG "--release")
endif()

if(FM_RUST_LIB)
    if(NOT EXISTS "${FM_RUST_LIB}")
        message(FATAL_ERROR "FM_RUST_LIB does not exist: ${FM_RUST_LIB}")
    endif()
    set(FM_RUST_LINK_PATH "${FM_RUST_LIB}")
    message(STATUS "Using prebuilt Rust library: ${FM_RUST_LINK_PATH}")
else()
    find_program(CARGO_EXECUTABLE cargo REQUIRED)
    set(FM_RUST_LINK_PATH
        "${FM_RUST_TARGET_DIR}/${FM_CARGO_PROFILE_DIR}/${FM_RUST_STATIC_NAME}")

    file(GLOB_RECURSE FM_RUST_SOURCES CONFIGURE_DEPENDS
        "${CMAKE_CURRENT_SOURCE_DIR}/src/*.rs"
        "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
        "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.lock")

    set(FM_CARGO_ENV "CARGO_TARGET_DIR=${FM_RUST_TARGET_DIR}")
    if(NOT FM_LIBUSB_UDEV)
        list(APPEND FM_CARGO_ENV "PKG_CONFIG_LIBDIR=/nonexistent")
    endif()

    add_custom_command(
        OUTPUT "${FM_RUST_LINK_PATH}"
        COMMAND ${CMAKE_COMMAND} -E env ${FM_CARGO_ENV}
                "${CARGO_EXECUTABLE}" build ${FM_CARGO_PROFILE_ARG}
        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
        DEPENDS ${FM_RUST_SOURCES}
        COMMENT "Building Rust core with Cargo (${FM_CARGO_PROFILE_DIR})"
        USES_TERMINAL
        VERBATIM)

    add_custom_target(fm_rust_build ALL DEPENDS "${FM_RUST_LINK_PATH}")
    message(STATUS "Rust core: ${FM_RUST_LINK_PATH}")
endif()

qt_add_executable(fmaker-explore-gui WIN32 MACOSX_BUNDLE
    gui/fm_explore.h
    gui/src/main.cpp
    gui/src/mainwindow.cpp
    gui/src/mainwindow.h
    gui/src/rustclient.cpp
    gui/src/rustclient.h
    gui/src/theme.cpp
    gui/src/theme.h
    gui/src/util.cpp
    gui/src/util.h)

if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    target_sources(fmaker-explore-gui PRIVATE
        gui/src/linuxlauncher.cpp
        gui/src/linuxlauncher.h)
endif()

target_include_directories(fmaker-explore-gui PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/gui")
target_compile_definitions(fmaker-explore-gui PRIVATE
    FM_VERSION_STRING="${PROJECT_VERSION}")

if(MSVC)
    target_compile_options(fmaker-explore-gui PRIVATE /W4 /permissive-)
else()
    target_compile_options(fmaker-explore-gui PRIVATE -Wall -Wextra -Wpedantic)
endif()

target_link_libraries(fmaker-explore-gui PRIVATE
    Qt6::Widgets
    Qt6::Concurrent
    "${FM_RUST_LINK_PATH}")

if(NOT FM_RUST_LIB)
    add_dependencies(fmaker-explore-gui fm_rust_build)
endif()

if(APPLE)
    target_link_libraries(fmaker-explore-gui PRIVATE
        "-framework CoreFoundation"
        "-framework IOKit"
        "-framework Security"
        objc
        iconv)
elseif(WIN32)
    target_link_libraries(fmaker-explore-gui PRIVATE
        setupapi
        user32
        advapi32
        ws2_32
        userenv
        bcrypt
        ntdll
        ole32
        oleaut32
        uuid
        shell32
        cfgmgr32)
else()
    find_package(Threads REQUIRED)
    target_link_libraries(fmaker-explore-gui PRIVATE
        Threads::Threads
        ${CMAKE_DL_LIBS}
        m)
    if(FM_LIBUSB_UDEV)
        find_package(PkgConfig QUIET)
        if(PkgConfig_FOUND)
            pkg_check_modules(UDEV QUIET libudev)
            if(UDEV_FOUND)
                target_link_directories(fmaker-explore-gui PRIVATE ${UDEV_LIBRARY_DIRS})
                target_link_libraries(fmaker-explore-gui PRIVATE ${UDEV_LIBRARIES})
            endif()
        endif()
    endif()
endif()

if(FM_STATIC_QT)
    # A static Qt only ships plugins that are explicitly imported.
    if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
        qt_import_plugins(fmaker-explore-gui NO_DEFAULT
            INCLUDE Qt::QXcbIntegrationPlugin
                    Qt::QWaylandIntegrationPlugin
                    Qt::QWaylandXdgShellIntegrationPlugin
                    Qt::QWaylandEglClientBufferPlugin
                    Qt::QOffscreenIntegrationPlugin
                    Qt::QComposePlatformInputContextPlugin)
    else()
        qt_import_plugins(fmaker-explore-gui
            INCLUDE_BY_TYPE platforms
            INCLUDE_BY_TYPE styles)
    endif()
endif()

if(FM_STATIC_RUNTIME)
    if(MSVC)
        set_property(TARGET fmaker-explore-gui PROPERTY
            MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
    elseif(CMAKE_CXX_COMPILER_ID STREQUAL "GNU" AND NOT APPLE)
        target_link_options(fmaker-explore-gui PRIVATE -static-libstdc++ -static-libgcc)
    endif()
endif()

set_target_properties(fmaker-explore-gui PROPERTIES
    MACOSX_BUNDLE_BUNDLE_NAME "FMakerExplore"
    MACOSX_BUNDLE_BUNDLE_VERSION "${PROJECT_VERSION}"
    MACOSX_BUNDLE_SHORT_VERSION_STRING "${PROJECT_VERSION}"
    MACOSX_BUNDLE_GUI_IDENTIFIER "cn.gov3.fmakerexplore")

if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    enable_testing()
    add_executable(linuxlauncher_test
        gui/tests/linuxlauncher_test.cpp
        gui/src/linuxlauncher.cpp)
    target_include_directories(linuxlauncher_test PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/gui/src")
    target_compile_options(linuxlauncher_test PRIVATE -Wall -Wextra -Wpedantic)
    add_test(NAME linuxlauncher COMMAND linuxlauncher_test)
endif()

install(TARGETS fmaker-explore-gui
    BUNDLE DESTINATION .
    RUNTIME DESTINATION bin)
