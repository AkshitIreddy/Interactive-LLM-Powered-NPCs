cmake_minimum_required(VERSION 3.24)

project(npc_subtitle_renderer VERSION 2.0.0 LANGUAGES CXX)

option(NPC_SUBTITLE_RENDERER_BUILD_TESTS "Build deterministic portable renderer tests" ON)
option(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS "Treat compiler warnings as errors" OFF)

add_library(npc_subtitle_renderer STATIC src/renderer.cpp src/presentation.cpp)
add_library(npc::subtitle_renderer ALIAS npc_subtitle_renderer)
target_compile_features(npc_subtitle_renderer PUBLIC cxx_std_20)
target_include_directories(npc_subtitle_renderer PUBLIC include)
set_target_properties(npc_subtitle_renderer PROPERTIES
    CXX_EXTENSIONS OFF
    POSITION_INDEPENDENT_CODE ON
)

if(MSVC)
    target_compile_options(npc_subtitle_renderer PRIVATE /W4 /permissive- /EHsc /utf-8)
    if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
        target_compile_options(npc_subtitle_renderer PRIVATE /WX)
    endif()
else()
    target_compile_options(npc_subtitle_renderer PRIVATE
        -Wall -Wextra -Wpedantic -Wconversion -Wshadow)
    if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
        target_compile_options(npc_subtitle_renderer PRIVATE -Werror)
    endif()
endif()

if(WIN32)
    add_library(npc_subtitle_renderer_windows STATIC
        src/windows_directwrite.cpp
        src/windows_surface.cpp)
    add_library(npc::subtitle_renderer_windows ALIAS npc_subtitle_renderer_windows)
    target_compile_features(npc_subtitle_renderer_windows PUBLIC cxx_std_20)
    target_include_directories(npc_subtitle_renderer_windows PUBLIC include)
    target_compile_definitions(npc_subtitle_renderer_windows PRIVATE
        NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
    target_link_libraries(npc_subtitle_renderer_windows
        PUBLIC npc_subtitle_renderer
        PRIVATE dwrite d2d1 d3d11 dcomp dwmapi dxgi)
    if(MSVC)
        target_compile_options(npc_subtitle_renderer_windows PRIVATE
            /W4 /permissive- /EHsc /utf-8 /FS)
        if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_subtitle_renderer_windows PRIVATE /WX)
        endif()
    endif()

    add_executable(npc_subtitle_presenter WIN32 src/presenter_main.cpp)
    target_compile_features(npc_subtitle_presenter PRIVATE cxx_std_20)
    target_compile_definitions(npc_subtitle_presenter PRIVATE
        NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
    target_link_libraries(npc_subtitle_presenter PRIVATE npc_subtitle_renderer_windows)
    set_target_properties(npc_subtitle_presenter PROPERTIES OUTPUT_NAME "npc-subtitle-presenter")
    if(MSVC)
        target_compile_options(npc_subtitle_presenter PRIVATE
            /W4 /permissive- /EHsc /utf-8 /FS)
        if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_subtitle_presenter PRIVATE /WX)
        endif()
    endif()
endif()

include(CTest)
if(BUILD_TESTING AND NPC_SUBTITLE_RENDERER_BUILD_TESTS)
    add_executable(npc_subtitle_renderer_tests tests/subtitle_renderer_tests.cpp)
    target_compile_features(npc_subtitle_renderer_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_subtitle_renderer_tests PRIVATE npc_subtitle_renderer)
    if(MSVC)
        target_compile_options(npc_subtitle_renderer_tests PRIVATE /W4 /permissive- /EHsc /utf-8)
        if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_subtitle_renderer_tests PRIVATE /WX)
        endif()
    else()
        target_compile_options(npc_subtitle_renderer_tests PRIVATE
            -Wall -Wextra -Wpedantic -Wconversion -Wshadow)
        if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_subtitle_renderer_tests PRIVATE -Werror)
        endif()
    endif()
    add_test(NAME npc_subtitle_renderer_tests COMMAND npc_subtitle_renderer_tests)

    add_executable(npc_subtitle_presentation_tests tests/subtitle_presentation_tests.cpp)
    target_compile_features(npc_subtitle_presentation_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_subtitle_presentation_tests PRIVATE npc_subtitle_renderer)
    if(MSVC)
        target_compile_options(npc_subtitle_presentation_tests PRIVATE
            /W4 /permissive- /EHsc /utf-8)
        if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_subtitle_presentation_tests PRIVATE /WX)
        endif()
    else()
        target_compile_options(npc_subtitle_presentation_tests PRIVATE
            -Wall -Wextra -Wpedantic -Wconversion -Wshadow)
        if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_subtitle_presentation_tests PRIVATE -Werror)
        endif()
    endif()
    add_test(NAME npc_subtitle_presentation_tests COMMAND npc_subtitle_presentation_tests)

    if(WIN32)
        add_executable(npc_subtitle_windows_text_shaping_tests
            tests/windows_text_shaping_tests.cpp)
        target_compile_features(npc_subtitle_windows_text_shaping_tests PRIVATE cxx_std_20)
        target_link_libraries(npc_subtitle_windows_text_shaping_tests PRIVATE
            npc_subtitle_renderer_windows)
        if(MSVC)
            target_compile_options(npc_subtitle_windows_text_shaping_tests PRIVATE
                /W4 /permissive- /EHsc /utf-8)
            if(NPC_SUBTITLE_RENDERER_WARNINGS_AS_ERRORS)
                target_compile_options(npc_subtitle_windows_text_shaping_tests PRIVATE /WX)
            endif()
        endif()
        add_test(NAME npc_subtitle_windows_text_shaping_tests
            COMMAND npc_subtitle_windows_text_shaping_tests)
    endif()
endif()
