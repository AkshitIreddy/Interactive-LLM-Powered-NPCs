#include "npc/media_broker/broker.hpp"
#include "npc/media_broker/ipc.hpp"
#include "npc/media_broker/service_config.hpp"
#include "npc/media_broker/target_policy.hpp"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <span>
#include <string_view>
#include <utility>
#include <vector>

namespace {

using namespace npc::media;

int failures{};

#define CHECK(...)                                                                                        \
    do {                                                                                                  \
        if (!(__VA_ARGS__)) {                                                                             \
            std::cerr << __FILE__ << ':' << __LINE__ << ": CHECK failed: " #__VA_ARGS__ << '\n';        \
            ++failures;                                                                                   \
        }                                                                                                 \
    } while (false)

[[nodiscard]] GameTarget safe_target() {
    return {0x1234U, 42U, "eclipse-harbor.exe", "Eclipse Harbor"};
}

[[nodiscard]] TargetGeometry standard_geometry() {
    return {
        {100, 80, 2020, 1160},
        {100, 80, 2020, 1160},
        {100, 80, 2020, 1160},
        {1920, 1080},
        {"display-1", {0, 0, 2560, 1440}, {0, 0, 2560, 1400}, 144, 144,
         ColorSpace::sdr_srgb, DisplayRotation::identity, 80.0},
        false,
    };
}

void test_geometry_negative_origin_clipping_and_hdr() {
    const TargetGeometry target{
        {-2700, -40, -300, 1440},
        {-2400, 100, -400, 1300},
        {-2560, 0, 0, 1440},
        {2560, 1440},
        {"left-hdr", {-2560, 0, 0, 1440}, {-2560, 0, 0, 1400}, 120, 144,
         ColorSpace::hdr10_pq, DisplayRotation::identity, 203.0},
        false,
    };

    const auto geometry = calculate_overlay_geometry(target);
    CHECK(geometry.has_value());
    CHECK(geometry->desktop_bounds_px == target.client_bounds_px);
    CHECK(geometry->clipped_desktop_bounds_px == target.client_bounds_px);
    CHECK(geometry->source_crop_px == RectI{160, 100, 2160, 1300});
    CHECK(geometry->source_size_px == SizeI{2560, 1440});
    CHECK(geometry->dpi_scale_x == 1.25);
    CHECK(geometry->dpi_scale_y == 1.5);
    CHECK(geometry->tone_map_required);
    CHECK(geometry->display_rotation == DisplayRotation::identity);

    const auto patch = map_normalized_source_rect({0.25, 0.25, 0.5, 0.5}, *geometry);
    CHECK(patch.has_value());
    CHECK(*patch == RectI{-1920, 360, -1280, 720});
}

void test_geometry_rejects_minimized_or_invalid_targets() {
    auto target = standard_geometry();
    target.minimized = true;
    CHECK(!calculate_overlay_geometry(target));
    target.minimized = false;
    target.captured_content_px = {};
    CHECK(!calculate_overlay_geometry(target));
    CHECK(!map_normalized_rect({-0.1, 0.2, 0.3, 0.4}, {0, 0, 100, 100}));
}

void test_mailbox_keeps_only_latest_value() {
    LatestValueMailbox<int> mailbox;
    CHECK(!mailbox.push(1).replaced_unread);
    CHECK(mailbox.push(2).replaced_unread);
    CHECK(mailbox.push(3).replaced_unread);
    CHECK(mailbox.dropped() == 2);
    const auto value = mailbox.take_latest();
    CHECK(value && *value == 3);
    CHECK(!mailbox.take_latest());
}

struct SampleMetrics {
    double peak{};
    double rms{};
    std::size_t clipped_samples{};
    double maximum_boundary_delta{};
};

[[nodiscard]] SampleMetrics measure_samples(const std::span<const float> samples) {
    SampleMetrics result;
    double sum_squared{};
    for (std::size_t index = 0; index < samples.size(); ++index) {
        const double absolute = std::abs(static_cast<double>(samples[index]));
        result.peak = std::max(result.peak, absolute);
        sum_squared += absolute * absolute;
        result.clipped_samples += absolute >= 1.0 ? 1U : 0U;
        if (index > 0) {
            result.maximum_boundary_delta = std::max(
                result.maximum_boundary_delta,
                std::abs(static_cast<double>(samples[index] - samples[index - 1])));
        }
    }
    result.rms = samples.empty() ? 0.0 : std::sqrt(sum_squared / static_cast<double>(samples.size()));
    return result;
}

void test_pcm_ring_measures_signal_silence_wrap_and_boundaries() {
    constexpr double pi = 3.14159265358979323846;
    const PcmFormat format{48000, 1, 32, 4, PcmSampleKind::floating_point};
    SharedPcmRing ring(format, 64);
    std::vector<float> signal(96);
    for (std::size_t index = 0; index < signal.size(); ++index) {
        signal[index] = static_cast<float>(0.5 * std::sin(2.0 * pi * 1000.0 *
                                                         static_cast<double>(index) / 48000.0));
    }

    CHECK(ring.write(std::as_bytes(std::span{signal}.first(48)), 48).transferred_frames == 48);
    std::vector<float> output(96, 0.0F);
    CHECK(ring.read(std::as_writable_bytes(std::span{output}.first(32)), 32).transferred_frames == 32);
    CHECK(ring.write(std::as_bytes(std::span{signal}.subspan(48)), 48).transferred_frames == 48);
    CHECK(ring.read(std::as_writable_bytes(std::span{output}.subspan(32)), 64).transferred_frames == 64);
    CHECK(output == signal);

    const auto metrics = measure_samples(output);
    CHECK(metrics.peak > 0.49 && metrics.peak <= 0.5);
    CHECK(metrics.rms > 0.34 && metrics.rms < 0.36);
    CHECK(metrics.clipped_samples == 0);
    CHECK(metrics.maximum_boundary_delta < 0.066);
    std::cout << "audio metrics: peak=" << metrics.peak << " rms=" << metrics.rms
              << " clipped=" << metrics.clipped_samples
              << " max_chunk_delta=" << metrics.maximum_boundary_delta << '\n';

    ring.clear();
    std::vector<float> silence(16, 0.0F);
    CHECK(ring.write(std::as_bytes(std::span{silence}), 16).transferred_frames == 16);
    std::vector<float> silent_output(16, 1.0F);
    CHECK(ring.read(std::as_writable_bytes(std::span{silent_output}), 16).transferred_frames == 16);
    const auto silence_metrics = measure_samples(silent_output);
    CHECK(silence_metrics.peak == 0.0);
    CHECK(silence_metrics.rms == 0.0);
    CHECK(silence_metrics.clipped_samples == 0);
}

void test_pcm_ring_reports_overrun_and_underrun_exactly() {
    SharedPcmRing ring({48000, 1, 32, 4, PcmSampleKind::floating_point}, 8);
    std::vector<float> input(10, 0.25F);
    const auto write = ring.write(std::as_bytes(std::span{input}), 10);
    CHECK(write.transferred_frames == 8);
    CHECK(write.overflowed);
    CHECK(ring.overflow_frames() == 2);

    std::vector<float> output(10, 0.0F);
    const auto read = ring.read(std::as_writable_bytes(std::span{output}), 10);
    CHECK(read.transferred_frames == 8);
    CHECK(read.underflowed);
    CHECK(ring.underflow_frames() == 2);
    CHECK(measure_samples(output).peak == 0.25);
}

void test_cancellation_clears_queued_render_audio_to_measured_silence() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    MediaBroker broker(std::move(platform));
    CHECK(broker.start());
    auto* render = simulation->render_pcm_ring();
    CHECK(render != nullptr);
    std::vector<float> queued(16, 0.75F);
    CHECK(render->write(std::as_bytes(std::span{queued}), 8).transferred_frames == 8);
    broker.cancel_generation(1);

