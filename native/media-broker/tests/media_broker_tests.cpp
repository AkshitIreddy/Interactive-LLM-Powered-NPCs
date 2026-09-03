#include "npc/media_broker/broker.hpp"
#include "npc/media_broker/ipc.hpp"
#include "npc/media_broker/service_config.hpp"
#include "npc/media_broker/target_policy.hpp"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <iostream>
#include <limits>
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

[[nodiscard]] OcclusionEvidence valid_occlusion(
    const MonotonicTime measured_at,
    const std::uint64_t frame_sequence,
    const std::uint64_t device_generation,
    const bool occluded = false,
    const std::uint64_t track_id = 77,
    const std::uint64_t actor_id = 11,
    const std::uint64_t track_epoch = 4) {
    return {.face_confidence = 0.99,
            .landmark_confidence = 0.99,
            .visibility_ratio = 0.99,
            .mouth_region_occluded = occluded,
            .measured_at = measured_at,
            .source_frame_sequence = frame_sequence,
            .source_device_generation = device_generation,
            .source_geometry_epoch = 1,
            .source_frame_qpc = 1,
            .actor_id = actor_id,
            .selected_track_id = track_id,
            .track_epoch = track_epoch};
}

[[nodiscard]] MouthPatch valid_patch(
    const MonotonicTime produced_at,
    const std::uint64_t frame_sequence,
    const std::uint64_t cancellation_generation,
    const std::uint64_t device_generation,
    const std::uintptr_t native_texture,
    const RectF bounds = {0.42, 0.56, 0.58, 0.70},
    const double confidence = 0.99,
    const std::uint64_t track_id = 77,
    const std::uint64_t actor_id = 11,
    const std::uint64_t track_epoch = 4) {
    return {.source_frame_sequence = frame_sequence,
            .cancellation_generation = cancellation_generation,
            .normalized_bounds = bounds,
            .confidence = confidence,
            .produced_at = produced_at,
            .native_texture = native_texture,
            .source_device_generation = device_generation,
            .source_frame_captured_at = produced_at,
            .source_geometry_epoch = 1,
            .source_frame_qpc = 1,
            .actor_id = actor_id,
            .selected_track_id = track_id,
            .track_epoch = track_epoch};
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
    simulation->emit_target(TargetState::selected, standard_geometry());
    CHECK(simulation->counters().overlay_starts == 0);
    CHECK(broker.diagnostics().overlay_backend == OverlayBackend::none);
    CHECK(!broker.diagnostics().overlay_visuals_allowed);
    CHECK(!broker.diagnostics().overlay_capture_excluded);
}

void test_initial_capture_binds_nonzero_device_generation() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    MediaBroker broker(std::move(platform));
    CHECK(broker.start());
    CHECK(broker.diagnostics().device_generation == 0);
    CHECK(broker.select_target(safe_target()));
    CHECK(broker.diagnostics().device_generation == 1);
    CHECK(simulation->counters().graphics_recreates == 1);

    broker.clear_target();
    CHECK(broker.select_target(safe_target()));
    CHECK(broker.diagnostics().device_generation == 1);
    CHECK(simulation->counters().graphics_recreates == 1);
}

void test_capture_evidence_requires_advancing_sequence_qpc_and_geometry_epoch() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    MediaBroker broker(std::move(platform));
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    simulation->emit_target(TargetState::selected, standard_geometry());
    const auto geometry_epoch = broker.diagnostics().geometry_epoch;
    const auto device_generation = broker.diagnostics().device_generation;
    const auto now = std::chrono::steady_clock::now();

    simulation->emit_frame({1, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 1, false, false,
                            geometry_epoch, 100, 0x1111});
    simulation->emit_frame({1, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 2, false, false,
                            geometry_epoch, 101, 0x2222});
    simulation->emit_frame({2, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 3, false, false,
                            geometry_epoch, 100, 0x2222});
    simulation->emit_frame({2, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 4, false, false,
                            geometry_epoch, 102, 0x2222});

    CHECK(broker.diagnostics().frames_received == 2);
    CHECK(broker.diagnostics().frames_dropped == 3); // two invalid plus one unread replacement
    CHECK(broker.diagnostics().nonadvancing_frames == 2);
    CHECK(broker.diagnostics().latest_frame_sequence == 2);
    CHECK(broker.diagnostics().latest_frame_qpc == 102);
    CHECK(broker.diagnostics().initial_content_hash == 0x1111);
    CHECK(broker.diagnostics().latest_content_hash == 0x2222);
    CHECK(broker.diagnostics().content_hash_changes == 1);
}

void test_geometry_epoch_invalidates_old_frames_and_tracks_minimize_restore() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    MediaBroker broker(std::move(platform));
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    auto geometry = standard_geometry();
    simulation->emit_target(TargetState::selected, geometry);
    const auto first_epoch = broker.diagnostics().geometry_epoch;
    const auto device_generation = broker.diagnostics().device_generation;
    CHECK(first_epoch > 0);
    CHECK(broker.diagnostics().overlay_capture_excluded);

    const auto now = std::chrono::steady_clock::now();
    simulation->emit_frame({5, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 5, false, false,
                            first_epoch, 500, 0xaaaa});
    geometry.window_bounds_px.right += 200;
    geometry.client_bounds_px.right += 200;
    geometry.captured_desktop_bounds_px.right += 200;
    geometry.captured_content_px.width += 200;
    simulation->emit_target(TargetState::selected, geometry);
    const auto resized_epoch = broker.diagnostics().geometry_epoch;
    CHECK(resized_epoch > first_epoch);
    CHECK(broker.diagnostics().latest_frame_sequence == 0);

    simulation->emit_frame({6, device_generation, now, {2120, 1080}, ColorSpace::sdr_srgb, 6, false, false,
                            first_epoch, 600, 0xbbbb});
    CHECK(broker.diagnostics().frames_received == 1);
    simulation->emit_frame({1, device_generation, now, {2120, 1080}, ColorSpace::sdr_srgb, 7, false, false,
                            resized_epoch, 601, 0xbbbb});
    CHECK(broker.diagnostics().frames_received == 2);

    geometry.monitor.dpi_x = 168;
    geometry.monitor.dpi_y = 168;
    simulation->emit_target(TargetState::selected, geometry);
    const auto dpi_epoch = broker.diagnostics().geometry_epoch;
    CHECK(dpi_epoch > resized_epoch);
    geometry.monitor.stable_id = "display-2";
    geometry.monitor.desktop_bounds_px = {2560, 0, 5120, 1440};
    geometry.monitor.work_area_px = {2560, 0, 5120, 1400};
    geometry.window_bounds_px.left += 2560;
    geometry.window_bounds_px.right += 2560;
    geometry.client_bounds_px.left += 2560;
    geometry.client_bounds_px.right += 2560;
    geometry.captured_desktop_bounds_px.left += 2560;
    geometry.captured_desktop_bounds_px.right += 2560;
    simulation->emit_target(TargetState::selected, geometry);
    const auto monitor_epoch = broker.diagnostics().geometry_epoch;
    CHECK(monitor_epoch > dpi_epoch);

    geometry.minimized = true;
    simulation->emit_target(TargetState::minimized, geometry);
    CHECK(broker.diagnostics().state == BrokerState::awaiting_target);
    CHECK(broker.diagnostics().capture_backend == CaptureBackend::windows_graphics_capture);
    CHECK(broker.diagnostics().overlay_backend == OverlayBackend::none);
    const auto minimized_epoch = broker.diagnostics().geometry_epoch;
    CHECK(minimized_epoch > monitor_epoch);

    geometry.minimized = false;
    simulation->emit_target(TargetState::selected, geometry);
    CHECK(broker.diagnostics().geometry_epoch > minimized_epoch);
    CHECK(broker.diagnostics().state == BrokerState::capturing_primary);
    CHECK(broker.diagnostics().overlay_capture_excluded);
}

