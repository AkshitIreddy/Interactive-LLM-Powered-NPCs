cmake_minimum_required(VERSION 3.24)

project(npc_media_broker VERSION 2.0.0 LANGUAGES CXX)

option(NPC_MEDIA_BROKER_BUILD_TESTS "Build the portable media-broker tests" ON)
option(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS "Treat compiler warnings as errors" OFF)

add_library(npc_media_broker_core STATIC
    src/audio_ring.cpp
    src/broker.cpp
    src/geometry.cpp
    src/input_transport.cpp
    src/ipc.cpp
    src/playback_transport.cpp
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
        src/windows_identity_reference.cpp
        src/windows_manual_actor_picker.cpp
        src/windows_overlay.cpp
        src/windows_input.cpp
        src/windows_presentation_context.cpp
        src/windows_playback.cpp
        src/windows_platform.cpp
        src/windows_service.cpp
    )
    target_compile_features(npc_media_broker_windows PUBLIC cxx_std_20)
    target_include_directories(npc_media_broker_windows PUBLIC include)
    target_link_libraries(npc_media_broker_windows
        PUBLIC npc_media_broker_core
        PRIVATE d3d11 dxgi dcomp dwmapi ole32 runtimeobject user32 avrt windowsapp advapi32 bcrypt
                shell32 windowscodecs
    )
    target_compile_definitions(npc_media_broker_windows PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
    if(MSVC)
        target_compile_options(npc_media_broker_windows PRIVATE /W4 /permissive- /EHsc /utf-8 /FS /wd4324)
        if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_media_broker_windows PRIVATE /WX)
        endif()
    endif()
endif()

