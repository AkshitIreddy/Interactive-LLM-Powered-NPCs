cmake_minimum_required(VERSION 3.24)

project(npc_media_broker VERSION 2.0.0 LANGUAGES CXX)

option(NPC_MEDIA_BROKER_BUILD_TESTS "Build the portable media-broker tests" ON)
option(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS "Treat compiler warnings as errors" OFF)

add_library(npc_media_broker_core STATIC
    src/audio_ring.cpp
    src/broker.cpp
    src/geometry.cpp
    src/ipc.cpp
    src/service_config.cpp
    src/simulated_platform.cpp
    src/target_policy.cpp
)

target_compile_features(npc_media_broker_core PUBLIC cxx_std_20)
target_include_directories(npc_media_broker_core PUBLIC include)
set_target_properties(npc_media_broker_core PROPERTIES
    CXX_EXTENSIONS OFF
    POSITION_INDEPENDENT_CODE ON
)

if(MSVC)
    target_compile_options(npc_media_broker_core PRIVATE /W4 /permissive- /EHsc /utf-8 /wd4324)
    if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
        target_compile_options(npc_media_broker_core PRIVATE /WX)
    endif()
else()
    target_compile_options(npc_media_broker_core PRIVATE -Wall -Wextra -Wpedantic -Wconversion -Wshadow)
    if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
        target_compile_options(npc_media_broker_core PRIVATE -Werror)
    endif()
endif()

if(WIN32)
    add_library(npc_media_broker_windows STATIC
        src/windows_audio.cpp
        src/windows_capture.cpp
        src/windows_overlay.cpp
        src/windows_platform.cpp
        src/windows_service.cpp
    )
    target_compile_features(npc_media_broker_windows PUBLIC cxx_std_20)
    target_include_directories(npc_media_broker_windows PUBLIC include)
    target_link_libraries(npc_media_broker_windows
        PUBLIC npc_media_broker_core
        PRIVATE d3d11 dxgi dcomp dwmapi ole32 runtimeobject user32 avrt windowsapp advapi32
    )
    target_compile_definitions(npc_media_broker_windows PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
    if(MSVC)
        target_compile_options(npc_media_broker_windows PRIVATE /W4 /permissive- /EHsc /utf-8 /FS /wd4324)
    endif()
endif()

add_executable(npc-media-broker src/main.cpp)
target_compile_features(npc-media-broker PRIVATE cxx_std_20)
target_link_libraries(npc-media-broker PRIVATE npc_media_broker_core)
if(WIN32)
    target_link_libraries(npc-media-broker PRIVATE npc_media_broker_windows)
endif()
if(MSVC)
    target_compile_options(npc-media-broker PRIVATE /W4 /permissive- /EHsc /utf-8 /wd4324)
    if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
        target_compile_options(npc-media-broker PRIVATE /WX)
    endif()
endif()

include(CTest)
if(BUILD_TESTING AND NPC_MEDIA_BROKER_BUILD_TESTS)
    add_executable(npc_media_broker_tests tests/media_broker_tests.cpp)
    target_compile_features(npc_media_broker_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_media_broker_tests PRIVATE npc_media_broker_core)
    if(MSVC)
        target_compile_options(npc_media_broker_tests PRIVATE /W4 /permissive- /EHsc /utf-8 /wd4324)
        if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_media_broker_tests PRIVATE /WX)
        endif()
    endif()
    add_test(NAME npc_media_broker_tests COMMAND npc_media_broker_tests)
    if(WIN32)
        add_executable(npc_media_broker_windows_smoke_tests tests/windows_smoke_tests.cpp)
        target_compile_features(npc_media_broker_windows_smoke_tests PRIVATE cxx_std_20)
        target_include_directories(npc_media_broker_windows_smoke_tests PRIVATE src)
        target_compile_definitions(npc_media_broker_windows_smoke_tests PRIVATE
            NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
        target_link_libraries(npc_media_broker_windows_smoke_tests
            PRIVATE npc_media_broker_windows d3d11 user32)
        add_dependencies(npc_media_broker_windows_smoke_tests npc-media-broker)
        if(MSVC)
            target_compile_options(npc_media_broker_windows_smoke_tests PRIVATE /W4 /permissive- /EHsc /utf-8 /FS /wd4324)
            if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
                target_compile_options(npc_media_broker_windows_smoke_tests PRIVATE /WX)
            endif()
        endif()
        add_test(NAME npc_media_broker_windows_smoke_tests COMMAND npc_media_broker_windows_smoke_tests)
        add_test(NAME npc_media_broker_service_protocol_tests
                 COMMAND npc_media_broker_windows_smoke_tests --service-only)
    endif()
endif()