void test_protected_exclusive_and_unsupported_paths_degrade_truthfully() {
    const auto run = [](const TargetState emitted, const TargetState expected,
                        const FailureCode code) {
        auto platform = std::make_unique<SimulatedMediaPlatform>();
        auto* simulation = platform.get();
        MediaBroker broker(std::move(platform));
        CHECK(broker.start());
        CHECK(broker.select_target(safe_target()));
        simulation->emit_target(TargetState::selected, standard_geometry());
        simulation->emit_target(emitted);
        CHECK(broker.diagnostics().state == BrokerState::degraded_audio_only);
        CHECK(broker.diagnostics().target_state == expected);
        CHECK(broker.diagnostics().capture_backend == CaptureBackend::none);
        CHECK(broker.diagnostics().overlay_backend == OverlayBackend::none);
        CHECK(broker.diagnostics().last_failure.has_value());
        CHECK(broker.diagnostics().last_failure->code == code);
    };
    run(TargetState::protected_content, TargetState::protected_content,
        FailureCode::protected_content);
    run(TargetState::exclusive_fullscreen, TargetState::exclusive_fullscreen,
        FailureCode::exclusive_fullscreen);
    run(TargetState::unsupported, TargetState::unsupported,
        FailureCode::unsupported_path);
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
    const auto device_generation = broker.diagnostics().device_generation;

    const auto now = std::chrono::steady_clock::now();
    simulation->emit_frame({1, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 1, false, false});
    simulation->emit_frame({2, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 2, false, false});
    auto evidence = valid_occlusion(now, 2, device_generation);
    evidence.face_confidence = 0.98;
    evidence.landmark_confidence = 0.97;
    evidence.visibility_ratio = 0.94;
    auto patch = valid_patch(now, 2, broker.diagnostics().cancellation_generation,
                             device_generation, 99);
    patch.confidence = 0.96;
    broker.submit_occlusion_evidence(std::move(evidence));
    broker.submit_patch(std::move(patch));
    broker.tick(now + std::chrono::milliseconds(1));

    CHECK(decision == CompositingDecision::patch);
    CHECK(simulation->counters().patch_presentations == 1);
    CHECK(simulation->counters().pristine_presentations == 0);
    CHECK(broker.diagnostics().frames_received == 2);
    CHECK(broker.diagnostics().frames_dropped == 1);
}

void test_residual_guard_rejects_unsafe_async_output_and_clears_prior_patch() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    CompositingDecision decision{CompositingDecision::no_frame};
    MediaBroker broker(std::move(platform), {},
                       {.on_compositing_decision = [&](const CompositingDecision value) { decision = value; }});
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    simulation->emit_target(TargetState::selected, standard_geometry());

    const auto generation = broker.diagnostics().cancellation_generation;
    const auto epoch = broker.diagnostics().device_generation;
    const auto base = std::chrono::steady_clock::now();

    const auto present_valid = [&](const std::uint64_t sequence, const MonotonicTime at) {
        simulation->emit_frame({sequence, epoch, at, {1920, 1080}, ColorSpace::sdr_srgb,
                                static_cast<std::uintptr_t>(sequence), false, false});
        broker.submit_occlusion_evidence(valid_occlusion(at, sequence, epoch));
        broker.submit_patch(valid_patch(at, sequence, generation, epoch, 1000 + sequence));
        broker.tick(at + std::chrono::milliseconds(1));
        CHECK(decision == CompositingDecision::patch);
        CHECK(simulation->residual_visible());
    };

    const auto reject = [&](const std::uint64_t sequence,
                            const MonotonicTime captured_at,
                            OcclusionEvidence evidence,
                            MouthPatch patch,
                            const MonotonicTime tick_at,
                            const CompositingDecision expected) {
        simulation->emit_frame({sequence, epoch, captured_at, {1920, 1080}, ColorSpace::sdr_srgb,
                                static_cast<std::uintptr_t>(sequence), false, false});
        broker.submit_occlusion_evidence(std::move(evidence));
        broker.submit_patch(std::move(patch));
        broker.tick(tick_at);
        CHECK(decision == expected);
        CHECK(!simulation->residual_visible());
    };

    present_valid(20, base);

    auto at = base + std::chrono::milliseconds(10);
    reject(21, at,
           valid_occlusion(at, 21, epoch),
           valid_patch(at, 21, generation, epoch, 1021, {0.0, 0.0, 1.0, 1.0}),
           at + std::chrono::milliseconds(1), CompositingDecision::pristine_unsafe_bounds);

    at += std::chrono::milliseconds(10);
    reject(22, at,
           valid_occlusion(at, 22, epoch),
           valid_patch(at, 22, generation, epoch + 1, 1022),
           at + std::chrono::milliseconds(1), CompositingDecision::pristine_wrong_epoch);

    at += std::chrono::milliseconds(10);
    auto wrong_source_time = valid_patch(at, 23, generation, epoch, 1023);
    wrong_source_time.source_frame_captured_at = at - std::chrono::milliseconds(10);
    reject(23, at,
           valid_occlusion(at, 23, epoch),
           std::move(wrong_source_time),
           at + std::chrono::milliseconds(1), CompositingDecision::pristine_wrong_source_time);

    at += std::chrono::milliseconds(10);
    auto late_patch = valid_patch(at + std::chrono::milliseconds(81), 24, generation, epoch, 1024);
    late_patch.source_frame_captured_at = at;
    reject(24, at,
           valid_occlusion(at, 24, epoch),
           std::move(late_patch),
           at + std::chrono::milliseconds(82), CompositingDecision::pristine_wrong_source_time);

    at += std::chrono::milliseconds(100);
    reject(25, at,
           valid_occlusion(at, 25, epoch),
           valid_patch(at, 25, generation, epoch, 1025, {0.42, 0.56, 0.58, 0.70},
                       std::numeric_limits<double>::quiet_NaN()),
           at + std::chrono::milliseconds(1), CompositingDecision::pristine_low_confidence);

    at += std::chrono::milliseconds(10);
    reject(26, at,
           valid_occlusion(at, 26, epoch),
           valid_patch(at, 26, generation, epoch, 0),
           at + std::chrono::milliseconds(1), CompositingDecision::pristine_missing_texture);

    at += std::chrono::milliseconds(10);
    reject(27, at,
           valid_occlusion(at, 26, epoch),
           valid_patch(at, 27, generation, epoch, 1027),
           at + std::chrono::milliseconds(1), CompositingDecision::pristine_wrong_frame);

    at += std::chrono::milliseconds(10);
    reject(29, at,
           valid_occlusion(at, 29, epoch, false, 77),
           valid_patch(at, 29, generation, epoch, 1029, {0.42, 0.56, 0.58, 0.70},
                       0.99, 78),
           at + std::chrono::milliseconds(1), CompositingDecision::pristine_wrong_track);

    CHECK(simulation->counters().patch_presentations == 1);
    CHECK(simulation->counters().pristine_presentations == 8);
    CHECK(broker.diagnostics().patches_received == 9);
    CHECK(broker.diagnostics().patches_presented == 1);
    CHECK(broker.diagnostics().patches_rejected == 8);

    at += std::chrono::milliseconds(10);
    present_valid(30, at);
    broker.cancel_generation(generation + 1);
    CHECK(!simulation->residual_visible());
    CHECK(simulation->counters().residual_suppressions > 0);
}