    std::vector<float> after_cancel(16, 0.0F);
    const auto read = render->read(std::as_writable_bytes(std::span{after_cancel}), 8);
    CHECK(read.transferred_frames == 0);
    CHECK(read.underflowed);
    const auto metrics = measure_samples(after_cancel);
    CHECK(metrics.peak == 0.0);
    CHECK(metrics.rms == 0.0);
}

void test_audio_device_loss_recreates_rings_and_generation() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    MediaBroker broker(std::move(platform));
    CHECK(broker.start());
    auto* capture = simulation->capture_pcm_ring();
    std::vector<float> captured(16, 0.3F);
    CHECK(capture->write(std::as_bytes(std::span{captured}), 8).transferred_frames == 8);
    simulation->emit_failure({FailureDomain::audio_capture, FailureCode::device_removed, true,
                              "simulated audio endpoint reset"});
    broker.tick(std::chrono::steady_clock::now() + std::chrono::seconds(1));
    CHECK(simulation->counters().audio_recreates == 1);
    CHECK(broker.diagnostics().audio_device_generation == 1);
    CHECK(simulation->capture_pcm_ring()->available_frames() == 0);
    CHECK(broker.diagnostics().capture_audio == AudioState::ready);
    CHECK(broker.diagnostics().render_audio == AudioState::ready);
}

