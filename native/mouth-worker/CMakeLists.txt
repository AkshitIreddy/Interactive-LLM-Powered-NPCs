cmake_minimum_required(VERSION 3.24)

project(npc_mouth_worker VERSION 1.0.0 LANGUAGES CXX)

option(NPC_MOUTH_WORKER_BUILD_TESTS "Build deterministic mouth-worker tests" ON)
option(NPC_MOUTH_WORKER_BUILD_BENCHMARKS "Build the CPU reference benchmark" ON)
option(NPC_MOUTH_WORKER_BUILD_SYNTHETIC_PROOF "Build the synthetic proof generator" ON)
option(NPC_MOUTH_WORKER_BUILD_HEADLESS_REALISTIC_PROOF
       "Build the real OpenSeeFace + audio headless render proof" ON)
option(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS "Treat compiler warnings as errors" OFF)

add_library(npc_mouth_worker STATIC
    src/compositor.cpp
    src/landmark_provider.cpp
    src/product_runtime.cpp
    src/service_protocol.cpp
    src/signal_adapter.cpp
    src/worker.cpp
)
target_compile_features(npc_mouth_worker PUBLIC cxx_std_20)
target_include_directories(npc_mouth_worker PUBLIC include)
set_target_properties(npc_mouth_worker PROPERTIES
    CXX_EXTENSIONS OFF
    POSITION_INDEPENDENT_CODE ON
)

add_executable(npc-mouth-worker
    src/service_main.cpp
    src/windows_service.cpp
    src/windows_ort_landmark_provider.cpp
)
target_compile_features(npc-mouth-worker PRIVATE cxx_std_20)
target_link_libraries(npc-mouth-worker PRIVATE npc_mouth_worker)
set_target_properties(npc-mouth-worker PROPERTIES OUTPUT_NAME "npc-mouth-worker")
if(WIN32)
    set_target_properties(npc-mouth-worker PROPERTIES WIN32_EXECUTABLE TRUE)
    target_link_libraries(npc-mouth-worker PRIVATE bcrypt d3d11 dxgi)
    if(MSVC)
        target_link_options(npc-mouth-worker PRIVATE /ENTRY:mainCRTStartup)
    endif()
endif()

if(MSVC)
    target_compile_options(npc-mouth-worker PRIVATE /W4 /permissive- /EHsc /utf-8)
    if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
        target_compile_options(npc-mouth-worker PRIVATE /WX)
    endif()
else()
    target_compile_options(npc-mouth-worker PRIVATE -Wall -Wextra -Wpedantic -Wconversion -Wshadow)
    if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
        target_compile_options(npc-mouth-worker PRIVATE -Werror)
    endif()
endif()

if(MSVC)
    target_compile_options(npc_mouth_worker PRIVATE /W4 /permissive- /EHsc /utf-8)
    if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
        target_compile_options(npc_mouth_worker PRIVATE /WX)
    endif()
else()
    target_compile_options(npc_mouth_worker PRIVATE -Wall -Wextra -Wpedantic -Wconversion -Wshadow)
    if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
        target_compile_options(npc_mouth_worker PRIVATE -Werror)
    endif()
endif()

include(CTest)
if(BUILD_TESTING AND NPC_MOUTH_WORKER_BUILD_TESTS)
    add_executable(npc_mouth_compositor_render_tests tests/compositor_render_tests.cpp)
    target_compile_features(npc_mouth_compositor_render_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_compositor_render_tests PRIVATE npc_mouth_worker)
    add_test(NAME npc_mouth_compositor_render_tests COMMAND npc_mouth_compositor_render_tests)

    add_executable(npc_mouth_review_cues_tests tests/review_cues_tests.cpp)
    target_compile_features(npc_mouth_review_cues_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_review_cues_tests PRIVATE npc_mouth_worker)
    add_test(NAME npc_mouth_review_cues_tests COMMAND npc_mouth_review_cues_tests)

    add_executable(npc_mouth_worker_tests tests/mouth_worker_tests.cpp)
    target_compile_features(npc_mouth_worker_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_worker_tests PRIVATE npc_mouth_worker)
    add_test(NAME npc_mouth_worker_tests COMMAND npc_mouth_worker_tests)

    add_executable(npc_mouth_product_runtime_tests tests/product_runtime_tests.cpp)
    target_compile_features(npc_mouth_product_runtime_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_product_runtime_tests PRIVATE npc_mouth_worker)
    add_test(NAME npc_mouth_product_runtime_tests COMMAND npc_mouth_product_runtime_tests)

    add_executable(npc_mouth_service_protocol_tests tests/service_protocol_tests.cpp)
    target_compile_features(npc_mouth_service_protocol_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_service_protocol_tests PRIVATE npc_mouth_worker)
    add_test(NAME npc_mouth_service_protocol_tests COMMAND npc_mouth_service_protocol_tests)

    add_executable(npc_mouth_landmark_provider_tests tests/landmark_provider_tests.cpp)
    target_compile_features(npc_mouth_landmark_provider_tests PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_landmark_provider_tests PRIVATE npc_mouth_worker)
    add_test(NAME npc_mouth_landmark_provider_tests COMMAND npc_mouth_landmark_provider_tests)

    if(WIN32)
        add_executable(npc_mouth_worker_pe_tests tests/pe_subsystem_tests.cpp)
        target_compile_features(npc_mouth_worker_pe_tests PRIVATE cxx_std_20)
        add_test(NAME npc_mouth_worker_pe_tests
                 COMMAND npc_mouth_worker_pe_tests $<TARGET_FILE:npc-mouth-worker>)

        add_executable(npc_mouth_worker_windows_service_smoke_tests
                       tests/windows_service_smoke_tests.cpp
                       src/windows_service.cpp)
        target_compile_features(npc_mouth_worker_windows_service_smoke_tests PRIVATE cxx_std_20)
        target_link_libraries(npc_mouth_worker_windows_service_smoke_tests
                              PRIVATE npc_mouth_worker bcrypt d3d11 dxgi)
        if(MSVC)
            target_compile_options(npc_mouth_worker_windows_service_smoke_tests
                                   PRIVATE /W4 /permissive- /EHsc /utf-8)
            if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
                target_compile_options(npc_mouth_worker_windows_service_smoke_tests PRIVATE /WX)
            endif()
        endif()
        add_test(NAME npc_mouth_worker_windows_service_smoke_tests
                 COMMAND npc_mouth_worker_windows_service_smoke_tests
                         $<TARGET_FILE:npc-mouth-worker>)
    endif()