void test_residual_safety_ceiling_cannot_be_disabled_by_configuration() {
    BrokerPolicy policy;
    policy.timing.frame_stale_after = std::chrono::seconds(10);
    policy.timing.patch_stale_after = std::chrono::seconds(10);
    policy.timing.evidence_stale_after = std::chrono::seconds(10);
    policy.timing.maximum_source_to_patch_latency = std::chrono::seconds(10);
    policy.occlusion = {0.0, 0.0, 0.0, 0.0};
    policy.residual_bounds = {1.0, 1.0, 1.0};

    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    CompositingDecision decision{CompositingDecision::no_frame};
    MediaBroker broker(std::move(platform), policy,
                       {.on_compositing_decision = [&](const CompositingDecision value) { decision = value; }});
    CHECK(broker.start());
    CHECK(broker.select_target(safe_target()));
    simulation->emit_target(TargetState::selected, standard_geometry());

    const auto epoch = broker.diagnostics().device_generation;
    const auto generation = broker.diagnostics().cancellation_generation;
    auto at = std::chrono::steady_clock::now();
    simulation->emit_frame({31, epoch, at, {1920, 1080}, ColorSpace::sdr_srgb, 31, false, false});
    broker.submit_occlusion_evidence(valid_occlusion(at, 31, epoch));
    broker.submit_patch(valid_patch(at, 31, generation, epoch, 31, {0.0, 0.0, 1.0, 1.0}));
    broker.tick(at + std::chrono::milliseconds(1));
    CHECK(decision == CompositingDecision::pristine_unsafe_bounds);

    at += std::chrono::milliseconds(10);
    simulation->emit_frame({32, epoch, at, {1920, 1080}, ColorSpace::sdr_srgb, 32, false, false});
    auto low_evidence = valid_occlusion(at, 32, epoch);
    low_evidence.face_confidence = 0.5;
    low_evidence.landmark_confidence = 0.5;
    low_evidence.visibility_ratio = 0.5;
    broker.submit_occlusion_evidence(std::move(low_evidence));
    broker.submit_patch(valid_patch(at, 32, generation, epoch, 32,
                                    {0.42, 0.56, 0.58, 0.70}, 0.5));
    broker.tick(at + std::chrono::milliseconds(1));
    CHECK(decision == CompositingDecision::pristine_low_confidence);

    at += std::chrono::milliseconds(10);
    simulation->emit_frame({33, epoch, at, {1920, 1080}, ColorSpace::sdr_srgb, 33, false, false});
    broker.submit_occlusion_evidence(valid_occlusion(at, 33, epoch));
    broker.submit_patch(valid_patch(at, 33, generation, epoch, 33));
    broker.tick(at + std::chrono::milliseconds(101));
    CHECK(decision == CompositingDecision::pristine_stale_frame);
    CHECK(simulation->counters().patch_presentations == 0);
    CHECK(!simulation->residual_visible());
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
    const auto device_generation = broker.diagnostics().device_generation;

    const auto now = std::chrono::steady_clock::now();
    simulation->emit_frame({7, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 1, false, false});
    broker.submit_occlusion_evidence(valid_occlusion(now, 7, device_generation, true));
    broker.submit_patch(valid_patch(now, 7, broker.diagnostics().cancellation_generation,
                                    device_generation, 88, {0.4, 0.5, 0.6, 0.7}));
    broker.tick(now + std::chrono::milliseconds(1));
    CHECK(decision == CompositingDecision::pristine_occluded);
    CHECK(simulation->counters().pristine_presentations == 1);
    CHECK(simulation->counters().patch_presentations == 0);

    simulation->emit_frame({8, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 2, false, false});
    broker.submit_occlusion_evidence(valid_occlusion(now, 8, device_generation));
    broker.submit_patch(valid_patch(now, 8, broker.diagnostics().cancellation_generation,
                                    device_generation, 89, {0.4, 0.5, 0.6, 0.7}));
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
    const auto device_generation = broker.diagnostics().device_generation;
    const auto old_generation = broker.diagnostics().cancellation_generation;
    broker.cancel_generation(old_generation + 1);

    const auto now = std::chrono::steady_clock::now();
    simulation->emit_frame({9, device_generation, now, {1920, 1080}, ColorSpace::sdr_srgb, 3, false, false});
    broker.submit_occlusion_evidence(valid_occlusion(now, 9, device_generation));
    broker.submit_patch(valid_patch(now, 9, old_generation, device_generation, 90,
                                    {0.4, 0.5, 0.6, 0.7}));
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
    const auto geometry_epoch = broker.diagnostics().geometry_epoch;
    simulation->emit_failure({FailureDomain::device, FailureCode::device_removed, true, "simulated reset"});
    CHECK(broker.diagnostics().state == BrokerState::recovering_device);
    broker.tick(std::chrono::steady_clock::now() + std::chrono::seconds(1));
    CHECK(broker.diagnostics().device_generation == 2);
    CHECK(broker.diagnostics().geometry_epoch > geometry_epoch);
    CHECK(simulation->counters().graphics_recreates == 2);
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

    const SharedTextureDescriptor descriptor{
        .schema_version = 1,
        .session_nonce = nonce,
        .session_id = "session-1234",
        .lease_nonce_high = 0x1111,
        .lease_nonce_low = 0x2222,
        .worker_process_id = 444,
        .worker_process_creation_time = 0x12345678,
        .worker_executable_name = "npc-mouth-worker.exe",
        .source_process_handle_value = 99,
        .adapter_luid = 100,
        .keyed_mutex_acquire_key = 1,
        .keyed_mutex_release_key = 2,
        .width = 256,
        .height = 128,
        .stride_bytes = 1024,
        .dxgi_format = 87,
        .alpha_mode = 1,
        .expires_qpc = 8500,
    };
    const Command patch = SubmitPatchCommand{
        .source_frame_sequence = 12,
        .cancellation_generation = 3,
        .left = 0.4,
        .top = 0.5,
        .right = 0.6,
        .bottom = 0.7,
        .confidence = 0.92,
        .produced_qpc = 8450,
        .shared_texture = descriptor,
        .source_device_generation = 9,
        .source_frame_qpc = 8400,
        .source_geometry_epoch = 3,
        .actor_id = 5,
        .track_id = 6,
        .track_epoch = 4,
    };
    const auto encoded_patch = encode_command(CommandKind::submit_patch, patch);
    const auto decoded_patch = encoded_patch ? decode_command(CommandKind::submit_patch, *encoded_patch) : std::nullopt;
    CHECK(decoded_patch && std::holds_alternative<SubmitPatchCommand>(*decoded_patch));
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).shared_texture->width == 256);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).source_device_generation == 9);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).source_frame_qpc == 8400);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).source_geometry_epoch == 3);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).actor_id == 5);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).track_id == 6);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).track_epoch == 4);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).shared_texture->session_nonce == nonce);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).shared_texture->worker_process_id == 444);
    CHECK(std::get<SubmitPatchCommand>(*decoded_patch).shared_texture->stride_bytes == 1024);

    auto legacy_patch = *encoded_patch;
    legacy_patch.push_back(std::byte{0x62}); // field 12, length-delimited
    legacy_patch.push_back(std::byte{0x01});
    legacy_patch.push_back(std::byte{'x'});
    CHECK(!decode_command(CommandKind::submit_patch, legacy_patch).has_value());

    auto invalid_patch_time_value = std::get<SubmitPatchCommand>(patch);
    invalid_patch_time_value.produced_qpc = 8300;
    const Command invalid_patch_time = invalid_patch_time_value;
    const auto encoded_invalid_patch = encode_command(CommandKind::submit_patch, invalid_patch_time);
    CHECK(encoded_invalid_patch &&
          !decode_command(CommandKind::submit_patch, *encoded_invalid_patch).has_value());

    const Command visual_allocation = AllocateVisualSourceCommand{
        .worker_process_id = 444,
        .worker_process_creation_time = 0x12345678,
        .worker_executable_name = "npc-mouth-worker.exe",
        .actor_id = 5,
        .track_id = 6,
        .track_epoch = 4,
    };
    const auto encoded_visual_allocation =
        encode_command(CommandKind::allocate_visual_source, visual_allocation);
    const auto decoded_visual_allocation = encoded_visual_allocation
        ? decode_command(CommandKind::allocate_visual_source, *encoded_visual_allocation)
        : std::nullopt;
    CHECK(decoded_visual_allocation &&
          std::holds_alternative<AllocateVisualSourceCommand>(*decoded_visual_allocation));
    CHECK(std::get<AllocateVisualSourceCommand>(*decoded_visual_allocation).actor_id == 5U);
    const Command visual_release = ReleaseVisualSourceCommand{
        .worker_process_id = 444,
        .lease_nonce_high = 0x3000,
        .lease_nonce_low = 0x4000,
    };
    const Command identity_allocation = AllocateIdentityFrameCommand{
        .worker_process_id = 445,
        .worker_process_creation_time = 0x22334455,
        .worker_executable_name = "python.exe",
        .crop_px = {32, 48, 288, 304},
    };
    const auto encoded_identity_allocation =
        encode_command(CommandKind::allocate_identity_frame, identity_allocation);
    const auto decoded_identity_allocation = encoded_identity_allocation
        ? decode_command(CommandKind::allocate_identity_frame, *encoded_identity_allocation)
        : std::nullopt;
    CHECK(decoded_identity_allocation &&
          std::holds_alternative<AllocateIdentityFrameCommand>(*decoded_identity_allocation));
    CHECK(decoded_identity_allocation &&
          std::get<AllocateIdentityFrameCommand>(*decoded_identity_allocation).crop_px ==
              RectI{32, 48, 288, 304});
    const Command identity_release = ReleaseIdentityFrameCommand{
        .worker_process_id = 445,
        .lease_id = "00112233445566778899aabbccddeeff",
        .lease_nonce = "ffeeddccbbaa99887766554433221100",
    };
    const Command reference_allocation = AllocateIdentityReferenceImportCommand{
        .worker_process_id = 445,
        .worker_process_creation_time = 0x22334455,
        .worker_executable_name = "python.exe",
        .picker_consent_token = "consent-001",
        .game_profile_id = "game-001",
        .character_id = "mara",
        .subject_id = "mara",
        .reference_id = "reference-001",
        .subject_display_name = "Mara",
        .source_class = IdentityReferenceSourceClass::user_private,
        .owner_user_id = "local-user",
        .original_work_license = "",
        .explicit_user_consent = true,
        .local_only = true,
        .imported_at_unix_ms = 1'700'000'000'000,
    };
    const auto encoded_reference_allocation =
        encode_command(CommandKind::allocate_identity_reference_import, reference_allocation);
    const auto decoded_reference_allocation = encoded_reference_allocation
        ? decode_command(CommandKind::allocate_identity_reference_import,
                         *encoded_reference_allocation)
        : std::nullopt;
    CHECK(decoded_reference_allocation &&
          std::holds_alternative<AllocateIdentityReferenceImportCommand>(
              *decoded_reference_allocation));
    CHECK(decoded_reference_allocation &&
          std::get<AllocateIdentityReferenceImportCommand>(*decoded_reference_allocation)
                  .subject_id == "mara");
    auto original_rights =
        std::get<AllocateIdentityReferenceImportCommand>(reference_allocation);
    original_rights.source_class = IdentityReferenceSourceClass::original_synthetic;
    original_rights.owner_user_id.clear();
    original_rights.original_work_license = "CC0-1.0";
    original_rights.explicit_user_consent = false;
    const auto encoded_original_reference = encode_command(
        CommandKind::allocate_identity_reference_import, Command{original_rights});
    const auto decoded_original_reference = encoded_original_reference
        ? decode_command(CommandKind::allocate_identity_reference_import,
                         *encoded_original_reference)
        : std::nullopt;
    CHECK(decoded_original_reference &&
          std::get<AllocateIdentityReferenceImportCommand>(*decoded_original_reference)
                  .source_class == IdentityReferenceSourceClass::original_synthetic);
    auto mixed_rights = std::get<AllocateIdentityReferenceImportCommand>(reference_allocation);
    mixed_rights.original_work_license = "Apache-2.0";
    CHECK(!encode_command(CommandKind::allocate_identity_reference_import,
                          Command{mixed_rights})
               .has_value());
    auto cross_character = std::get<AllocateIdentityReferenceImportCommand>(reference_allocation);
    cross_character.subject_id = "different-character";
    CHECK(!encode_command(CommandKind::allocate_identity_reference_import,
                          Command{cross_character})
               .has_value());
    const Command reference_release = ReleaseIdentityReferenceImportCommand{
        .worker_process_id = 445,
        .lease_id = "00112233445566778899aabbccddeeff",
        .lease_nonce = "ffeeddccbbaa99887766554433221100",
    };

    const IdentityFrameLease identity_lease{
        .schema_version = 1,
        .worker_process_id = 445,
        .worker_process_creation_time = 0x22334455,
        .worker_executable_name = "python.exe",
        .lease_id = "00112233445566778899aabbccddeeff",
        .shared_memory_name = "Local\\npc.identity.0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        .lease_nonce = "ffeeddccbbaa99887766554433221100",
        .byte_length = 256U * 256U * 4U,
        .width = 256,
        .height = 256,
        .stride_bytes = 1024,
        .pixel_format = "b8g8r8a8_unorm",
        .content_sha256 = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        .expires_qpc = 9'000,
        .qpc_frequency = 10'000'000,
        .cancellation_generation = 3,
        .capture_session_id = "identity-session",
        .selected_process_id = 123,
        .selected_window_handle = 0x9988,
        .selected_executable_name = "game.exe",
        .source_device_generation = 9,
        .source_geometry_epoch = 7,
        .source_frame_sequence = 12,
        .source_frame_qpc = 8'400,
        .captured_at_unix_ms = 1'700'000'000'000,
        .advancing_frame_verified = true,
        .overlay_capture_excluded = true,
        .protected_online_detected = false,
        .anti_cheat_detected = false,
        .crop_px = {32, 48, 288, 304},
        .source_size_px = {1280, 720},
    };
    const auto encoded_identity_lease = encode_identity_frame_lease(identity_lease);
    const auto decoded_identity_lease = encoded_identity_lease
        ? decode_identity_frame_lease(*encoded_identity_lease)
        : std::nullopt;
    CHECK(decoded_identity_lease.has_value());
    CHECK(decoded_identity_lease && decoded_identity_lease->worker_process_id == 445U);
    CHECK(decoded_identity_lease && decoded_identity_lease->shared_memory_name ==
          identity_lease.shared_memory_name);
    CHECK(decoded_identity_lease && decoded_identity_lease->content_sha256 ==
          identity_lease.content_sha256);
    CHECK(decoded_identity_lease && decoded_identity_lease->crop_px == identity_lease.crop_px);
    CHECK(decoded_identity_lease &&
          decoded_identity_lease->source_frame_sequence == identity_lease.source_frame_sequence);
    auto unsafe_identity_lease = identity_lease;
    unsafe_identity_lease.overlay_capture_excluded = false;
    CHECK(!encode_identity_frame_lease(unsafe_identity_lease).has_value());

    const IdentityReferenceImportLease reference_lease{
        .schema_version = 1,
        .worker_process_id = 445,
        .worker_process_creation_time = 0x22334455,
        .worker_executable_name = "python.exe",
        .lease_id = "00112233445566778899aabbccddeeff",
        .shared_memory_name = "Local\\npc.identity.0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        .lease_nonce = "ffeeddccbbaa99887766554433221100",
        .byte_length = 4U * 4U * 4U,
        .width = 4,
        .height = 4,
        .stride_bytes = 16,
        .pixel_format = "b8g8r8a8_unorm",
        .content_sha256 = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        .source_asset_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        .source_media_type = "image/png",
        .expires_qpc = 9'000,
        .qpc_frequency = 10'000'000,
        .cancellation_generation = 3,
        .capture_session_id = "identity-session",
        .selected_process_id = 123,
        .selected_window_handle = 0x9988,
        .selected_executable_name = "game.exe",
        .source_device_generation = 9,
        .source_geometry_epoch = 7,
        .picker_consent_token = "consent-001",
        .game_profile_id = "game-001",
        .character_id = "mara",
        .subject_id = "mara",
        .reference_id = "reference-001",
        .subject_display_name = "Mara",
        .source_class = IdentityReferenceSourceClass::user_private,
        .owner_user_id = "local-user",
        .original_work_license = "",
        .explicit_user_consent = true,
        .local_only = true,
        .imported_at_unix_ms = 1'700'000'000'000,
    };
    const auto encoded_reference_lease =
        encode_identity_reference_import_lease(reference_lease);
    const auto decoded_reference_lease = encoded_reference_lease
        ? decode_identity_reference_import_lease(*encoded_reference_lease)
        : std::nullopt;
    CHECK(decoded_reference_lease.has_value());
    CHECK(decoded_reference_lease &&
          decoded_reference_lease->source_asset_sha256 ==
              reference_lease.source_asset_sha256);
    CHECK(decoded_reference_lease &&
          decoded_reference_lease->picker_consent_token ==
              reference_lease.picker_consent_token);
    CHECK(decoded_reference_lease &&
          decoded_reference_lease->subject_id == decoded_reference_lease->character_id);
    auto wrong_reference_rights = reference_lease;
    wrong_reference_rights.original_work_license = "Apache-2.0";
    CHECK(!encode_identity_reference_import_lease(wrong_reference_rights).has_value());

    const VisualSourceLease visual_lease{
        .schema_version = 1,
        .broker_process_id = 100,
        .broker_process_creation_time = 0x1000,
        .broker_executable_name = "npc-media-broker.exe",
        .worker_process_id = 444,
        .worker_process_creation_time = 0x12345678,
        .worker_executable_name = "npc-mouth-worker.exe",
        .worker_handle_value = 0x2000,
        .lease_nonce_high = 0x3000,
        .lease_nonce_low = 0x4000,
        .adapter_luid = 0x5000,
        .keyed_mutex_acquire_key = 1,
        .keyed_mutex_release_key = 2,
        .width = 1280,
        .height = 720,
        .stride_bytes = 5120,
        .dxgi_format = 87,
        .alpha_mode = 1,
        .expires_qpc = 9000,
        .qpc_frequency = 10'000'000,
        .cancellation_generation = 3,
        .source_device_generation = 0,
        .source_geometry_epoch = 7,
        .source_frame_sequence = 12,
        .source_frame_qpc = 8400,
        .actor_id = 5,
        .track_id = 6,
        .track_epoch = 4,
    };
    const auto encoded_visual_lease = encode_visual_source_lease(visual_lease);
    const auto decoded_visual_lease = encoded_visual_lease
        ? decode_visual_source_lease(*encoded_visual_lease) : std::nullopt;
    CHECK(decoded_visual_lease && decoded_visual_lease->worker_handle_value == 0x2000U);
    CHECK(decoded_visual_lease && decoded_visual_lease->source_frame_qpc == 8400U);
    CHECK(decoded_visual_lease && decoded_visual_lease->source_device_generation == 0U);
    CHECK(decoded_visual_lease && decoded_visual_lease->actor_id == 5U &&
          decoded_visual_lease->track_id == 6U && decoded_visual_lease->track_epoch == 4U);

    const std::vector<std::pair<CommandKind, Command>> typed_commands{
        {CommandKind::health, HealthCommand{}},
        {CommandKind::clear_target, ClearTargetCommand{}},
        {CommandKind::configure_ptt, ConfigurePttCommand{0x77}},
        {CommandKind::audio_status, AudioStatusCommand{}},
        {CommandKind::submit_occlusion, SubmitOcclusionCommand{
             .face_confidence = 0.9,
             .landmark_confidence = 0.91,
             .visibility_ratio = 0.92,
             .mouth_occluded = false,
             .measured_qpc = 8000,
             .source_frame_sequence = 11,
             .source_device_generation = 9,
             .source_geometry_epoch = 3,
             .source_frame_qpc = 7950,
             .actor_id = 5,
             .track_id = 6,
             .track_epoch = 4}},
        {CommandKind::cancel, CancelCommand{4}},
        {CommandKind::diagnostics, DiagnosticsCommand{}},
        {CommandKind::capture_evidence, CaptureEvidenceCommand{}},
        {CommandKind::allocate_visual_source, visual_allocation},
        {CommandKind::release_visual_source, visual_release},
        {CommandKind::allocate_identity_frame, identity_allocation},
        {CommandKind::release_identity_frame, identity_release},
        {CommandKind::allocate_identity_reference_import, reference_allocation},
        {CommandKind::release_identity_reference_import, reference_release},
        {CommandKind::shutdown, ShutdownCommand{}},
    };
    for (const auto& [kind, typed] : typed_commands) {
        const auto encoded = encode_command(kind, typed);
        CHECK(encoded && decode_command(kind, *encoded).has_value());
    }
}

