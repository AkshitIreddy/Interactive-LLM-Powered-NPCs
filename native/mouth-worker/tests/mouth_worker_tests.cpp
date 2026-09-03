#include "npc/mouth_worker/compositor.hpp"
#include "npc/mouth_worker/worker.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <iostream>
#include <limits>
#include <string_view>
#include <utility>
#include <vector>

namespace {

using namespace npc::mouth;

int failures = 0;

void expect(const bool condition, const std::string_view message) {
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        ++failures;
    }
}

[[nodiscard]] std::uint64_t digest(const std::vector<std::uint8_t>& bytes) {
    std::uint64_t value = 1469598103934665603ULL;
    for (const auto byte : bytes) {
        value ^= byte;
        value *= 1099511628211ULL;
    }
    return value;
}

[[nodiscard]] CpuFrame make_frame(const std::uint64_t sequence,
                                  const Nanoseconds captured_at_ns,
                                  const std::uint32_t width = 320U,
                                  const std::uint32_t height = 180U) {
    CpuFrame frame{};
    frame.lease.schema_version = 1U;
    frame.lease.transport = LeaseTransport::cpu_reference;
    frame.lease.lease_nonce_high = 0x1122334455667788ULL;
    frame.lease.lease_nonce_low = sequence;
    frame.lease.owner_process_id = 100U;
    frame.lease.intended_consumer_process_id = 200U;
    frame.lease.width = width;
    frame.lease.height = height;
    frame.lease.stride_bytes = width * 4U;
    frame.lease.expires_at_ns = captured_at_ns + 100'000'000;
    frame.identity = {sequence, 7U, 11U, captured_at_ns};
    frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) * height);
    for (std::uint32_t y = 0; y < height; ++y) {
        for (std::uint32_t x = 0; x < width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * frame.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            frame.bgra[offset + 0U] = static_cast<std::uint8_t>((x * 3U + y) % 256U);
            frame.bgra[offset + 1U] = static_cast<std::uint8_t>((x + y * 2U) % 256U);
            frame.bgra[offset + 2U] = static_cast<std::uint8_t>((x * 2U + y * 3U) % 256U);
            frame.bgra[offset + 3U] = 255U;
        }
    }
    return frame;
}

[[nodiscard]] WorkItem make_item(const std::uint64_t sequence = 41U,
                                 const Nanoseconds captured_at_ns = 1'000'000'000,
                                 const std::uint64_t generation = 3U) {
    WorkItem item{};
    item.track = {generation, 71U, 101U, 5U};
    item.source = make_frame(sequence, captured_at_ns);
    item.tracking.track = item.track;
    item.tracking.frame = item.source.identity;
    item.tracking.face_bounds = {0.31, 0.2, 0.38, 0.66};
    item.tracking.mouth_bounds = {0.43, 0.58, 0.14, 0.12};
    item.tracking.mouth_landmarks.schema_version = 1U;
    item.tracking.mouth_landmarks.provider_instance_id = 0x50524f5649444552ULL;
    item.tracking.mouth_landmarks.left_corner = {0.442, 0.64, 0.96};
    item.tracking.mouth_landmarks.right_corner = {0.558, 0.654, 0.96};
    item.tracking.mouth_landmarks.upper_lip_center = {0.5, 0.616, 0.95};
    item.tracking.mouth_landmarks.lower_lip_center = {0.5, 0.676, 0.95};
    item.tracking.pose = {4.0, -2.0, 7.0};
    item.tracking.face_confidence = 0.96;
    item.tracking.landmark_confidence = 0.95;
    item.tracking.visibility_ratio = 0.93;
    item.tracking.measured_at_ns = captured_at_ns + 2'000'000;
    item.drive.kind = DriveKind::timed_viseme;
    item.drive.clock.stream_generation = generation;
    item.drive.clock.segment_id = 19U;
    item.drive.clock.sample_rate = 48'000U;
    item.drive.clock.channels = 1U;
    item.drive.clock.playback_at_ns = captured_at_ns;
    item.drive.viseme = Viseme::open_vowel;
    item.drive.viseme_strength = 0.9;
    item.deadline_ns = captured_at_ns + 50'000'000;
    return item;
}