endif()

if(NPC_MOUTH_WORKER_BUILD_BENCHMARKS)
    add_executable(npc_mouth_worker_benchmark benchmarks/mouth_worker_benchmark.cpp)
    target_compile_features(npc_mouth_worker_benchmark PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_worker_benchmark PRIVATE npc_mouth_worker)
endif()

if(NPC_MOUTH_WORKER_BUILD_SYNTHETIC_PROOF)
    add_executable(npc_mouth_worker_synthetic_proof tools/synthetic_proof.cpp)
    target_compile_features(npc_mouth_worker_synthetic_proof PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_worker_synthetic_proof PRIVATE npc_mouth_worker)
endif()

if(WIN32 AND NPC_MOUTH_WORKER_BUILD_HEADLESS_REALISTIC_PROOF)
    add_executable(npc_mouth_worker_tracked_replay tools/replay_tracked_mouth.cpp)
    target_compile_features(npc_mouth_worker_tracked_replay PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_worker_tracked_replay PRIVATE npc_mouth_worker bcrypt psapi)
    if(MSVC)
        target_compile_options(npc_mouth_worker_tracked_replay PRIVATE /W4 /permissive- /EHsc /utf-8)
        if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_mouth_worker_tracked_replay PRIVATE /WX)
        endif()
    endif()

    add_executable(npc_mouth_worker_headless_realistic_proof
                   tools/headless_realistic_proof.cpp
                   src/windows_ort_landmark_provider.cpp)
    target_compile_features(npc_mouth_worker_headless_realistic_proof PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_worker_headless_realistic_proof
                          PRIVATE npc_mouth_worker bcrypt psapi)
    if(MSVC)
        target_compile_options(npc_mouth_worker_headless_realistic_proof
                               PRIVATE /W4 /permissive- /EHsc /utf-8)
        if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_mouth_worker_headless_realistic_proof PRIVATE /WX)
        endif()
    endif()

    add_executable(npc_mouth_worker_landmark_dump
                   tools/landmark_dump.cpp
                   src/windows_ort_landmark_provider.cpp)
    target_compile_features(npc_mouth_worker_landmark_dump PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_worker_landmark_dump
                          PRIVATE npc_mouth_worker bcrypt)
    if(MSVC)
        target_compile_options(npc_mouth_worker_landmark_dump
                               PRIVATE /W4 /permissive- /EHsc /utf-8)
        if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_mouth_worker_landmark_dump PRIVATE /WX)
        endif()
    endif()

    add_executable(npc_mouth_worker_yunet_qualifier
                   tools/yunet_native_qualifier.cpp
                   src/windows_ort_landmark_provider.cpp)
    target_compile_features(npc_mouth_worker_yunet_qualifier PRIVATE cxx_std_20)
    target_link_libraries(npc_mouth_worker_yunet_qualifier
                          PRIVATE npc_mouth_worker bcrypt psapi)
    if(MSVC)
        target_compile_options(npc_mouth_worker_yunet_qualifier
                               PRIVATE /W4 /permissive- /EHsc /utf-8)
        if(NPC_MOUTH_WORKER_WARNINGS_AS_ERRORS)
            target_compile_options(npc_mouth_worker_yunet_qualifier PRIVATE /WX)
        endif()
    endif()
endif()