void test_primary_capture_falls_back_without_hooking() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    simulation->fail_next_capture(
        CaptureBackend::windows_graphics_capture,
        {FailureDomain::capture, FailureCode::backend_unavailable, true, "WGC unavailable"});
    MediaBroker broker(std::move(platform));
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    CHECK(broker.diagnostics().capture_backend == CaptureBackend::desktop_duplication);
    CHECK(broker.diagnostics().state == BrokerState::capturing_fallback);
    CHECK(simulation->counters().capture_starts == 2);
}

void test_latest_frame_and_high_confidence_patch_are_presented() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    CompositingDecision decision{CompositingDecision::no_frame};
    MediaBroker broker(std::move(platform), {},
                       {.on_compositing_decision = [&](const CompositingDecision value) { decision = value; }});
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    simulation->emit_target(TargetState::selected, standard_geometry());

    const auto now = std::chrono::steady_clock::now();
    simulation->emit_frame({1, 0, now, {1920, 1080}, ColorSpace::sdr_srgb, 1, false, false});
    simulation->emit_frame({2, 0, now, {1920, 1080}, ColorSpace::sdr_srgb, 2, false, false});
    broker.submit_occlusion_evidence({0.98, 0.97, 0.94, false, now});
    broker.submit_patch({2, broker.diagnostics().cancellation_generation,
                         {0.42, 0.56, 0.58, 0.70}, 0.96, now, 99});
    broker.tick(now + std::chrono::milliseconds(1));

    CHECK(decision == CompositingDecision::patch);
    CHECK(simulation->counters().patch_presentations == 1);
    CHECK(simulation->counters().pristine_presentations == 0);
    CHECK(broker.diagnostics().frames_received == 2);
    CHECK(broker.diagnostics().frames_dropped == 1);
}

void test_occlusion_and_staleness_fail_open_to_pristine_game() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    CompositingDecision decision{CompositingDecision::no_frame};
    MediaBroker broker(std::move(platform), {},
                       {.on_compositing_decision = [&](const CompositingDecision value) { decision = value; }});
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    simulation->emit_target(TargetState::selected, standard_geometry());

    const auto now = std::chrono::steady_clock::now();
    simulation->emit_frame({7, 0, now, {1920, 1080}, ColorSpace::sdr_srgb, 1, false, false});
    broker.submit_occlusion_evidence({0.99, 0.99, 0.99, true, now});
    broker.submit_patch({7, broker.diagnostics().cancellation_generation,
                         {0.4, 0.5, 0.6, 0.7}, 0.99, now, 88});
    broker.tick(now + std::chrono::milliseconds(1));
    CHECK(decision == CompositingDecision::pristine_occluded);
    CHECK(simulation->counters().pristine_presentations == 1);
    CHECK(simulation->counters().patch_presentations == 0);

    simulation->emit_frame({8, 0, now, {1920, 1080}, ColorSpace::sdr_srgb, 2, false, false});
    broker.submit_occlusion_evidence({0.99, 0.99, 0.99, false, now});
    broker.submit_patch({8, broker.diagnostics().cancellation_generation,
                         {0.4, 0.5, 0.6, 0.7}, 0.99, now, 89});
    broker.tick(now + std::chrono::milliseconds(500));
    CHECK(decision == CompositingDecision::pristine_stale_frame);
    CHECK(simulation->counters().pristine_presentations == 2);
}

void test_late_generation_is_never_composited() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    CompositingDecision decision{CompositingDecision::no_frame};
    MediaBroker broker(std::move(platform), {},
                       {.on_compositing_decision = [&](const CompositingDecision value) { decision = value; }});
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    simulation->emit_target(TargetState::selected, standard_geometry());
    const auto old_generation = broker.diagnostics().cancellation_generation;
    broker.cancel_generation(old_generation + 1);

    const auto now = std::chrono::steady_clock::now();
    simulation->emit_frame({9, 0, now, {1920, 1080}, ColorSpace::sdr_srgb, 3, false, false});
    broker.submit_occlusion_evidence({0.99, 0.99, 0.99, false, now});
    broker.submit_patch({9, old_generation, {0.4, 0.5, 0.6, 0.7}, 0.99, now, 90});
    broker.tick(now + std::chrono::milliseconds(1));
    CHECK(decision == CompositingDecision::pristine_wrong_generation);
    CHECK(simulation->counters().pristine_presentations == 1);
}

