#include "npc/media_broker/broker.hpp"
#include "npc/media_broker/ipc.hpp"
#include "npc/media_broker/service_config.hpp"
#include "npc/media_broker/target_policy.hpp"

#ifdef _WIN32
#include "windows_service.hpp"
#include <Windows.h>
#endif

#include <chrono>
#include <cstddef>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <span>
#include <string_view>
#include <thread>
#include <type_traits>
#include <vector>

namespace {

using namespace npc::media;

template <typename T>
void append_little(std::vector<std::byte>& output, T value) {
    static_assert(std::is_unsigned_v<T>);
    for (unsigned shift = 0; shift < sizeof(T) * 8; shift += 8) {
        output.push_back(static_cast<std::byte>((value >> shift) & 0xffU));
    }
}

[[nodiscard]] std::vector<std::byte> diagnostics_payload(const Diagnostics& value) {
    std::vector<std::byte> result;
    result.reserve(96);
    append_little(result, static_cast<std::uint32_t>(value.state));
    append_little(result, static_cast<std::uint32_t>(value.capture_backend));
    append_little(result, static_cast<std::uint32_t>(value.overlay_backend));
    append_little(result, static_cast<std::uint32_t>(value.capture_audio));
    append_little(result, static_cast<std::uint32_t>(value.render_audio));
    append_little(result, static_cast<std::uint32_t>(value.target_state));
    append_little(result, value.device_generation);
    append_little(result, value.audio_device_generation);
    append_little(result, value.cancellation_generation);
    append_little(result, value.frames_received);
    append_little(result, value.frames_presented);
    append_little(result, value.frames_dropped);
    append_little(result, value.overlays_suppressed);
    append_little(result, value.patches_received);
    append_little(result, value.patches_presented);
    append_little(result, value.patches_rejected);
    return result;
}

void append_ring(std::vector<std::byte>& output, const SharedPcmRing* ring) {
    append_little(output, static_cast<std::uint32_t>(ring != nullptr));
    if (!ring) {
        for (int index = 0; index < 9; ++index) append_little(output, std::uint32_t{});
        return;
    }
    const auto& format = ring->format();
    append_little(output, format.sample_rate);
    append_little(output, static_cast<std::uint32_t>(format.channels));
    append_little(output, static_cast<std::uint32_t>(format.bits_per_sample));
    append_little(output, static_cast<std::uint32_t>(format.block_align));
    append_little(output, static_cast<std::uint32_t>(format.sample_kind));
    append_little(output, ring->capacity_frames());
    append_little(output, ring->available_frames());
    append_little(output, ring->overflow_frames());
    append_little(output, ring->underflow_frames());
}

[[nodiscard]] std::vector<std::byte> audio_payload(IMediaPlatform& platform) {
    std::vector<std::byte> result;
    result.reserve(128);
    append_little(result, std::uint32_t{1});
    append_little(result, std::uint32_t{0}); // cross-process mapping is not negotiated
    append_ring(result, platform.capture_pcm_ring());
    append_ring(result, platform.render_pcm_ring());
    return result;
}

#ifdef _WIN32
[[nodiscard]] MonotonicTime qpc_to_monotonic(const std::uint64_t timestamp,
                                             const std::uint64_t now_qpc,
                                             const std::uint64_t frequency,
                                             const MonotonicTime now) {
    if (timestamp >= now_qpc || frequency == 0) return now;
    const auto nanoseconds = static_cast<std::uint64_t>(
        (static_cast<long double>(now_qpc - timestamp) * 1'000'000'000.0L) /
        static_cast<long double>(frequency));
    return now - std::chrono::nanoseconds(nanoseconds);
}

[[nodiscard]] ipc::Response process_command(const ipc::Envelope& envelope,
                                            const ipc::Command& command,
                                            MediaBroker& broker,
                                            bool& shutdown,
                                            const std::uint64_t now_qpc,
                                            const std::uint64_t frequency) {
    ipc::Response response{ipc::protocol_version, envelope.sequence, ipc::StatusCode::ok,
                           broker.diagnostics().cancellation_generation, {}};
    if (std::holds_alternative<ipc::HealthCommand>(command)) {
        append_little(response.payload, now_qpc);
        append_little(response.payload, static_cast<std::uint32_t>(broker.diagnostics().state));
    } else if (const auto* select = std::get_if<ipc::SelectTargetCommand>(&command)) {
        const auto evidence = windows::inspect_target(
            reinterpret_cast<HWND>(static_cast<std::uintptr_t>(select->native_window)),
            select->expected_process_id);
        const auto decision = evaluate_target_policy(evidence, select->allowed_process_names);
        if (!decision.capture_allowed) {
            broker.clear_target();
            response.status = ipc::StatusCode::target_blocked;
            append_little(response.payload, static_cast<std::uint32_t>(decision.reason));
        } else {
            GameTarget target{static_cast<std::uintptr_t>(select->native_window), evidence.process_id,
                              evidence.process_name, evidence.process_name};
            if (!broker.select_target(std::move(target))) response.status = ipc::StatusCode::target_blocked;
        }
    } else if (std::holds_alternative<ipc::ClearTargetCommand>(command)) {
        broker.clear_target();
    } else if (const auto* ptt = std::get_if<ipc::ConfigurePttCommand>(&command)) {
        Failure failure;
        broker.platform().unregister_ptt_hotkey();
        if (!broker.platform().register_ptt_hotkey(ptt->virtual_key, failure)) response.status = ipc::StatusCode::payload_invalid;
    } else if (std::holds_alternative<ipc::AudioStatusCommand>(command)) {
        response.payload = audio_payload(broker.platform());
        response.status = ipc::StatusCode::capability_unavailable;
    } else if (const auto* occlusion = std::get_if<ipc::SubmitOcclusionCommand>(&command)) {
        const auto now = std::chrono::steady_clock::now();
        broker.submit_occlusion_evidence({occlusion->face_confidence, occlusion->landmark_confidence,
                                          occlusion->visibility_ratio, occlusion->mouth_occluded,
                                          qpc_to_monotonic(occlusion->measured_qpc, now_qpc, frequency, now),
                                          occlusion->source_frame_sequence,
                                          occlusion->source_device_generation});
    } else if (std::holds_alternative<ipc::SubmitPatchCommand>(command)) {
        response.status = ipc::StatusCode::capability_unavailable;
    } else if (const auto* cancel = std::get_if<ipc::CancelCommand>(&command)) {
        if (cancel->new_generation <= broker.diagnostics().cancellation_generation) {
            response.status = ipc::StatusCode::cancellation_mismatch;
        } else {
            broker.cancel_generation(cancel->new_generation);
            response.cancellation_generation = broker.diagnostics().cancellation_generation;
        }
    } else if (std::holds_alternative<ipc::DiagnosticsCommand>(command)) {
        response.payload = diagnostics_payload(broker.diagnostics());
    } else if (std::holds_alternative<ipc::ShutdownCommand>(command)) {
        shutdown = true;
    }
    // Target selection, clearing, and cancellation invalidate in-flight visual
    // work. Return the post-command generation so an authenticated controller
    // can issue its next envelope without guessing how the broker advanced it.
    response.cancellation_generation = broker.diagnostics().cancellation_generation;
    return response;
}
#endif

} // namespace

int main(int argc, char** argv) {
    std::vector<std::string_view> arguments;
    arguments.reserve(argc > 1 ? static_cast<std::size_t>(argc - 1) : 0);
    for (int index = 1; index < argc; ++index) arguments.emplace_back(argv[index]);
    const auto config = parse_service_launch_arguments(arguments);
    if (!config) {
        std::cerr << "media service launch context is missing or malformed\n";
        return EXIT_FAILURE;
    }

#ifndef _WIN32
    std::cerr << "media service is supported only on Windows\n";
    return EXIT_FAILURE;
#else
    HANDLE parent{};
    if (!windows::verify_parent_and_job(*config, parent)) {
        std::cerr << "media service parent or Job Object verification failed\n";
        return EXIT_FAILURE;
    }
    MediaBroker broker(create_windows_media_platform());
    if (!broker.start()) {
        CloseHandle(parent);
        std::cerr << "media service initialization failed\n";
        return EXIT_FAILURE;
    }
    windows::CurrentUserPipeServer pipe(config->pipe_name, config->parent_process_id);
    if (!pipe.start()) {
        broker.stop();
        CloseHandle(parent);
        std::cerr << "media service control endpoint failed\n";
        return EXIT_FAILURE;
    }

    const auto frequency = windows::qpc_frequency();
    ipc::EnvelopeValidator validator({config->nonce, config->session_id, frequency * 10});
    bool shutdown{};
    while (!shutdown) {
        if (WaitForSingleObject(parent, 0) == WAIT_OBJECT_0 || pipe.disconnected()) break;
        broker.tick();
        if (auto envelope = pipe.take_request()) {
            const auto now_qpc = windows::qpc_now();
            const auto status = validator.validate(*envelope, now_qpc,
                                                   broker.diagnostics().cancellation_generation);
            ipc::Response response{ipc::protocol_version, envelope->sequence, status,
                                   broker.diagnostics().cancellation_generation, {}};
            if (status == ipc::StatusCode::ok) {
                auto command = ipc::decode_command(envelope->command, envelope->payload);
                response = command ? process_command(*envelope, *command, broker, shutdown, now_qpc, frequency)
                                   : ipc::Response{ipc::protocol_version, envelope->sequence,
                                                   ipc::StatusCode::payload_invalid,
                                                   broker.diagnostics().cancellation_generation, {}};
            }
            const auto response_sequence = response.response_to_sequence;
            if (!pipe.submit_response(std::move(response))) break;
            if (shutdown) {
                const auto flush_deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(500);
                while (!pipe.response_sent(response_sequence) &&
                       std::chrono::steady_clock::now() < flush_deadline && !pipe.disconnected()) {
                    std::this_thread::sleep_for(std::chrono::milliseconds(1));
                }
            }
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }
    pipe.stop();
    broker.stop();
    CloseHandle(parent);
    return EXIT_SUCCESS;
#endif
}