add_executable(npc-media-broker src/main.cpp)
target_compile_features(npc-media-broker PRIVATE cxx_std_20)
target_link_libraries(npc-media-broker PRIVATE npc_media_broker_core)
if(WIN32)
    # The broker is a supervised hidden product child. Keep the normal `main`
    # argument parser while preventing Explorer/review launches from creating a
    # console window.
    set_target_properties(npc-media-broker PROPERTIES WIN32_EXECUTABLE TRUE)
    target_link_libraries(npc-media-broker PRIVATE npc_media_broker_windows)
    if(MSVC)
        target_link_options(npc-media-broker PRIVATE /ENTRY:mainCRTStartup)
    endif()
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
    add_executable(npc_media_broker_display_matrix_tests
        tests/display_matrix_tests.cpp)
    target_compile_features(npc_media_broker_display_matrix_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_media_broker_display_matrix_tests PRIVATE npc_media_broker_core)
    if(MSVC)
        target_compile_options(npc_media_broker_display_matrix_tests PRIVATE
            /W4 /permissive- /EHsc /utf-8 /wd4324)
        if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_media_broker_display_matrix_tests PRIVATE /WX)
        endif()
    endif()
    add_test(NAME npc_media_broker_display_matrix_tests
             COMMAND npc_media_broker_display_matrix_tests)
    add_executable(npc_media_broker_playback_transport_tests tests/playback_transport_tests.cpp)
    target_compile_features(npc_media_broker_playback_transport_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_media_broker_playback_transport_tests PRIVATE npc_media_broker_core)
    if(MSVC)
        target_compile_options(npc_media_broker_playback_transport_tests PRIVATE /W4 /permissive- /EHsc /utf-8 /wd4324 /UNDEBUG)
        if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_media_broker_playback_transport_tests PRIVATE /WX)
        endif()
    endif()
    add_test(NAME npc_media_broker_playback_transport_tests
             COMMAND npc_media_broker_playback_transport_tests)

    add_executable(npc_media_broker_presentation_context_tests
        tests/presentation_context_tests.cpp)
    target_compile_features(npc_media_broker_presentation_context_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_media_broker_presentation_context_tests PRIVATE npc_media_broker_core)
    if(MSVC)
        target_compile_options(npc_media_broker_presentation_context_tests PRIVATE
            /W4 /permissive- /EHsc /utf-8 /wd4324)
        if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_media_broker_presentation_context_tests PRIVATE /WX)
        endif()
    endif()
    add_test(NAME npc_media_broker_presentation_context_tests
             COMMAND npc_media_broker_presentation_context_tests)
    add_executable(npc_media_broker_input_transport_tests
        tests/input_transport_tests.cpp)
    target_compile_features(npc_media_broker_input_transport_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_media_broker_input_transport_tests PRIVATE npc_media_broker_core)
    if(MSVC)
        target_compile_options(npc_media_broker_input_transport_tests PRIVATE
            /W4 /permissive- /EHsc /utf-8 /wd4324)
        if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_media_broker_input_transport_tests PRIVATE /WX)
        endif()
    endif()
    add_test(NAME npc_media_broker_input_transport_tests
             COMMAND npc_media_broker_input_transport_tests)
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
        add_test(NAME npc_media_broker_capture_lifecycle_regression
                 COMMAND npc_media_broker_windows_smoke_tests --direct-shared-only)
        add_test(NAME npc_media_broker_service_protocol_tests
                 COMMAND npc_media_broker_windows_smoke_tests --service-only)
        add_executable(npc_media_broker_windows_display_smoke_tests
                       tests/windows_display_smoke_tests.cpp)
        target_compile_features(npc_media_broker_windows_display_smoke_tests PRIVATE cxx_std_20)
        target_include_directories(npc_media_broker_windows_display_smoke_tests PRIVATE src)
        target_compile_definitions(npc_media_broker_windows_display_smoke_tests PRIVATE
            NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
        target_link_libraries(npc_media_broker_windows_display_smoke_tests
            PRIVATE npc_media_broker_windows d3d11 dwmapi user32)
        if(MSVC)
            target_compile_options(npc_media_broker_windows_display_smoke_tests PRIVATE
                /W4 /permissive- /EHsc /utf-8 /FS /wd4324)
            if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
                target_compile_options(npc_media_broker_windows_display_smoke_tests PRIVATE /WX)
            endif()
        endif()
        add_test(NAME npc_media_broker_windows_display_smoke_tests
                 COMMAND npc_media_broker_windows_display_smoke_tests)
        add_executable(npc_media_broker_windows_playback_smoke_tests
                       tests/windows_playback_smoke_tests.cpp)
        target_compile_features(npc_media_broker_windows_playback_smoke_tests PRIVATE cxx_std_20)
        target_include_directories(npc_media_broker_windows_playback_smoke_tests PRIVATE src)
        target_compile_definitions(npc_media_broker_windows_playback_smoke_tests PRIVATE
            NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
        target_link_libraries(npc_media_broker_windows_playback_smoke_tests
            PRIVATE npc_media_broker_windows)
        if(MSVC)
            target_compile_options(npc_media_broker_windows_playback_smoke_tests PRIVATE
                /W4 /permissive- /EHsc /utf-8 /FS /wd4324 /UNDEBUG)
            if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
                target_compile_options(npc_media_broker_windows_playback_smoke_tests PRIVATE /WX)
            endif()
        endif()
        add_test(NAME npc_media_broker_windows_playback_smoke_tests
                 COMMAND npc_media_broker_windows_playback_smoke_tests)
        add_executable(npc_media_broker_windows_input_smoke_tests
                       tests/windows_input_smoke_tests.cpp)
        target_compile_features(npc_media_broker_windows_input_smoke_tests PRIVATE cxx_std_20)
        target_include_directories(npc_media_broker_windows_input_smoke_tests PRIVATE src)
        target_compile_definitions(npc_media_broker_windows_input_smoke_tests PRIVATE
            NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
        target_link_libraries(npc_media_broker_windows_input_smoke_tests
            PRIVATE npc_media_broker_windows)
        if(MSVC)
            target_compile_options(npc_media_broker_windows_input_smoke_tests PRIVATE
                /W4 /permissive- /EHsc /utf-8 /FS /wd4324)
            if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
                target_compile_options(npc_media_broker_windows_input_smoke_tests PRIVATE /WX)
            endif()
        endif()
        add_test(NAME npc_media_broker_windows_input_smoke_tests
                 COMMAND npc_media_broker_windows_input_smoke_tests)
        add_executable(npc_media_broker_windows_identity_smoke_tests
                       tests/windows_identity_smoke_tests.cpp)
        target_compile_features(npc_media_broker_windows_identity_smoke_tests PRIVATE cxx_std_20)
        target_include_directories(npc_media_broker_windows_identity_smoke_tests PRIVATE src)
        target_compile_definitions(npc_media_broker_windows_identity_smoke_tests PRIVATE
            NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
        target_link_libraries(npc_media_broker_windows_identity_smoke_tests
            PRIVATE npc_media_broker_windows user32 bcrypt)
        add_dependencies(npc_media_broker_windows_identity_smoke_tests npc-media-broker)
        if(MSVC)
            target_compile_options(npc_media_broker_windows_identity_smoke_tests PRIVATE
                /W4 /permissive- /EHsc /utf-8 /FS /wd4324)
            if(NPC_MEDIA_BROKER_WARNINGS_AS_ERRORS)
                target_compile_options(npc_media_broker_windows_identity_smoke_tests PRIVATE /WX)
            endif()
        endif()
        add_test(NAME npc_media_broker_windows_identity_smoke_tests
                 COMMAND npc_media_broker_windows_identity_smoke_tests)
        # These tests create a real broker sidecar and exercise the user's shared
        # WASAPI/WGC devices. Keep them isolated even under parallel CTest runs.
        set_tests_properties(
            npc_media_broker_windows_smoke_tests
            npc_media_broker_capture_lifecycle_regression
            npc_media_broker_service_protocol_tests
            npc_media_broker_windows_display_smoke_tests
            npc_media_broker_windows_playback_smoke_tests
            npc_media_broker_windows_input_smoke_tests
            npc_media_broker_windows_identity_smoke_tests
            PROPERTIES
                LABELS interactive_windows_smoke
                RESOURCE_LOCK npc_media_broker_windows_services)
    endif()
endif()