void test_device_loss_recreates_generation_and_capture() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    MediaBroker broker(std::move(platform));
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    simulation->emit_target(TargetState::selected, standard_geometry());
    simulation->emit_failure({FailureDomain::device, FailureCode::device_removed, true, "simulated reset"});
    CHECK(broker.diagnostics().state == BrokerState::recovering_device);
    broker.tick(std::chrono::steady_clock::now() + std::chrono::seconds(1));
    CHECK(broker.diagnostics().device_generation == 1);
    CHECK(simulation->counters().graphics_recreates == 1);
    CHECK(broker.diagnostics().capture_backend == CaptureBackend::windows_graphics_capture);
}

void test_ptt_press_and_release_are_forwarded() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    int transitions{};
    PttState last{PttState::released};
    MediaBroker broker(std::move(platform), {},
                       {.on_ptt = [&](const PttState value, MonotonicTime) {
                            ++transitions;
                            last = value;
                        }});
    CHECK(broker.start());
    simulation->emit_ptt(PttState::pressed);
    simulation->emit_ptt(PttState::released);
    CHECK(transitions == 2);
    CHECK(last == PttState::released);
}

void test_ipc_codec_is_deterministic_and_typed() {
    using namespace npc::media::ipc;
    std::array<std::byte, launch_nonce_bytes> nonce{};
    for (std::size_t index = 0; index < nonce.size(); ++index) nonce[index] = static_cast<std::byte>(index);
    const Command command = SelectTargetCommand{0x12345678U, 42U, {"skyrimse.exe", "fallout4.exe"}};
    const auto payload = encode_command(CommandKind::select_target, command);
    CHECK(payload.has_value());
    const Envelope envelope{protocol_version, nonce, "session-1234", 7, 9000, 3,
                            CommandKind::select_target, *payload};
    const auto first = encode_envelope(envelope);
    const auto second = encode_envelope(envelope);
    CHECK(first && second && *first == *second);
    const auto decoded = decode_envelope(*first);
    CHECK(decoded && decoded->nonce == nonce && decoded->session_id == "session-1234");
    CHECK(decoded && decoded->sequence == 7 && decoded->command == CommandKind::select_target);
    const auto decoded_command = decode_command(decoded->command, decoded->payload);
    CHECK(decoded_command && std::holds_alternative<SelectTargetCommand>(*decoded_command));
    const auto& target = std::get<SelectTargetCommand>(*decoded_command);
    CHECK(target.native_window == 0x12345678U && target.expected_process_id == 42U);
    CHECK(target.allowed_process_names == std::vector<std::string>{"skyrimse.exe", "fallout4.exe"});

    const auto framed = frame_message(*first);
    CHECK(framed.size() == first->size() + 4);
    std::array<std::byte, 4> prefix{};
    std::copy_n(framed.begin(), 4, prefix.begin());
    CHECK(decode_frame_size(prefix) == first->size());
    const std::array<std::byte, 4> oversized{std::byte{1}, std::byte{0}, std::byte{1}, std::byte{0}};
    CHECK(!decode_frame_size(oversized));

    const Command patch = SubmitPatchCommand{12, 3, 0.4, 0.5, 0.6, 0.7, 0.92, 8500,
        SharedTextureDescriptor{99, 100, 1, 2, 256, 128, 87}};
    const auto encoded_patch = encode_command(CommandKind::submit_patch, patch);
    const auto decoded_patch = encoded_patch ? decode_command(CommandKind::submit_patch, *encoded_patch) : std::nullopt;
    CHECK(decoded_patch && std::holds_alternative<SubmitPatchCommand>(*decoded_patch));
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).shared_texture->width == 256);

    const std::vector<std::pair<CommandKind, Command>> typed_commands{
        {CommandKind::health, HealthCommand{}},
        {CommandKind::clear_target, ClearTargetCommand{}},
        {CommandKind::configure_ptt, ConfigurePttCommand{0x77}},
        {CommandKind::audio_status, AudioStatusCommand{}},
        {CommandKind::submit_occlusion, SubmitOcclusionCommand{0.9, 0.91, 0.92, false, 8000}},
        {CommandKind::cancel, CancelCommand{4}},
        {CommandKind::diagnostics, DiagnosticsCommand{}},
        {CommandKind::shutdown, ShutdownCommand{}},
    };
    for (const auto& [kind, typed] : typed_commands) {
        const auto encoded = encode_command(kind, typed);
        CHECK(encoded && decode_command(kind, *encoded).has_value());
    }
}