void test_residual_contract_rejects_every_untrusted_or_stale_dimension() {
    using namespace npc::media::ipc;
    std::array<std::byte, launch_nonce_bytes> nonce{};
    nonce[0] = std::byte{0xa5};
    const SharedTextureDescriptor texture{
        .schema_version = 1,
        .session_nonce = nonce,
        .session_id = "residual-session",
        .lease_nonce_high = 0x1111,
        .lease_nonce_low = 0x2222,
        .worker_process_id = 444,
        .worker_process_creation_time = 0x12345678,
        .worker_executable_name = "npc-mouth-worker.exe",
        .source_process_handle_value = 0x1234,
        .adapter_luid = 0x5678,
        .keyed_mutex_acquire_key = 1,
        .keyed_mutex_release_key = 2,
        .width = 256,
        .height = 128,
        .stride_bytes = 1024,
        .dxgi_format = 87,
        .alpha_mode = 1,
        .expires_qpc = 8500,
    };
    const SubmitPatchCommand command{
        .source_frame_sequence = 12,
        .cancellation_generation = 3,
        .left = 0.4,
        .top = 0.5,
        .right = 0.6,
        .bottom = 0.7,
        .confidence = 0.92,
        .produced_qpc = 8450,
        .shared_texture = texture,
        .source_device_generation = 9,
        .source_frame_qpc = 8400,
        .source_geometry_epoch = 7,
        .actor_id = 5,
        .track_id = 6,
        .track_epoch = 4,
    };
    const ResidualValidationContext context{
        .nonce = nonce,
        .session_id = "residual-session",
        .broker_process_id = 100,
        .selected_target_process_id = 42,
        .cancellation_generation = 3,
        .device_generation = 9,
        .geometry_epoch = 7,
        .latest_frame_sequence = 12,
        .latest_frame_qpc = 8400,
        .latest_frame_size_px = {1280, 640},
        .now_qpc = 8460,
        .qpc_frequency = 1000,
        .capture_backend = CaptureBackend::windows_graphics_capture,
        .overlay_visuals_allowed = true,
    };
    CHECK(validate_residual_contract(command, context) == ResidualContractStatus::accepted);

    auto bounded_recent = command;
    bounded_recent.source_frame_sequence = 11;
    bounded_recent.source_frame_qpc = 8330;
    bounded_recent.produced_qpc = 8450;
    CHECK(validate_residual_contract(bounded_recent, context) ==
          ResidualContractStatus::accepted);

    auto stale = bounded_recent;
    stale.source_frame_qpc = 7999;
    CHECK(validate_residual_contract(stale, context) ==
          ResidualContractStatus::invalid_timing);

    const auto reject_command = [&](const ResidualContractStatus expected, const auto& mutate) {
        auto candidate = command;
        mutate(candidate);
        CHECK(validate_residual_contract(candidate, context) == expected);
    };
    const auto reject_context = [&](const ResidualContractStatus expected, const auto& mutate) {
        auto candidate = context;
        mutate(candidate);
        CHECK(validate_residual_contract(command, candidate) == expected);
    };

    reject_context(ResidualContractStatus::unavailable_capture_path,
                   [](auto& value) { value.capture_backend = CaptureBackend::desktop_duplication; });
    reject_command(ResidualContractStatus::missing_texture,
                   [](auto& value) { value.shared_texture.reset(); });
    reject_command(ResidualContractStatus::session_mismatch,
                   [](auto& value) { value.shared_texture->session_id = "other"; });
    reject_command(ResidualContractStatus::invalid_worker,
                   [](auto& value) { value.shared_texture->worker_process_id = 42; });
    reject_command(ResidualContractStatus::invalid_lease_nonce,
                   [](auto& value) { value.shared_texture->lease_nonce_low = 0; });
    reject_command(ResidualContractStatus::invalid_handle,
                   [](auto& value) { value.shared_texture->source_process_handle_value = 0; });
    reject_command(ResidualContractStatus::invalid_adapter,
                   [](auto& value) { value.shared_texture->adapter_luid = 0; });
    reject_command(ResidualContractStatus::invalid_mutex_keys,
                   [](auto& value) { value.shared_texture->keyed_mutex_release_key = 1; });
    reject_command(ResidualContractStatus::invalid_format,
                   [](auto& value) { value.shared_texture->alpha_mode = 0; });
    reject_command(ResidualContractStatus::invalid_extent,
                   [](auto& value) { value.shared_texture->width = 255; });
    reject_command(ResidualContractStatus::wrong_cancellation_generation,
                   [](auto& value) { ++value.cancellation_generation; });
    reject_command(ResidualContractStatus::wrong_device_generation,
                   [](auto& value) { ++value.source_device_generation; });
    reject_command(ResidualContractStatus::wrong_geometry_epoch,
                   [](auto& value) { ++value.source_geometry_epoch; });
    reject_command(ResidualContractStatus::wrong_frame,
                   [](auto& value) { ++value.source_frame_sequence; });
    reject_command(ResidualContractStatus::wrong_frame_qpc,
                   [](auto& value) { ++value.source_frame_qpc; });
    reject_command(ResidualContractStatus::invalid_track,
                   [](auto& value) { value.actor_id = 0; });
    reject_command(ResidualContractStatus::invalid_bounds,
                   [](auto& value) { value.left = 0.0; value.right = 0.9; });
    reject_command(ResidualContractStatus::invalid_confidence,
                   [](auto& value) { value.confidence = 0.81; });
    reject_command(ResidualContractStatus::invalid_timing,
                   [](auto& value) { value.shared_texture->expires_qpc = 8459; });
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
        CommandKind::capture_evidence,
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

void test_manual_actor_picker_command31_is_native_typed_and_privacy_safe() {
    using namespace npc::media::ipc;
    const ManualActorPickerCommand begin{
        .action = ManualActorPickerAction::begin,
        .request_id = "manual-actor-001",
        .source_device_generation = 9,
        .source_geometry_epoch = 7,
        .source_frame_sequence = 123,
        .source_frame_qpc = 456'000,
        .timeout_ms = 5'000,
        .candidates = {
            {11, 21, 31, {0.10, 0.20, 0.30, 0.60}},
            {12, 22, 32, {0.55, 0.18, 0.82, 0.64}},
        },
    };
    const auto encoded = encode_command(CommandKind::manual_actor_picker, Command{begin});
    const auto decoded = encoded
        ? decode_command(CommandKind::manual_actor_picker, *encoded) : std::nullopt;
    CHECK(decoded && std::holds_alternative<ManualActorPickerCommand>(*decoded));
    CHECK(decoded && std::get<ManualActorPickerCommand>(*decoded).request_id == begin.request_id);
    CHECK(decoded && std::get<ManualActorPickerCommand>(*decoded).candidates.size() == 2U);
    CHECK(decoded && std::get<ManualActorPickerCommand>(*decoded).candidates[1].actor_id == 12U);

    const ManualActorPickerCommand poll{.action = ManualActorPickerAction::poll,
                                        .request_id = begin.request_id};
    const ManualActorPickerCommand cancel{.action = ManualActorPickerAction::cancel,
                                          .request_id = begin.request_id};
    for (const auto& command : {poll, cancel}) {
        const auto wire = encode_command(CommandKind::manual_actor_picker, Command{command});
        CHECK(wire && decode_command(CommandKind::manual_actor_picker, *wire).has_value());
    }
    auto polluted_poll = poll;
    polluted_poll.source_frame_sequence = 1U;
    CHECK(!encode_command(CommandKind::manual_actor_picker, Command{polluted_poll}));
    auto empty = begin;
    empty.candidates.clear();
    CHECK(!encode_command(CommandKind::manual_actor_picker, Command{empty}));
    auto duplicate_actor = begin;
    duplicate_actor.candidates[1].actor_id = duplicate_actor.candidates[0].actor_id;
    CHECK(!encode_command(CommandKind::manual_actor_picker, Command{duplicate_actor}));
    auto invalid_roi = begin;
    invalid_roi.candidates[0].normalized_bounds.right = 1.1;
    CHECK(!encode_command(CommandKind::manual_actor_picker, Command{invalid_roi}));

    std::array<std::byte, launch_nonce_bytes> nonce{};
    nonce[0] = std::byte{0x31};
    const Envelope envelope{protocol_version, nonce, "session-actor", 1U, 10'000U, 3U,
                            CommandKind::manual_actor_picker, *encoded};
    const auto envelope_wire = encode_envelope(envelope);
    const auto envelope_round_trip = envelope_wire ? decode_envelope(*envelope_wire) : std::nullopt;
    CHECK(envelope_round_trip &&
          envelope_round_trip->command == CommandKind::manual_actor_picker);
    CHECK(command_id_registry.back() == CommandKind::manual_actor_picker);
    CHECK(command_ids_are_unique());
    CHECK(target_process_effect(CommandKind::manual_actor_picker) == TargetProcessEffect::none);

    ManualActorPickerReceipt pending{
        .schema_version = 1,
        .request_id = begin.request_id,
        .status = ManualActorPickerStatus::pending,
        .receipt_nonce_high = 0x1111,
        .receipt_nonce_low = 0x2222,
        .capture_session_id = "session-actor",
        .cancellation_generation = 3,
        .selected_process_id = 42,
        .selected_window_handle = 0x1234,
        .selected_executable_name = "synthetic-game.exe",
        .source_device_generation = begin.source_device_generation,
        .source_geometry_epoch = begin.source_geometry_epoch,
        .source_frame_sequence = begin.source_frame_sequence,
        .source_frame_qpc = begin.source_frame_qpc,
        .candidate_count = 2,
        .candidate_set_sha256 = std::string(64, 'a'),
        .began_qpc = 500'000,
        .attested_at_qpc = 500'000,
        .qpc_frequency = 10'000'000,
        .frozen_wgc_frame_verified = true,
        .overlay_capture_excluded = true,
        .overlay_nonactivating = true,
        .pixels_withheld_from_webview = true,
        .coordinates_withheld_from_webview = true,
    };
    const auto pending_wire = encode_manual_actor_picker_receipt(pending);
    const auto pending_round_trip = pending_wire
        ? decode_manual_actor_picker_receipt(*pending_wire) : std::nullopt;
    CHECK(pending_round_trip && pending_round_trip->status == ManualActorPickerStatus::pending);
    CHECK(pending_round_trip && pending_round_trip->selected_actor_id == 0U &&
          pending_round_trip->clicked_qpc == 0U);

    auto selected = pending;
    selected.status = ManualActorPickerStatus::selected;
    selected.selected_actor_id = 12;
    selected.selected_track_id = 22;
    selected.selected_track_epoch = 32;
    selected.clicked_qpc = 510'000;
    selected.attested_at_qpc = 510'100;
    selected.pointer_kind = ManualActorPointerKind::mouse;
    selected.single_hardware_pointer_click = true;
    const auto selected_wire = encode_manual_actor_picker_receipt(selected);
    const auto selected_round_trip = selected_wire
        ? decode_manual_actor_picker_receipt(*selected_wire) : std::nullopt;
    CHECK(selected_round_trip && selected_round_trip->selected_actor_id == 12U &&
          selected_round_trip->selected_track_id == 22U &&
          selected_round_trip->selected_track_epoch == 32U);
    CHECK(selected_round_trip && selected_round_trip->pixels_withheld_from_webview &&
          selected_round_trip->coordinates_withheld_from_webview);

    auto click_outside = pending;
    click_outside.status = ManualActorPickerStatus::click_outside_detected_roi;
    click_outside.clicked_qpc = 510'000;
    click_outside.attested_at_qpc = 510'100;
    click_outside.pointer_kind = ManualActorPointerKind::mouse;
    CHECK(encode_manual_actor_picker_receipt(click_outside).has_value());
    auto cancelled_with_rejected_click = pending;
    cancelled_with_rejected_click.status = ManualActorPickerStatus::cancelled;
    cancelled_with_rejected_click.clicked_qpc = 510'000;
    cancelled_with_rejected_click.attested_at_qpc = 510'100;
    cancelled_with_rejected_click.pointer_kind = ManualActorPointerKind::mouse;
    CHECK(!encode_manual_actor_picker_receipt(cancelled_with_rejected_click));
    auto selected_without_hardware_up = selected;
    selected_without_hardware_up.single_hardware_pointer_click = false;
    CHECK(!encode_manual_actor_picker_receipt(selected_without_hardware_up));
    auto leaked_coordinates = selected;
    leaked_coordinates.coordinates_withheld_from_webview = false;
    CHECK(!encode_manual_actor_picker_receipt(leaked_coordinates));
    auto unbound_selection = pending;
    unbound_selection.selected_actor_id = 12;
    CHECK(!encode_manual_actor_picker_receipt(unbound_selection));
    auto invalid_hash = pending;
    invalid_hash.candidate_set_sha256 = "not-a-hash";
    CHECK(!encode_manual_actor_picker_receipt(invalid_hash));
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
    evidence = safe_evidence();
    CHECK(evaluate_target_policy(evidence, {"SKYRIMSE.EXE"}).capture_allowed);
    CHECK(evaluate_target_policy(evidence, {"C:\\Games\\skyrimse.exe"}).reason ==
          TargetBlockReason::not_allowlisted);
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
    test_initial_capture_binds_nonzero_device_generation();
    test_capture_evidence_requires_advancing_sequence_qpc_and_geometry_epoch();
    test_geometry_epoch_invalidates_old_frames_and_tracks_minimize_restore();
    test_protected_exclusive_and_unsupported_paths_degrade_truthfully();
    test_latest_frame_and_high_confidence_patch_are_presented();
    test_residual_guard_rejects_unsafe_async_output_and_clears_prior_patch();
    test_residual_safety_ceiling_cannot_be_disabled_by_configuration();
    test_occlusion_and_staleness_fail_open_to_pristine_game();
    test_late_generation_is_never_composited();
    test_device_loss_recreates_generation_and_capture();
    test_ptt_press_and_release_are_forwarded();
    test_ipc_codec_is_deterministic_and_typed();
    test_residual_contract_rejects_every_untrusted_or_stale_dimension();
    test_command_surface_cannot_modify_or_extend_the_game_process();
    test_ipc_auth_sequence_deadline_and_cancellation_validation();
    test_service_launch_arguments_fail_closed();
    test_manual_actor_picker_command31_is_native_typed_and_privacy_safe();
    test_target_policy_blocks_untrusted_and_ambiguous_targets();

    if (failures != 0) {
        std::cerr << failures << " media-broker test(s) failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "all media-broker state, geometry, recovery, IPC, target-policy, and fail-open tests passed\n";
    return EXIT_SUCCESS;
}