[[nodiscard]] CanonicalMouthPatch make_atlas_patch(const std::uint8_t blue,
                                                   const std::uint8_t green,
                                                   const std::uint8_t red) {
    CanonicalMouthPatch patch{};
    patch.width = 48U;
    patch.height = 32U;
    patch.stride_bytes = patch.width * 4U;
    patch.premultiplied_bgra.resize(
        static_cast<std::size_t>(patch.stride_bytes) * patch.height, 0U);
    for (std::uint32_t y = 0; y < patch.height; ++y) {
        for (std::uint32_t x = 0; x < patch.width; ++x) {
            const double nx = (static_cast<double>(x) + 0.5) /
                                  static_cast<double>(patch.width) * 2.0 - 1.0;
            const double ny = (static_cast<double>(y) + 0.5) /
                                  static_cast<double>(patch.height) * 2.0 - 1.0;
            const double radius = std::sqrt(nx * nx + ny * ny);
            const auto alpha = static_cast<std::uint8_t>(std::lround(
                std::clamp((1.0 - radius) / 0.25, 0.0, 1.0) * 255.0));
            const auto offset = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            patch.premultiplied_bgra[offset + 0U] = static_cast<std::uint8_t>(
                (static_cast<std::uint32_t>(blue) * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 1U] = static_cast<std::uint8_t>(
                (static_cast<std::uint32_t>(green) * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 2U] = static_cast<std::uint8_t>(
                (static_cast<std::uint32_t>(red) * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 3U] = alpha;
        }
    }
    return patch;
}

void test_viseme_and_audio_drives() {
    const auto silence = coefficients_for_viseme(Viseme::silence);
    const auto open = coefficients_for_viseme(Viseme::open_vowel, 1.0);
    expect(silence.lip_close == 1.0 && silence.jaw_open == 0.0,
           "silence closes the reference mouth");
    expect(open.jaw_open > 0.8 && open.lower_lip_depress > 0.5,
           "open vowel maps to bounded opening coefficients");

    std::vector<float> quiet(480U, 0.0F);
    const auto quiet_coefficients = coefficients_from_pcm(quiet, 48'000U, 1U);
    expect(quiet_coefficients.lip_close > 0.99 && quiet_coefficients.jaw_open == 0.0,
           "silent PCM fails naturally to a closed mouth");

    std::vector<float> voiced(480U);
    for (std::size_t index = 0; index < voiced.size(); ++index) {
        voiced[index] = static_cast<float>(0.24 * std::sin(
            static_cast<double>(index) * 2.0 * 3.14159265358979323846 * 220.0 / 48'000.0));
    }
    const auto voiced_coefficients = coefficients_from_pcm(voiced, 48'000U, 1U);
    expect(voiced_coefficients.jaw_open > 0.5 && voiced_coefficients.lip_close < 0.2,
           "voiced PCM drives a real causal mouth opening");

    std::vector<float> normalized_speech(1'600U);
    for (std::size_t index = 0; index < normalized_speech.size(); ++index) {
        normalized_speech[index] = static_cast<float>(0.04 * std::sin(
            static_cast<double>(index) * 2.0 * 3.14159265358979323846 * 180.0 / 48'000.0));
    }
    const auto normalized_speech_coefficients =
        coefficients_from_pcm(normalized_speech, 48'000U, 1U);
    expect(normalized_speech_coefficients.jaw_open > 0.45 &&
               normalized_speech_coefficients.lip_close < 0.4,
           "normally normalized hosted speech remains visibly expressive");
}

void test_current_frame_residual_is_bounded_and_premultiplied() {
    auto item = make_item();
    const auto source_before = item.source.bgra;
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.submit(item), "valid item accepted");
    const auto result = worker.process_latest(item.source.identity,
                                              item.source.identity.captured_at_ns + 10'000'000);
    expect(result.has_residual(), "valid exact-frame work produces a residual");
    expect(item.source.bgra == source_before, "source frame remains immutable");
    expect(result.residual.source_frame == item.source.identity,
           "residual preserves exact source identity");
    expect(result.residual.track == item.track, "residual preserves exact actor/track epoch");
    expect(result.residual.residual_lease.lease_nonce_low !=
               result.residual.source_lease.lease_nonce_low &&
               result.residual.residual_lease.width == result.residual.width &&
               result.residual.residual_lease.height == result.residual.height &&
               result.residual.residual_lease.owner_process_id ==
                   item.source.lease.intended_consumer_process_id,
           "residual has a distinct, reverse-direction protocol lease");
    expect(result.residual.normalized_bounds.width <= 0.45 &&
               result.residual.normalized_bounds.height <= 0.35 &&
               result.residual.normalized_bounds.width * result.residual.normalized_bounds.height <= 0.12,
           "residual is bounded to the hard mouth region ceilings");

    bool saw_transparent = false;
    bool saw_opaque = false;
    for (std::size_t offset = 0; offset < result.residual.premultiplied_bgra.size(); offset += 4U) {
        const auto alpha = result.residual.premultiplied_bgra[offset + 3U];
        saw_transparent = saw_transparent || alpha == 0U;
        saw_opaque = saw_opaque || alpha == 255U;
        expect(result.residual.premultiplied_bgra[offset + 0U] <= alpha &&
                   result.residual.premultiplied_bgra[offset + 1U] <= alpha &&
                   result.residual.premultiplied_bgra[offset + 2U] <= alpha,
               "every residual pixel obeys premultiplied alpha");
    }
    expect(saw_transparent && saw_opaque, "mouth mask has transparent exterior and opaque core");

    const auto composited = composite_over_source(item.source, result.residual);
    expect(composited != item.source.bgra, "reference compositor changes the mouth presentation");
    expect(item.source.bgra == source_before, "compositing helper still leaves source untouched");
    const auto patch_left = static_cast<std::uint32_t>(std::llround(
        result.residual.normalized_bounds.x * item.source.lease.width));
    const auto patch_top = static_cast<std::uint32_t>(std::llround(
        result.residual.normalized_bounds.y * item.source.lease.height));
    std::size_t changed_inside = 0U;
    for (std::uint32_t y = 0; y < item.source.lease.height; ++y) {
        for (std::uint32_t x = 0; x < item.source.lease.width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const bool inside = x >= patch_left && y >= patch_top &&
                                x < patch_left + result.residual.width &&
                                y < patch_top + result.residual.height;
            const bool changed = !std::equal(composited.begin() + static_cast<std::ptrdiff_t>(offset),
                                             composited.begin() + static_cast<std::ptrdiff_t>(offset + 4U),
                                             item.source.bgra.begin() +
                                                 static_cast<std::ptrdiff_t>(offset));
            expect(inside || !changed, "pixels outside the mouth rectangle remain byte-identical");
            changed_inside += inside && changed ? 1U : 0U;
        }
    }
    expect(changed_inside > 0U, "at least one pixel changes inside the mouth rectangle");
}

void test_atlas_residual_interpolates_and_binds_to_current_frame() {
    const auto item = make_item();
    const auto closed = make_atlas_patch(30U, 50U, 90U);
    const auto open = make_atlas_patch(80U, 140U, 210U);
    const auto closed_result = compose_atlas_residual(
        item.source, item.track, item.tracking, closed, open, 0.0,
        item.source.identity.captured_at_ns + 4'000'000);
    const auto mixed_result = compose_atlas_residual(
        item.source, item.track, item.tracking, closed, open, 0.5,
        item.source.identity.captured_at_ns + 4'000'000);
    const auto open_result = compose_atlas_residual(
        item.source, item.track, item.tracking, closed, open, 1.0,
        item.source.identity.captured_at_ns + 4'000'000);
    expect(!closed_result.premultiplied_bgra.empty() &&
               !mixed_result.premultiplied_bgra.empty() &&
               !open_result.premultiplied_bgra.empty(),
           "valid atlas states produce current-frame residuals");
    expect(closed_result.source_frame == item.source.identity &&
               closed_result.track == item.track &&
               closed_result.normalized_bounds.x <= item.tracking.mouth_bounds.x &&
               closed_result.normalized_bounds.right() >= item.tracking.mouth_bounds.right() &&
               closed_result.normalized_bounds.y <= item.tracking.mouth_bounds.y &&
               closed_result.normalized_bounds.bottom() >= item.tracking.mouth_bounds.bottom(),
           "atlas residual retains exact frame, track, and bounded mouth geometry");
    expect(digest(closed_result.premultiplied_bgra) != digest(mixed_result.premultiplied_bgra) &&
               digest(mixed_result.premultiplied_bgra) != digest(open_result.premultiplied_bgra),
           "two atlas states interpolate to a distinct intermediate residual");
    for (std::size_t offset = 0; offset < mixed_result.premultiplied_bgra.size(); offset += 4U) {
        const auto alpha = mixed_result.premultiplied_bgra[offset + 3U];
        expect(mixed_result.premultiplied_bgra[offset + 0U] <= alpha &&
                   mixed_result.premultiplied_bgra[offset + 1U] <= alpha &&
                   mixed_result.premultiplied_bgra[offset + 2U] <= alpha,
               "interpolated atlas pixels preserve premultiplied alpha");
    }

    const auto composited = composite_over_source(item.source, mixed_result);
    const auto left = static_cast<std::uint32_t>(std::llround(
        mixed_result.normalized_bounds.x * item.source.lease.width));
    const auto top = static_cast<std::uint32_t>(std::llround(
        mixed_result.normalized_bounds.y * item.source.lease.height));
    for (std::uint32_t y = 0; y < item.source.lease.height; ++y) {
        for (std::uint32_t x = 0; x < item.source.lease.width; ++x) {
            const bool inside = x >= left && y >= top &&
                                x < left + mixed_result.width && y < top + mixed_result.height;
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const bool changed = !std::equal(
                composited.begin() + static_cast<std::ptrdiff_t>(offset),
                composited.begin() + static_cast<std::ptrdiff_t>(offset + 4U),
                item.source.bgra.begin() + static_cast<std::ptrdiff_t>(offset));
            expect(inside || !changed,
                   "atlas compositing leaves every pixel outside the mouth rectangle identical");
        }
    }

    auto malformed = open;
    malformed.premultiplied_bgra[0] = 255U;
    malformed.premultiplied_bgra[3] = 0U;
    expect(compose_atlas_residual(item.source, item.track, item.tracking, closed, malformed,
                                  0.5, item.source.identity.captured_at_ns + 4'000'000)
               .premultiplied_bgra.empty(),
           "atlas compositor rejects non-premultiplied enrollment artifacts");
    expect(compose_atlas_residual(item.source, item.track, item.tracking, closed, open,
                                  1.01, item.source.identity.captured_at_ns + 4'000'000)
               .premultiplied_bgra.empty(),
           "atlas compositor rejects an out-of-range interpolation weight");
}

void test_queue_depth_one() {
    auto older = make_item(80U);
    auto newer = make_item(81U);
    ReferenceMouthWorker worker(older.track.cancellation_generation);
    expect(worker.submit(std::move(older)), "older work accepted");
    expect(worker.submit(newer), "newer work accepted");
    expect(worker.stats().replaced_before_processing == 1U, "queue replaces its only pending slot");
    const auto result = worker.process_latest(newer.source.identity,
                                              newer.source.identity.captured_at_ns + 10'000'000);
    expect(result.has_residual() && result.residual.source_frame.sequence == 81U,
           "only newest frame can produce output");
    expect(!worker.has_pending_work(), "processing drains the queue");
}

void test_exact_binding_and_no_retained_visual() {
    auto item = make_item();
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.submit(item), "binding test item accepted");
    auto advanced = item.source.identity;
    ++advanced.sequence;
    const auto mismatch = worker.process_latest(advanced,
                                                item.source.identity.captured_at_ns + 10'000'000);
    expect(mismatch.disposition == Disposition::bypass_wrong_frame && !mismatch.has_residual(),
           "advanced current frame rejects old residual");
    const auto empty = worker.process_latest(advanced,
                                             item.source.identity.captured_at_ns + 11'000'000);
    expect(empty.disposition == Disposition::bypass_no_work && !empty.has_residual(),
           "rejected work is consumed and never retained");

    item = make_item();
    item.tracking.track.track_epoch += 1U;
    expect(worker.submit(item), "mismatched track item enters queue for authoritative validation");
    const auto wrong_track = worker.process_latest(item.source.identity,
                                                   item.source.identity.captured_at_ns + 10'000'000);
    expect(wrong_track.disposition == Disposition::bypass_wrong_frame,
           "wrong track epoch is rejected with no visual");
}

void test_cancellation_generation() {
    auto item = make_item();
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.submit(item), "pre-cancel work accepted");
    expect(worker.cancel_to(item.track.cancellation_generation + 1U), "generation advances");
    expect(!worker.has_pending_work(), "generation cancellation synchronously clears pending work");
    expect(!worker.submit(item), "old-generation work is refused");
    const auto empty = worker.process_latest(item.source.identity,
                                             item.source.identity.captured_at_ns + 10'000'000);
    expect(empty.disposition == Disposition::bypass_no_work, "cancellation leaves untouched fallback");
}

void test_safety_gates() {
    const auto run = [](WorkItem item, const Nanoseconds now_ns) {
        const auto generation = item.track.cancellation_generation;
        const auto identity = item.source.identity;
        ReferenceMouthWorker worker(generation);
        (void)worker.submit(std::move(item));
        return worker.process_latest(identity, now_ns).disposition;
    };

    auto occluded = make_item();
    occluded.tracking.mouth_occluded = true;
    expect(run(occluded, occluded.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_occluded,
           "mouth occlusion bypasses");

    auto pose = make_item();
    pose.tracking.pose.yaw = 36.0;
    expect(run(pose, pose.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_pose,
           "excessive pose bypasses");

    auto confidence = make_item();
    confidence.tracking.landmark_confidence = 0.81;
    expect(run(confidence, confidence.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_low_confidence,
           "low landmark confidence bypasses");

    auto containment = make_item();
    containment.tracking.mouth_bounds.x = 0.1;
    expect(run(containment, containment.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "mouth outside selected face bypasses");

    auto feathered_edge = make_item();
    feathered_edge.tracking.face_bounds.height = 0.49;
    expect(run(feathered_edge, feathered_edge.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::residual_ready,
           "bounded mask feather may cross the face box when all semantic lip points remain inside");

    auto landmark_outside_face = feathered_edge;
    landmark_outside_face.tracking.mouth_landmarks.lower_lip_center.y = 0.695;
    expect(run(landmark_outside_face,
               landmark_outside_face.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "semantic lip points outside the face remain rejected even inside the padded mask");

    auto landmark_schema = make_item();
    landmark_schema.tracking.mouth_landmarks.schema_version = 2U;
    expect(run(landmark_schema, landmark_schema.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "unknown landmark adapter schema bypasses");

    auto landmark_confidence = make_item();
    landmark_confidence.tracking.mouth_landmarks.left_corner.confidence = 0.4;
    expect(run(landmark_confidence,
               landmark_confidence.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "low-confidence semantic landmark bypasses");

    auto qualified_semantic_confidence = make_item();
    qualified_semantic_confidence.tracking.mouth_landmarks.lower_lip_center.confidence = 0.56;
    expect(run(qualified_semantic_confidence,
               qualified_semantic_confidence.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::residual_ready,
           "visible semantic point is accepted alongside qualified aggregate confidence");

    auto hidden_semantic_point = make_item();
    hidden_semantic_point.tracking.mouth_landmarks.lower_lip_center.confidence = 0.54;
    expect(run(hidden_semantic_point,
               hidden_semantic_point.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "semantic point below the provider visibility floor still bypasses");

    auto huge = make_item();
    huge.tracking.face_bounds = {0.0, 0.0, 1.0, 1.0};
    huge.tracking.mouth_bounds = {0.2, 0.3, 0.5, 0.5};
    expect(run(huge, huge.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_unsafe_bounds,
           "oversized mouth residual bypasses");

    auto stale = make_item();
    expect(run(stale, stale.source.identity.captured_at_ns + 81'000'000) ==
               Disposition::bypass_deadline,
           "expired per-frame deadline bypasses before compositing");

    auto stale_source = make_item();
    stale_source.deadline_ns = stale_source.source.identity.captured_at_ns + 250'000'000;
    stale_source.source.lease.expires_at_ns =
        stale_source.source.identity.captured_at_ns + 250'000'000;
    expect(run(stale_source, stale_source.source.identity.captured_at_ns + 151'000'000) ==
               Disposition::bypass_stale_frame,
           "source age hard limit bypasses even with a later caller deadline");

    auto stale_tracking = make_item();
    stale_tracking.deadline_ns = stale_tracking.source.identity.captured_at_ns + 250'000'000;
    stale_tracking.source.lease.expires_at_ns =
        stale_tracking.source.identity.captured_at_ns + 250'000'000;
    stale_tracking.tracking.measured_at_ns = stale_tracking.source.identity.captured_at_ns - 150'000'001;
    expect(run(stale_tracking,
               stale_tracking.source.identity.captured_at_ns + 1'000'000) ==
               Disposition::bypass_invalid_tracking,
           "stale landmark evidence bypasses independently of source freshness");

    auto expired = make_item();
    expired.source.lease.expires_at_ns = expired.source.identity.captured_at_ns + 5'000'000;
    expect(run(expired, expired.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_expired_lease,
           "expired source lease bypasses");

    auto audio = make_item();
    audio.drive.clock.playback_at_ns += 81'000'000;
    expect(run(audio, audio.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "audio outside the source-frame skew budget bypasses");

    auto malformed_drive = make_item();
    malformed_drive.drive.kind = static_cast<DriveKind>(255U);
    expect(run(malformed_drive,
               malformed_drive.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "unknown drive kind bypasses");

    auto empty_pcm = make_item();
    empty_pcm.drive.kind = DriveKind::pcm_window;
    empty_pcm.drive.interleaved_pcm.clear();
    expect(run(empty_pcm, empty_pcm.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "empty PCM window bypasses");

    auto nan = make_item();
    nan.tracking.face_confidence = std::numeric_limits<double>::quiet_NaN();
    expect(run(nan, nan.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "non-finite tracking evidence bypasses");
}

void test_pcm_path_and_determinism() {
    auto first = make_item();
    first.drive.kind = DriveKind::pcm_window;
    first.drive.interleaved_pcm.resize(480U);
    for (std::size_t index = 0; index < first.drive.interleaved_pcm.size(); ++index) {
        first.drive.interleaved_pcm[index] = static_cast<float>(
            0.2 * std::sin(static_cast<double>(index) * 0.071));
    }
    auto second = first;
    ReferenceMouthWorker first_worker(first.track.cancellation_generation);
    ReferenceMouthWorker second_worker(second.track.cancellation_generation);
    (void)first_worker.submit(first);
    (void)second_worker.submit(second);
    const auto now = first.source.identity.captured_at_ns + 10'000'000;
    const auto first_result = first_worker.process_latest(first.source.identity, now);
    const auto second_result = second_worker.process_latest(second.source.identity, now);
    expect(first_result.has_residual() && second_result.has_residual(),
           "PCM fallback produces real residuals");
    expect(digest(first_result.residual.premultiplied_bgra) ==
               digest(second_result.residual.premultiplied_bgra),
           "reference output is byte-for-byte deterministic");
}

void test_hard_safety_limits_cannot_be_relaxed() {
    auto item = make_item();
    item.tracking.pose.yaw = 36.0;
    WorkerPolicy permissive{};
    permissive.maximum_absolute_yaw_degrees = 180.0;
    permissive.minimum_face_confidence = 0.0;
    permissive.minimum_landmark_confidence = 0.0;
    permissive.minimum_visibility_ratio = 0.0;
    permissive.maximum_mouth_width_fraction = 1.0;
    permissive.maximum_mouth_height_fraction = 1.0;
    permissive.maximum_mouth_area_fraction = 1.0;
    permissive.maximum_source_age_ns = 1'000'000'000;
    permissive.maximum_tracking_age_ns = 1'000'000'000;
    permissive.maximum_audio_skew_ns = 1'000'000'000;
    ReferenceMouthWorker worker(item.track.cancellation_generation, permissive);
    (void)worker.submit(item);
    const auto result = worker.process_latest(item.source.identity,
                                              item.source.identity.captured_at_ns + 10'000'000);
    expect(result.disposition == Disposition::bypass_pose,
           "configuration cannot relax the hard pose ceiling");
}

void test_public_compositor_rejects_malformed_direct_calls() {
    const auto item = make_item();
    auto invalid_tracking = item.tracking;
    invalid_tracking.mouth_bounds.x = -0.1;
    const auto invalid = compose_current_frame_residual(
        item.source, item.track, invalid_tracking,
        coefficients_for_viseme(Viseme::open_vowel), item.source.identity.captured_at_ns + 1);
    expect(invalid.premultiplied_bgra.empty(),
           "direct compositor rejects an out-of-range normalized ROI");

    auto truncated = ResidualPatch{};
    truncated.source_frame = item.source.identity;
    truncated.normalized_bounds = item.tracking.mouth_bounds;
    truncated.width = 20U;
    truncated.height = 10U;
    truncated.stride_bytes = 80U;
    truncated.premultiplied_bgra.resize(4U);
    expect(composite_over_source(item.source, truncated) == item.source.bgra,
           "preview compositor rejects a truncated residual buffer");
}

} // namespace

int main() {
    test_viseme_and_audio_drives();
    test_current_frame_residual_is_bounded_and_premultiplied();
    test_atlas_residual_interpolates_and_binds_to_current_frame();
    test_queue_depth_one();
    test_exact_binding_and_no_retained_visual();
    test_cancellation_generation();
    test_safety_gates();
    test_pcm_path_and_determinism();
    test_hard_safety_limits_cannot_be_relaxed();
    test_public_compositor_rejects_malformed_direct_calls();

    if (failures != 0) {
        std::cerr << failures << " mouth-worker test assertion(s) failed\n";
        return 1;
    }
    std::cout << "PASS: deterministic current-frame mouth worker (all safety gates)\n";
    return 0;
}