void test_command_surface_cannot_modify_or_extend_the_game_process() {
    using namespace npc::media::ipc;
    constexpr std::array commands{
        CommandKind::health,
        CommandKind::select_target,
        CommandKind::clear_target,
        CommandKind::configure_ptt,
        CommandKind::audio_status,
        CommandKind::submit_occlusion,
        CommandKind::submit_patch,
        CommandKind::cancel,
        CommandKind::diagnostics,
        CommandKind::shutdown,
    };
    for (const auto command : commands) {
        const auto effect = target_process_effect(command);
        CHECK(effect == TargetProcessEffect::none ||
              effect == TargetProcessEffect::read_only_inspection_and_external_capture);
    }
    CHECK(target_process_effect(CommandKind::select_target) ==
          TargetProcessEffect::read_only_inspection_and_external_capture);
    CHECK(!ExternalGameBoundary::permits_native_rig_animation);
    CHECK(!ExternalGameBoundary::permits_executable_adapters);
    CHECK(!ExternalGameBoundary::permits_process_injection);
    CHECK(!ExternalGameBoundary::permits_game_hooks);
    CHECK(!ExternalGameBoundary::permits_game_module_loading);
    CHECK(!ExternalGameBoundary::permits_process_memory_writes);
    CHECK(ExternalGameBoundary::capture_backends ==
          std::array{CaptureBackend::windows_graphics_capture,
                     CaptureBackend::desktop_duplication});
}

void test_ipc_auth_sequence_deadline_and_cancellation_validation() {
    using namespace npc::media::ipc;
    std::array<std::byte, launch_nonce_bytes> nonce{};
    nonce[0] = std::byte{0xaa};
    EnvelopeValidator validator({nonce, "session-1234", 1000});
    Envelope envelope{protocol_version, nonce, "session-1234", 1, 1500, 4,
                      CommandKind::health, {}};
    CHECK(validator.validate(envelope, 1000, 4) == StatusCode::ok);
    CHECK(validator.validate(envelope, 1000, 4) == StatusCode::sequence_replayed);

    envelope.sequence = 2;
    envelope.nonce[0] = std::byte{0xbb};
    CHECK(validator.validate(envelope, 1000, 4) == StatusCode::authentication_failed);
    envelope.nonce = nonce;
    envelope.session_id = "wrong-session";
    CHECK(validator.validate(envelope, 1000, 4) == StatusCode::session_mismatch);
    envelope.session_id = "session-1234";
    envelope.deadline_qpc = 999;
    CHECK(validator.validate(envelope, 1000, 4) == StatusCode::deadline_expired);
    envelope.deadline_qpc = 2500;
    CHECK(validator.validate(envelope, 1000, 4) == StatusCode::deadline_too_far);
    envelope.deadline_qpc = 1500;
    envelope.cancellation_generation = 3;
    CHECK(validator.validate(envelope, 1000, 4) == StatusCode::cancellation_mismatch);
    envelope.command = CommandKind::cancel;
    CHECK(validator.validate(envelope, 1000, 4) == StatusCode::ok);
}

