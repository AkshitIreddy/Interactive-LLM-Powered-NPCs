cmake_minimum_required(VERSION 3.24)
project(npc_moonshine_bridge VERSION 1.0.0 LANGUAGES CXX)

if(NOT WIN32)
  message(FATAL_ERROR "npc-moonshine-bridge is a Windows-only target")
endif()

set(NPC_MOONSHINE_SDK_ROOT "" CACHE PATH "Pinned Moonshine Windows SDK root")
if(NOT NPC_MOONSHINE_SDK_ROOT)
  message(FATAL_ERROR "NPC_MOONSHINE_SDK_ROOT is required")
endif()

foreach(required_file IN ITEMS
    "include/moonshine-c-api.h"
    "lib/moonshine.lib"
    "lib/bin-tokenizer.lib"
    "lib/ort-utils.lib"
    "lib/moonshine-utils.lib"
    "lib/onnxruntime.lib"
    "lib/onnxruntime.dll")
  if(NOT EXISTS "${NPC_MOONSHINE_SDK_ROOT}/${required_file}")
    message(FATAL_ERROR "Pinned Moonshine SDK is incomplete: ${required_file}")
  endif()
endforeach()

add_library(npc-moonshine-bridge SHARED moonshine_bridge.cpp)
target_compile_features(npc-moonshine-bridge PRIVATE cxx_std_20)
target_include_directories(npc-moonshine-bridge PRIVATE
  "${NPC_MOONSHINE_SDK_ROOT}/include")
target_link_directories(npc-moonshine-bridge PRIVATE
  "${NPC_MOONSHINE_SDK_ROOT}/lib")
target_link_libraries(npc-moonshine-bridge PRIVATE
  moonshine
  bin-tokenizer
  ort-utils
  moonshine-utils
  onnxruntime
  ole32
  mmdevapi)

set_property(TARGET npc-moonshine-bridge PROPERTY
  MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")
target_compile_definitions(npc-moonshine-bridge PRIVATE
  WIN32_LEAN_AND_MEAN
  NOMINMAX
  _CRT_SECURE_NO_WARNINGS)
target_compile_options(npc-moonshine-bridge PRIVATE
  /W4 /permissive- /EHsc /utf-8 /guard:cf)
target_link_options(npc-moonshine-bridge PRIVATE
  /DYNAMICBASE /NXCOMPAT /guard:cf)
set_target_properties(npc-moonshine-bridge PROPERTIES
  OUTPUT_NAME "npc-moonshine-bridge"
  PREFIX "")

add_custom_command(TARGET npc-moonshine-bridge POST_BUILD
  COMMAND "${CMAKE_COMMAND}" -E copy_if_different
    "${NPC_MOONSHINE_SDK_ROOT}/lib/onnxruntime.dll"
    "$<TARGET_FILE_DIR:npc-moonshine-bridge>/onnxruntime.dll"
  VERBATIM)
