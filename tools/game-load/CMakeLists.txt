cmake_minimum_required(VERSION 3.24)

project(ResponseConsoleGameLoadHarness VERSION 0.1.0 LANGUAGES CXX)

option(GAME_LOAD_BUILD_TESTS "Build portable policy tests" ON)
option(GAME_LOAD_WARNINGS_AS_ERRORS "Treat this tool's warnings as errors" OFF)

add_library(game_load_policy STATIC
  src/policy.cpp
)
add_library(game_load::policy ALIAS game_load_policy)
target_include_directories(game_load_policy PUBLIC include)
target_compile_features(game_load_policy PUBLIC cxx_std_20)

if(MSVC)
  target_compile_options(game_load_policy PRIVATE /W4 /permissive- /EHsc)
  if(GAME_LOAD_WARNINGS_AS_ERRORS)
    target_compile_options(game_load_policy PRIVATE /WX)
  endif()
else()
  target_compile_options(game_load_policy PRIVATE -Wall -Wextra -Wpedantic)
  if(GAME_LOAD_WARNINGS_AS_ERRORS)
    target_compile_options(game_load_policy PRIVATE -Werror)
  endif()
endif()

if(WIN32)
  set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/shaders/scene.hlsli"
    "${CMAKE_CURRENT_SOURCE_DIR}/shaders/pressure.hlsli"
  )
  # .hlsli keeps Visual Studio from compiling these as its default vs_2_0 target.
  # They are embedded below and compiled at runtime with explicit shader targets.
  file(READ "${CMAKE_CURRENT_SOURCE_DIR}/shaders/scene.hlsli" GAME_LOAD_SCENE_HLSL)
  file(READ "${CMAKE_CURRENT_SOURCE_DIR}/shaders/pressure.hlsli" GAME_LOAD_PRESSURE_HLSL)
  configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/src/win32/shader_sources.hpp.in"
    "${CMAKE_CURRENT_BINARY_DIR}/generated/shader_sources.hpp"
    @ONLY
  )

  add_executable(game-load
    src/win32/main.cpp
    src/win32/command_line.cpp
    src/win32/cpu_ram_load.cpp
    src/win32/d3d12_harness.cpp
    src/win32/etw_markers.cpp
    src/win32/result_manifest.cpp
    src/win32/system_monitor.cpp
  )
  target_include_directories(game-load PRIVATE
    include
    src/win32
    "${CMAKE_CURRENT_BINARY_DIR}/generated"
  )
  target_compile_features(game-load PRIVATE cxx_std_20)
  target_compile_definitions(game-load PRIVATE
    NOMINMAX
    UNICODE
    _UNICODE
    WIN32_LEAN_AND_MEAN
    GAME_LOAD_VERSION="${PROJECT_VERSION}"
  )
  target_compile_options(game-load PRIVATE /W4 /permissive- /EHsc /utf-8)
  if(GAME_LOAD_WARNINGS_AS_ERRORS)
    target_compile_options(game-load PRIVATE /WX)
  endif()
  target_link_libraries(game-load PRIVATE
    game_load::policy
    d3d12
    dxgi
    dxguid
    d3dcompiler
    wbemuuid
    advapi32
    ole32
    oleaut32
    psapi
    user32
    shell32
  )
endif()

if(GAME_LOAD_BUILD_TESTS)
  include(CTest)
  add_executable(game_load_policy_tests tests/policy_tests.cpp)
  target_link_libraries(game_load_policy_tests PRIVATE game_load::policy)
  target_compile_features(game_load_policy_tests PRIVATE cxx_std_20)
  if(MSVC)
    target_compile_options(game_load_policy_tests PRIVATE /W4 /permissive- /EHsc)
  else()
    target_compile_options(game_load_policy_tests PRIVATE -Wall -Wextra -Wpedantic)
  endif()
  add_test(NAME game_load_policy_tests COMMAND game_load_policy_tests)
endif()