void test_service_launch_arguments_fail_closed() {
    const std::string nonce(64, 'a');
    const std::vector<std::string> storage{
        "--parent-pid=42", "--session=session-1234", "--nonce=" + nonce};
    const std::vector<std::string_view> arguments{storage[0], storage[1], storage[2]};
    const auto config = parse_service_launch_arguments(arguments);
    CHECK(config && config->parent_process_id == 42 && config->session_id == "session-1234");
    CHECK(config && config->pipe_name == "\\\\.\\pipe\\npc-media-broker-session-1234");
    const std::vector<std::string_view> missing{"--parent-pid=42", "--session=session-1234"};
    CHECK(!parse_service_launch_arguments(missing));
    const std::vector<std::string_view> traversal{"--parent-pid=42", "--session=../../pipe",
                                                   "--nonce=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"};
    CHECK(!parse_service_launch_arguments(traversal));
    const std::vector<std::string> adapter_storage{
        "--parent-pid=42", "--session=session-1234", "--nonce=" + nonce,
        "--adapter=C:\\untrusted\\game-hook.dll"};
    const std::vector<std::string_view> adapter_arguments{
        adapter_storage[0], adapter_storage[1], adapter_storage[2], adapter_storage[3]};
    CHECK(!parse_service_launch_arguments(adapter_arguments));

    const std::vector<std::string> hook_storage{
        "--parent-pid=42", "--session=session-1234", "--nonce=" + nonce,
        "--hook-address=0x1234"};
    const std::vector<std::string_view> hook_arguments{
        hook_storage[0], hook_storage[1], hook_storage[2], hook_storage[3]};
    CHECK(!parse_service_launch_arguments(hook_arguments));
}

[[nodiscard]] TargetInspectionEvidence safe_evidence() {
    return {true, false, false, true, true, true, true, 42, "skyrimse.exe",
            {"skyrimse.exe", "kernel32.dll", "d3d11.dll"}};
}

void test_target_policy_blocks_untrusted_and_ambiguous_targets() {
    auto evidence = safe_evidence();
    CHECK(evaluate_target_policy(evidence, {"skyrimse.exe"}).capture_allowed);
    evidence.session_matches = false;
    CHECK(evaluate_target_policy(evidence, {"skyrimse.exe"}).reason == TargetBlockReason::cross_session);
    evidence = safe_evidence();
    evidence.user_matches = false;
    CHECK(evaluate_target_policy(evidence, {"skyrimse.exe"}).reason == TargetBlockReason::cross_user);
    evidence = safe_evidence();
    evidence.loaded_module_names.push_back("EasyAntiCheat_EOS.sys");
    CHECK(evaluate_target_policy(evidence, {"skyrimse.exe"}).reason == TargetBlockReason::anti_cheat_marker);
    evidence = safe_evidence();
    evidence.process_name = "gta5.exe";
    CHECK(evaluate_target_policy(evidence, {"gta5.exe"}).reason == TargetBlockReason::online_ambiguity);
    evidence = safe_evidence();
    evidence.inspection_complete = false;
    CHECK(evaluate_target_policy(evidence, {"skyrimse.exe"}).reason == TargetBlockReason::inspection_incomplete);
    evidence = safe_evidence();
    CHECK(evaluate_target_policy(evidence, {}).reason == TargetBlockReason::not_allowlisted);
    evidence = safe_evidence();
    evidence.minimized = true;
    CHECK(evaluate_target_policy(evidence, {"skyrimse.exe"}).reason == TargetBlockReason::minimized);
    evidence = safe_evidence();
    evidence.protected_process = true;
    CHECK(evaluate_target_policy(evidence, {"skyrimse.exe"}).reason == TargetBlockReason::protected_process);
}

} // namespace

int main() {
    test_geometry_negative_origin_clipping_and_hdr();
    test_geometry_rejects_minimized_or_invalid_targets();
    test_mailbox_keeps_only_latest_value();
    test_pcm_ring_measures_signal_silence_wrap_and_boundaries();
    test_pcm_ring_reports_overrun_and_underrun_exactly();
    test_cancellation_clears_queued_render_audio_to_measured_silence();
    test_audio_device_loss_recreates_rings_and_generation();
    test_primary_capture_falls_back_without_hooking();
    test_latest_frame_and_high_confidence_patch_are_presented();
    test_occlusion_and_staleness_fail_open_to_pristine_game();
    test_late_generation_is_never_composited();
    test_device_loss_recreates_generation_and_capture();
    test_ptt_press_and_release_are_forwarded();
    test_ipc_codec_is_deterministic_and_typed();
    test_command_surface_cannot_modify_or_extend_the_game_process();
    test_ipc_auth_sequence_deadline_and_cancellation_validation();
    test_service_launch_arguments_fail_closed();
    test_target_policy_blocks_untrusted_and_ambiguous_targets();

    if (failures != 0) {
        std::cerr << failures << " media-broker test(s) failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "all media-broker state, geometry, recovery, IPC, target-policy, and fail-open tests passed\n";
    return EXIT_SUCCESS;
}
