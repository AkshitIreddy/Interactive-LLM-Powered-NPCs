#include "npc/mouth_worker/product_runtime.hpp"

#include <cmath>
#include <cstdint>
#include <iostream>
#include <string_view>

namespace {

using namespace npc::mouth;

int failures = 0;

void expect(const bool condition, const std::string_view message) {
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        ++failures;
    }
}

ProductFrameRequest make_request(const std::uint64_t request_id,
                                 const std::uint64_t sequence,
                                 const Nanoseconds captured_at_ns,
                                 const std::uint64_t generation = 1U) {
    ProductFrameRequest request{};
    request.identity = {request_id, 11U, 12U, 21U, 22U, request_id};
    request.source.identity = {sequence, 3U, 4U, captured_at_ns};
    request.source.lease.schema_version = 1U;
    request.source.lease.transport = LeaseTransport::cpu_reference;
    request.source.lease.lease_nonce_high = 101U + request_id;
    request.source.lease.lease_nonce_low = 201U + request_id;
    request.source.lease.width = 320U;
    request.source.lease.height = 180U;
    request.source.lease.stride_bytes = 320U * 4U;
    request.source.lease.expires_at_ns = captured_at_ns + 75'000'000;
    request.source.bgra.assign(320U * 180U * 4U, 0U);
    for (std::size_t index = 0; index < request.source.bgra.size(); index += 4U) {
        request.source.bgra[index + 0U] = 72U;
        request.source.bgra[index + 1U] = 112U;
        request.source.bgra[index + 2U] = 168U;
        request.source.bgra[index + 3U] = 255U;
    }

    request.landmarks.schema_version = 1U;
    request.landmarks.provider_instance_id = 0x4f53464c4d315631ULL;
    request.landmarks.track = {generation, 41U, 42U, 43U};
    request.landmarks.frame = request.source.identity;
    request.landmarks.source_frame_qpc = 10'000U;
    request.landmarks.qpc_frequency = 10'000'000U;
    request.landmarks.face_bounds = {0.30, 0.16, 0.40, 0.68};
    for (auto& point : request.landmarks.landmarks) {
        point = {0.50, 0.45, 0.96};
    }
    // Exact OpenSeeFace/LS3D-W 66-point mouth contour indices.
    request.landmarks.landmarks[48U] = {0.43, 0.60, 0.97};
    request.landmarks.landmarks[49U] = {0.46, 0.58, 0.97};
    request.landmarks.landmarks[50U] = {0.48, 0.57, 0.97};
    request.landmarks.landmarks[51U] = {0.50, 0.565, 0.97};
    request.landmarks.landmarks[52U] = {0.52, 0.57, 0.97};
    request.landmarks.landmarks[53U] = {0.54, 0.58, 0.97};
    request.landmarks.landmarks[54U] = {0.57, 0.60, 0.97};
    request.landmarks.landmarks[55U] = {0.54, 0.64, 0.97};
    request.landmarks.landmarks[56U] = {0.52, 0.655, 0.97};
    request.landmarks.landmarks[57U] = {0.50, 0.66, 0.97};
    request.landmarks.landmarks[58U] = {0.48, 0.655, 0.97};
    request.landmarks.landmarks[59U] = {0.46, 0.64, 0.97};
    request.landmarks.landmarks[58U] = {0.57, 0.60, 0.96};
    request.landmarks.landmarks[59U] = {0.54, 0.585, 0.96};
    request.landmarks.landmarks[60U] = {0.50, 0.578, 0.96};
    request.landmarks.landmarks[61U] = {0.46, 0.585, 0.96};
    request.landmarks.landmarks[62U] = {0.43, 0.60, 0.96};
    request.landmarks.landmarks[63U] = {0.46, 0.63, 0.96};
    request.landmarks.landmarks[64U] = {0.50, 0.642, 0.96};
    request.landmarks.landmarks[65U] = {0.54, 0.63, 0.96};
    request.landmarks.pose = {2.0, -1.0, 1.0};
    request.landmarks.detector_confidence = 0.96;
    request.landmarks.landmark_confidence = 0.95;
    request.landmarks.visibility_ratio = 0.94;
    request.landmarks.measured_at_ns = captured_at_ns;

    request.appearance.schema_version = 1U;
    request.appearance.runtime_actor_id = 41U;
    request.appearance.descriptor_revision = 7U;
    request.appearance.expected_descriptor_digest_high = 1001U;
    request.appearance.expected_descriptor_digest_low = 1002U;
    request.appearance.observed_descriptor_digest_high = 2001U;
    request.appearance.observed_descriptor_digest_low = 2002U;
    request.appearance.similarity = 0.94;
    request.appearance.temporal_iou = 0.89;
    request.appearance.blocker_coverage = 0.02;
    request.appearance.identity_locked = true;
    request.appearance.target_visible = true;

    request.resources = {1U, VisualPressure::nominal, 15U, true};
    request.drive.kind = DriveKind::timed_viseme;
    request.drive.clock = {generation, request_id, 0U, 800U, 0U,
                           24'000U, 1U, captured_at_ns};
    request.drive.viseme = Viseme::open_vowel;
    request.drive.viseme_strength = 0.75;
    request.deadline_ns = captured_at_ns + 70'000'000;
    return request;
}

[[nodiscard]] CharacterMouthAtlas make_source_only_schema_four_atlas() {
    CharacterMouthAtlas atlas{};
    atlas.schema_version = 4U;
    atlas.cancellation_generation = 1U;
    atlas.actor_id = 41U;
    atlas.identity_revision = 91U;
    const auto state = [](const MouthCoefficients coefficients) {
        MouthAtlasState result{};
        result.coefficients = coefficients;
        result.appearance.width = 128U;
        result.appearance.height = 64U;
        result.appearance.stride_bytes = result.appearance.width * 4U;
        result.appearance.representation =
            MouthPatchRepresentation::normalized_oral_strip_v1;
        result.appearance.reference_context_mean = 1.0;
        result.appearance.premultiplied_bgra.assign(
            static_cast<std::size_t>(result.appearance.stride_bytes) *
                result.appearance.height,
            0U);
        return result;
    };
    MouthCoefficients neutral{};
    neutral.lip_close = 1.0;
    MouthCoefficients rounded{};
    rounded.jaw_open = 0.75;
    rounded.funnel = 1.0;
    MouthCoefficients open{};
    open.jaw_open = 1.0;
    MouthCoefficients spread{};
    spread.jaw_open = 0.575;
    spread.smile_left = spread.smile_right = 1.0;
    atlas.states = {state(neutral), state(rounded), state(open), state(spread)};
    return atlas;
}

void set_stream_sample(ProductFrameRequest& request,
                       const std::uint64_t segment_id,
                       const std::uint64_t playback_sample,
                       const std::uint64_t sample_count = 8'000U) {
    request.drive.clock.segment_id = segment_id;
    request.drive.clock.first_sample_index = 0U;
    request.drive.clock.sample_count = sample_count;
    request.drive.clock.playback_sample_index = playback_sample;
    request.drive.clock.playback_at_ns = request.source.identity.captured_at_ns;
}

[[nodiscard]] PresentationReceiptV1 submit_and_process(
    MouthProductRuntime& runtime, ProductFrameRequest request,
    const Nanoseconds now_ns) {
    const auto frame = request.source.identity;
    const auto queued = runtime.submit(std::move(request), frame, now_ns);
    expect(queued.receipt.disposition == PresentationDisposition::queued,
           "accepted cadence sample reaches the schema-four worker");
    return runtime.process_latest(frame, now_ns + 2'000'000);
}

void shift_mouth_geometry(ProductFrameRequest& request,
                          const double dx,
                          const double dy = 0.0) {
    for (std::size_t index = 48U; index < 66U; ++index) {
        request.landmarks.landmarks[index].x += dx;
        request.landmarks.landmarks[index].y += dy;
    }
}

void set_valid_schema_four_geometry(ProductFrameRequest& request) {
    request.landmarks.face_bounds = {0.05, 0.05, 0.90, 0.90};
    const auto set = [&](const std::size_t index, const double x, const double y) {
        request.landmarks.landmarks[index] = {x / 320.0, y / 180.0, 0.97};
    };
    set(48U, 128.0, 84.0);
    set(49U, 144.0, 80.0);
    set(50U, 160.0, 78.0);
    set(51U, 176.0, 80.0);
    set(52U, 192.0, 84.0);
    set(53U, 192.0, 96.0);
    set(54U, 176.0, 100.0);
    set(55U, 160.0, 102.0);
    set(56U, 144.0, 100.0);
    set(57U, 128.0, 96.0);
    set(58U, 120.0, 90.0);
    set(59U, 136.0, 88.0);
    set(60U, 160.0, 86.0);
    set(61U, 184.0, 88.0);
    set(62U, 200.0, 90.0);
    set(63U, 184.0, 92.0);
    set(64U, 160.0, 94.0);
    set(65U, 136.0, 92.0);
}

void test_source_preserving_geometry_follows_face_motion() {
    OpenSeeFaceSignalAdapter adapter;
    adapter.set_current_frame_geometry(true);
    auto first = make_request(800U, 800U, 30'000'000'000);
    expect(adapter.adapt(first.landmarks, first.appearance, first.resources,
                         first.source.identity, 30'005'000'000).accepted(),
           "source-preserving geometry acquires its initial actor");
    auto translated = make_request(801U, 801U, 30'070'000'000);
    for (auto& point : translated.landmarks.landmarks) point.x += .15;
    translated.landmarks.face_bounds.x += .15;
    const auto moved = adapter.adapt(translated.landmarks, translated.appearance,
        translated.resources, translated.source.identity, 30'075'000'000);
    expect(moved.accepted() &&
               std::abs(moved.tracking->mouth_landmarks.left_corner.x - .58) < 1e-12,
           "whole-face translation preserves exact current mouth position without an EMA lag");

    auto scaled = make_request(802U, 802U, 30'140'000'000);
    const double scale = 1.15;
    for (auto& point : scaled.landmarks.landmarks) {
        point.x = .5 + (point.x - .5) * scale + .15;
        point.y = .5 + (point.y - .5) * scale;
    }
    scaled.landmarks.face_bounds = {.5 - .2*scale + .15, .5 - .34*scale,
                                    .4*scale, .68*scale};
    const auto resized = adapter.adapt(scaled.landmarks, scaled.appearance,
        scaled.resources, scaled.source.identity, 30'145'000'000);
    expect(resized.accepted(), "whole-face scale change is not mistaken for a mouth identity jump");

    auto mouth_jump = scaled;
    mouth_jump.source.identity.sequence++;
    mouth_jump.source.identity.captured_at_ns += 70'000'000;
    mouth_jump.landmarks.frame = mouth_jump.source.identity;
    mouth_jump.landmarks.measured_at_ns += 70'000'000;
    shift_mouth_geometry(mouth_jump, .13);
    expect(adapter.adapt(mouth_jump.landmarks, mouth_jump.appearance, mouth_jump.resources,
                         mouth_jump.source.identity, 30'215'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "mouth-only displacement retains the hard geometry-consistency gate");
}

void test_typed_openseeface_mapping_and_rate_policy() {
    OpenSeeFaceSignalAdapter adapter;
    auto request = make_request(1U, 9U, 1'000'000'000);
    const auto accepted = adapter.adapt(request.landmarks, request.appearance, request.resources,
                                        request.source.identity, 1'010'000'000);
    expect(accepted.accepted(), "qualified typed OpenSeeFace packet is accepted");
    expect(accepted.admitted_signal_rate_hz == 15U, "nominal signal is capped at 15 Hz");
    expect(accepted.tracking->mouth_landmarks.left_corner.x ==
               request.landmarks.landmarks[62U].x &&
               accepted.tracking->mouth_landmarks.left_corner.y ==
                   request.landmarks.landmarks[62U].y,
           "OpenSeeFace inner-corner index 62 maps to the semantic left corner");
    expect(accepted.tracking->mouth_landmarks.right_corner.x ==
               request.landmarks.landmarks[58U].x &&
               accepted.tracking->mouth_landmarks.right_corner.y ==
                   request.landmarks.landmarks[58U].y,
           "OpenSeeFace inner-corner index 58 maps to the semantic right corner");
    expect(accepted.tracking->mouth_landmarks.upper_lip_center.y <
               accepted.tracking->mouth_landmarks.lower_lip_center.y,
           "OpenSeeFace 59..61 and 63..65 map to upper and lower aperture lines");
    expect(accepted.tracking->mouth_landmarks.schema_version == 2U &&
               accepted.tracking->mouth_landmarks.contour_points ==
                   mouth_contour_point_count &&
               accepted.tracking->mouth_landmarks.contour[10U].x ==
                   request.landmarks.landmarks[58U].x &&
               accepted.tracking->mouth_landmarks.contour[17U].y ==
                   request.landmarks.landmarks[65U].y,
           "semantic schema 2 preserves all ordered OpenSeeFace mouth points 48..65");
    expect(accepted.tracking->mouth_bounds.width < request.landmarks.face_bounds.width,
           "dynamic mouth mask remains contained by the current face");

    auto too_soon = make_request(2U, 10U, 1'030'000'000);
    const auto limited = adapter.adapt(too_soon.landmarks, too_soon.appearance,
                                       too_soon.resources, too_soon.source.identity,
                                       1'035'000'000);
    expect(limited.disposition == SignalDisposition::bypass_rate_limited,
           "queue source is capped rather than exceeding 15 Hz");

    auto elevated = make_request(3U, 11U, 1'100'000'000);
    elevated.resources.pressure = VisualPressure::elevated_memory;
    elevated.resources.admitted_signal_rate_hz = 15U;
    const auto degraded = adapter.adapt(elevated.landmarks, elevated.appearance,
                                        elevated.resources, elevated.source.identity,
                                        1'105'000'000);
    expect(degraded.disposition == SignalDisposition::bypass_rate_limited &&
               degraded.admitted_signal_rate_hz == 5U,
           "memory pressure reduces the CPU signal rate to 5 Hz and drops early work");
}

void test_qualified_detector_confidence_floor() {
    MouthProductRuntime qualified_runtime;
    auto qualified = make_request(90U, 90U, 8'000'000'000);
    set_valid_schema_four_geometry(qualified);
    qualified.landmarks.detector_confidence = 0.719482;
    const auto qualified_frame = qualified.source.identity;
    const auto queued = qualified_runtime.submit(std::move(qualified), qualified_frame,
                                                  8'005'000'000);
    expect(queued.receipt.disposition == PresentationDisposition::queued,
           "an identity-bound OpenSeeFace face above the 0.70 product floor is queued");
    const auto rendered = qualified_runtime.process_latest(qualified_frame, 8'010'000'000);
    expect(rendered.proposed_residual(),
           "the worker retains a residual for the qualified 0.70-0.78 detector band");

    MouthProductRuntime below_floor_runtime;
    auto below_floor = make_request(91U, 91U, 8'100'000'000);
    below_floor.landmarks.detector_confidence = 0.699999;
    const auto below_floor_frame = below_floor.source.identity;
    const auto rejected = below_floor_runtime.submit(std::move(below_floor), below_floor_frame,
                                                      8'105'000'000);
    expect(rejected.receipt.signal_disposition == SignalDisposition::bypass_invalid_packet,
           "detector confidence below the guarded 0.70 floor still fails closed");
}

void test_padded_mask_may_cross_detector_edge_but_lip_contour_may_not() {
    auto padded_only = make_request(1U, 9U, 1'000'000'000);
    // Raw mouth points end at y=0.66, while the 35% feathered mask extends to
    // roughly 0.693. A detector box ending at 0.67 still contains every lip
    // landmark and is therefore a safe exact-frame input.
    padded_only.landmarks.face_bounds.height = 0.51;
    OpenSeeFaceSignalAdapter accepted_adapter;
    const auto accepted = accepted_adapter.adapt(
        padded_only.landmarks,
        padded_only.appearance,
        padded_only.resources,
        padded_only.source.identity,
        1'010'000'000);
    expect(accepted.accepted(),
           "feathered mask may cross the detector edge when the raw lip contour is contained");

    auto contour_outside = padded_only;
    contour_outside.landmarks.landmarks[57U].y = 0.68;
    OpenSeeFaceSignalAdapter rejected_adapter;
    const auto rejected = rejected_adapter.adapt(
        contour_outside.landmarks,
        contour_outside.appearance,
        contour_outside.resources,
        contour_outside.source.identity,
        1'010'000'000);
    expect(rejected.disposition == SignalDisposition::bypass_unsafe_roi,
           "a raw lip landmark outside the detector box still fails closed");
}

void test_closed_mouth_landmark_jitter_is_canonicalized() {
    auto closed = make_request(92U, 92U, 8'200'000'000);
    closed.landmarks.detector_confidence = 0.864673;
    closed.landmarks.landmark_confidence = 0.894691;
    // A held-out real-person frame from the pinned OpenSeeFace pack produced
    // inner-lip means only 0.001053 frame-height apart in the wrong order.
    // This is sub-pixel closed-mouth jitter at the 420x540 proof resolution,
    // while the full contour and both corners remain finite and face-bound.
    for (std::size_t index = 59U; index <= 61U; ++index) {
        closed.landmarks.landmarks[index].y = 0.434731;
    }
    for (std::size_t index = 63U; index <= 65U; ++index) {
        closed.landmarks.landmarks[index].y = 0.433678;
    }

    OpenSeeFaceSignalAdapter closed_adapter;
    const auto accepted = closed_adapter.adapt(
        closed.landmarks, closed.appearance, closed.resources,
        closed.source.identity, 8'205'000'000);
    expect(accepted.accepted(),
           "sub-pixel closed-mouth landmark inversion is canonicalized safely");
    expect(accepted.accepted() &&
               accepted.tracking->mouth_landmarks.upper_lip_center.y <
                   accepted.tracking->mouth_landmarks.lower_lip_center.y,
           "canonical closed-mouth semantics preserve an ordered near-zero aperture");

    auto crossed = make_request(93U, 93U, 8'300'000'000);
    for (std::size_t index = 59U; index <= 61U; ++index) {
        crossed.landmarks.landmarks[index].y = 0.65;
    }
    for (std::size_t index = 63U; index <= 65U; ++index) {
        crossed.landmarks.landmarks[index].y = 0.58;
    }
    OpenSeeFaceSignalAdapter crossed_adapter;
    const auto rejected = crossed_adapter.adapt(
        crossed.landmarks, crossed.appearance, crossed.resources,
        crossed.source.identity, 8'305'000'000);
    expect(rejected.disposition == SignalDisposition::bypass_unsafe_roi,
           "materially crossed inner-lip geometry still fails closed");
}

void test_appearance_occlusion_latch_and_recovery() {
    OpenSeeFaceSignalAdapter adapter;
    auto first = make_request(1U, 1U, 2'000'000'000);
    expect(adapter.adapt(first.landmarks, first.appearance, first.resources,
                         first.source.identity, 2'005'000'000).accepted(),
           "appearance latch starts from authoritative selected actor evidence");

    auto blocked = make_request(2U, 2U, 2'070'000'000);
    blocked.appearance.blocker_coverage = 0.60;
    expect(adapter.adapt(blocked.landmarks, blocked.appearance, blocked.resources,
                         blocked.source.identity, 2'075'000'000).disposition ==
               SignalDisposition::bypass_occluded,
           "modal or blocker coverage suppresses all mouth output immediately");
    expect(!adapter.appearance_latched(), "rejected appearance opens the latch");

    auto recovery_one = make_request(3U, 3U, 2'140'000'000);
    expect(adapter.adapt(recovery_one.landmarks, recovery_one.appearance,
                         recovery_one.resources, recovery_one.source.identity,
                         2'145'000'000).disposition == SignalDisposition::bypass_appearance,
           "one good frame cannot recover a rejected appearance latch");
    auto recovery_two = make_request(4U, 4U, 2'210'000'000);
    expect(adapter.adapt(recovery_two.landmarks, recovery_two.appearance,
                         recovery_two.resources, recovery_two.source.identity,
                         2'215'000'000).accepted(),
           "two consecutive authoritative matches recover the latch");

    auto wrong_actor = make_request(5U, 5U, 2'280'000'000);
    wrong_actor.appearance.runtime_actor_id = 999U;
    expect(adapter.adapt(wrong_actor.landmarks, wrong_actor.appearance,
                         wrong_actor.resources, wrong_actor.source.identity,
                         2'285'000'000).disposition == SignalDisposition::bypass_wrong_actor,
           "identity runtime actor mismatch fails closed");
}

void test_persistent_moved_geometry_reacquires_without_old_position_deadlock() {
    OpenSeeFaceSignalAdapter adapter;
    auto initial = make_request(110U, 110U, 9'000'000'000);
    const auto first = adapter.adapt(initial.landmarks, initial.appearance, initial.resources,
                                     initial.source.identity, 9'005'000'000);
    expect(first.accepted(), "persistent-motion test establishes stable geometry");

    auto moved_one = make_request(111U, 111U, 9'070'000'000);
    shift_mouth_geometry(moved_one, 0.10);
    const auto first_vote = adapter.adapt(
        moved_one.landmarks, moved_one.appearance, moved_one.resources,
        moved_one.source.identity, 9'075'000'000);
    expect(first_vote.disposition == SignalDisposition::bypass_appearance,
           "the first large geometry jump is withheld while reacquisition starts");

    auto moved_two = make_request(112U, 112U, 9'140'000'000);
    shift_mouth_geometry(moved_two, 0.10);
    const auto reacquired = adapter.adapt(
        moved_two.landmarks, moved_two.appearance, moved_two.resources,
        moved_two.source.identity, 9'145'000'000);
    expect(reacquired.accepted(),
           "two fresh consistent moved observations replace stale stable geometry");
    expect(reacquired.accepted() && first.accepted() &&
               reacquired.tracking->mouth_bounds.x > first.tracking->mouth_bounds.x + 0.08,
           "reacquisition latches the new position without smoothing across the jump");
}

void test_geometry_reacquisition_requires_consecutive_fresh_consensus() {
    OpenSeeFaceSignalAdapter single_outlier_adapter;
    auto initial = make_request(120U, 120U, 10'000'000'000);
    const auto stable = single_outlier_adapter.adapt(
        initial.landmarks, initial.appearance, initial.resources,
        initial.source.identity, 10'005'000'000);
    expect(stable.accepted(), "single-outlier test establishes stable geometry");

    auto outlier = make_request(121U, 121U, 10'070'000'000);
    shift_mouth_geometry(outlier, 0.10);
    expect(single_outlier_adapter.adapt(
               outlier.landmarks, outlier.appearance, outlier.resources,
               outlier.source.identity, 10'075'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "one large geometry outlier cannot move the accepted mouth position");

    auto original_one = make_request(122U, 122U, 10'140'000'000);
    expect(single_outlier_adapter.adapt(
               original_one.landmarks, original_one.appearance, original_one.resources,
               original_one.source.identity, 10'145'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "returning stable geometry starts fresh post-rejection consensus");
    auto original_two = make_request(123U, 123U, 10'210'000'000);
    const auto recovered_original = single_outlier_adapter.adapt(
        original_two.landmarks, original_two.appearance, original_two.resources,
        original_two.source.identity, 10'215'000'000);
    expect(recovered_original.accepted() && stable.accepted() &&
               std::abs(recovered_original.tracking->mouth_bounds.x -
                        stable.tracking->mouth_bounds.x) < 1e-12,
           "a rejected outlier never becomes the stable mouth geometry");

    OpenSeeFaceSignalAdapter alternating_adapter;
    auto alternating_initial = make_request(124U, 124U, 11'000'000'000);
    expect(alternating_adapter.adapt(
               alternating_initial.landmarks, alternating_initial.appearance,
               alternating_initial.resources, alternating_initial.source.identity,
               11'005'000'000).accepted(),
           "alternating-candidate test establishes stable geometry");
    const double offsets[] = {0.10, -0.10, 0.10, -0.10};
    for (std::size_t index = 0; index < 4U; ++index) {
        const auto timestamp = 11'070'000'000 +
            static_cast<Nanoseconds>(index) * 70'000'000;
        auto alternating = make_request(125U + index, 125U + index, timestamp);
        shift_mouth_geometry(alternating, offsets[index]);
        expect(alternating_adapter.adapt(
                   alternating.landmarks, alternating.appearance, alternating.resources,
                   alternating.source.identity, timestamp + 5'000'000).disposition ==
                   SignalDisposition::bypass_appearance,
               "alternating incompatible geometry cannot form reacquisition consensus");
    }

    OpenSeeFaceAdapterPolicy one_match_policy{};
    one_match_policy.recovery_matches_after_rejection = 1U;
    OpenSeeFaceSignalAdapter hard_floor_adapter(1U, one_match_policy);
    auto floor_initial = make_request(130U, 130U, 12'000'000'000);
    expect(hard_floor_adapter.adapt(
               floor_initial.landmarks, floor_initial.appearance, floor_initial.resources,
               floor_initial.source.identity, 12'005'000'000).accepted(),
           "hard-floor test establishes stable geometry");
    auto floor_jump = make_request(131U, 131U, 12'070'000'000);
    shift_mouth_geometry(floor_jump, 0.10);
    expect(hard_floor_adapter.adapt(
               floor_jump.landmarks, floor_jump.appearance, floor_jump.resources,
               floor_jump.source.identity, 12'075'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "configuration cannot weaken the two-observation reacquisition floor");
}

void test_geometry_reacquisition_rejects_duplicate_fast_and_old_votes() {
    OpenSeeFaceSignalAdapter adapter;
    auto initial = make_request(140U, 140U, 13'000'000'000);
    expect(adapter.adapt(initial.landmarks, initial.appearance, initial.resources,
                         initial.source.identity, 13'005'000'000).accepted(),
           "vote-timing test establishes stable geometry");

    auto first_vote = make_request(141U, 141U, 13'070'000'000);
    shift_mouth_geometry(first_vote, 0.10);
    expect(adapter.adapt(first_vote.landmarks, first_vote.appearance, first_vote.resources,
                         first_vote.source.identity, 13'075'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "first moved observation starts withheld consensus");

    auto duplicate = make_request(142U, 142U, 13'100'000'000);
    shift_mouth_geometry(duplicate, 0.10);
    duplicate.landmarks.measured_at_ns = first_vote.landmarks.measured_at_ns;
    expect(adapter.adapt(duplicate.landmarks, duplicate.appearance, duplicate.resources,
                         duplicate.source.identity, 13'105'000'000).disposition ==
               SignalDisposition::bypass_rate_limited,
           "a duplicate measurement timestamp cannot cast a second vote");

    auto too_fast = make_request(143U, 143U, 13'100'000'000);
    shift_mouth_geometry(too_fast, 0.10);
    expect(adapter.adapt(too_fast.landmarks, too_fast.appearance, too_fast.resources,
                         too_fast.source.identity, 13'105'000'000).disposition ==
               SignalDisposition::bypass_rate_limited,
           "a too-fast measurement cannot cast a second vote");

    auto second_fresh = make_request(144U, 144U, 13'140'000'000);
    shift_mouth_geometry(second_fresh, 0.10);
    expect(adapter.adapt(second_fresh.landmarks, second_fresh.appearance,
                         second_fresh.resources, second_fresh.source.identity,
                         13'145'000'000).accepted(),
           "the next rate-valid compatible observation completes consensus");

    OpenSeeFaceSignalAdapter old_vote_adapter;
    auto old_initial = make_request(145U, 145U, 14'000'000'000);
    expect(old_vote_adapter.adapt(
               old_initial.landmarks, old_initial.appearance, old_initial.resources,
               old_initial.source.identity, 14'005'000'000).accepted(),
           "old-vote test establishes stable geometry");
    auto old_first = make_request(146U, 146U, 14'070'000'000);
    shift_mouth_geometry(old_first, 0.10);
    expect(old_vote_adapter.adapt(
               old_first.landmarks, old_first.appearance, old_first.resources,
               old_first.source.identity, 14'075'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "old-vote test starts a candidate");
    auto after_pause = make_request(147U, 147U, 15'070'000'000);
    shift_mouth_geometry(after_pause, 0.10);
    expect(old_vote_adapter.adapt(
               after_pause.landmarks, after_pause.appearance, after_pause.resources,
               after_pause.source.identity, 15'075'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "a long gap restarts rather than completes geometry consensus");
    auto after_pause_fresh = make_request(148U, 148U, 15'140'000'000);
    shift_mouth_geometry(after_pause_fresh, 0.10);
    expect(old_vote_adapter.adapt(
               after_pause_fresh.landmarks, after_pause_fresh.appearance,
               after_pause_fresh.resources, after_pause_fresh.source.identity,
               15'145'000'000).accepted(),
           "two fresh observations after the gap can reacquire geometry");

    OpenSeeFaceSignalAdapter five_hz_adapter;
    auto five_hz_initial = make_request(149U, 149U, 15'500'000'000);
    five_hz_initial.resources.pressure = VisualPressure::elevated_memory;
    expect(five_hz_adapter.adapt(
               five_hz_initial.landmarks, five_hz_initial.appearance,
               five_hz_initial.resources, five_hz_initial.source.identity,
               15'505'000'000).accepted(),
           "five-hertz test establishes stable geometry at admitted cadence");
    auto five_hz_one = make_request(150U, 150U, 15'700'000'000);
    five_hz_one.resources.pressure = VisualPressure::elevated_memory;
    shift_mouth_geometry(five_hz_one, 0.10);
    expect(five_hz_adapter.adapt(
               five_hz_one.landmarks, five_hz_one.appearance, five_hz_one.resources,
               five_hz_one.source.identity, 15'705'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "first five-hertz moved observation starts consensus");
    auto five_hz_two = make_request(151U, 151U, 15'900'000'000);
    five_hz_two.resources.pressure = VisualPressure::elevated_memory;
    shift_mouth_geometry(five_hz_two, 0.10);
    expect(five_hz_adapter.adapt(
               five_hz_two.landmarks, five_hz_two.appearance, five_hz_two.resources,
               five_hz_two.source.identity, 15'905'000'000).accepted(),
           "fresh consensus remains reachable at the admitted five-hertz cadence");
}

void test_original_geometry_recovery_also_requires_fresh_consensus() {
    OpenSeeFaceSignalAdapter duplicate_adapter;
    auto initial = make_request(149U, 149U, 15'500'000'000);
    expect(duplicate_adapter.adapt(
               initial.landmarks, initial.appearance, initial.resources,
               initial.source.identity, 15'505'000'000).accepted(),
           "original-geometry duplicate test establishes stable geometry");
    auto occluded = make_request(150U, 150U, 15'570'000'000);
    occluded.landmarks.mouth_occluded = true;
    expect(duplicate_adapter.adapt(
               occluded.landmarks, occluded.appearance, occluded.resources,
               occluded.source.identity, 15'575'000'000).disposition ==
               SignalDisposition::bypass_occluded,
           "occlusion opens the original-geometry appearance latch");
    auto original_one = make_request(151U, 151U, 15'640'000'000);
    expect(duplicate_adapter.adapt(
               original_one.landmarks, original_one.appearance, original_one.resources,
               original_one.source.identity, 15'645'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "one original-geometry observation starts recovery consensus");
    auto duplicate = make_request(152U, 152U, 15'670'000'000);
    duplicate.landmarks.measured_at_ns = original_one.landmarks.measured_at_ns;
    expect(duplicate_adapter.adapt(
               duplicate.landmarks, duplicate.appearance, duplicate.resources,
               duplicate.source.identity, 15'675'000'000).disposition ==
               SignalDisposition::bypass_rate_limited,
           "a duplicate original-geometry timestamp cannot complete recovery");
    auto original_two = make_request(153U, 153U, 15'710'000'000);
    expect(duplicate_adapter.adapt(
               original_two.landmarks, original_two.appearance, original_two.resources,
               original_two.source.identity, 15'715'000'000).accepted(),
           "the next fresh original-geometry observation completes recovery");

    OpenSeeFaceSignalAdapter gap_adapter;
    auto gap_initial = make_request(154U, 154U, 16'000'000'000);
    expect(gap_adapter.adapt(
               gap_initial.landmarks, gap_initial.appearance, gap_initial.resources,
               gap_initial.source.identity, 16'005'000'000).accepted(),
           "original-geometry gap test establishes stable geometry");
    auto gap_occluded = make_request(155U, 155U, 16'070'000'000);
    gap_occluded.landmarks.mouth_occluded = true;
    expect(gap_adapter.adapt(
               gap_occluded.landmarks, gap_occluded.appearance, gap_occluded.resources,
               gap_occluded.source.identity, 16'075'000'000).disposition ==
               SignalDisposition::bypass_occluded,
           "original-geometry gap test opens the appearance latch");
    auto gap_one = make_request(156U, 156U, 16'140'000'000);
    expect(gap_adapter.adapt(
               gap_one.landmarks, gap_one.appearance, gap_one.resources,
               gap_one.source.identity, 16'145'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "original-geometry gap test starts recovery consensus");
    auto gap_old = make_request(157U, 157U, 17'140'000'000);
    expect(gap_adapter.adapt(
               gap_old.landmarks, gap_old.appearance, gap_old.resources,
               gap_old.source.identity, 17'145'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "a long gap restarts original-geometry recovery consensus");
    auto gap_fresh = make_request(158U, 158U, 17'210'000'000);
    expect(gap_adapter.adapt(
               gap_fresh.landmarks, gap_fresh.appearance, gap_fresh.resources,
               gap_fresh.source.identity, 17'215'000'000).accepted(),
           "fresh original-geometry observations after a gap can recover");
}

void test_geometry_reacquisition_candidate_resets_at_safety_boundaries() {
    OpenSeeFaceSignalAdapter occlusion_adapter;
    auto occlusion_initial = make_request(150U, 150U, 16'000'000'000);
    expect(occlusion_adapter.adapt(
               occlusion_initial.landmarks, occlusion_initial.appearance,
               occlusion_initial.resources, occlusion_initial.source.identity,
               16'005'000'000).accepted(),
           "occlusion reset test establishes stable geometry");
    auto before_occlusion = make_request(151U, 151U, 16'070'000'000);
    shift_mouth_geometry(before_occlusion, 0.10);
    expect(occlusion_adapter.adapt(
               before_occlusion.landmarks, before_occlusion.appearance,
               before_occlusion.resources, before_occlusion.source.identity,
               16'075'000'000).disposition == SignalDisposition::bypass_appearance,
           "occlusion reset test starts a candidate");
    auto occluded = make_request(152U, 152U, 16'140'000'000);
    shift_mouth_geometry(occluded, 0.10);
    occluded.landmarks.mouth_occluded = true;
    expect(occlusion_adapter.adapt(
               occluded.landmarks, occluded.appearance, occluded.resources,
               occluded.source.identity, 16'145'000'000).disposition ==
               SignalDisposition::bypass_occluded,
           "occlusion interrupts geometry consensus");
    auto after_occlusion_one = make_request(153U, 153U, 16'210'000'000);
    shift_mouth_geometry(after_occlusion_one, 0.10);
    expect(occlusion_adapter.adapt(
               after_occlusion_one.landmarks, after_occlusion_one.appearance,
               after_occlusion_one.resources, after_occlusion_one.source.identity,
               16'215'000'000).disposition == SignalDisposition::bypass_appearance,
           "the first moved observation after occlusion cannot reuse an old vote");
    auto after_occlusion_two = make_request(154U, 154U, 16'280'000'000);
    shift_mouth_geometry(after_occlusion_two, 0.10);
    expect(occlusion_adapter.adapt(
               after_occlusion_two.landmarks, after_occlusion_two.appearance,
               after_occlusion_two.resources, after_occlusion_two.source.identity,
               16'285'000'000).accepted(),
           "fresh post-occlusion consensus can reacquire geometry");

    OpenSeeFaceSignalAdapter identity_adapter;
    auto identity_initial = make_request(155U, 155U, 17'000'000'000);
    expect(identity_adapter.adapt(
               identity_initial.landmarks, identity_initial.appearance,
               identity_initial.resources, identity_initial.source.identity,
               17'005'000'000).accepted(),
           "identity reset test establishes stable geometry");
    auto before_identity_loss = make_request(156U, 156U, 17'070'000'000);
    shift_mouth_geometry(before_identity_loss, 0.10);
    expect(identity_adapter.adapt(
               before_identity_loss.landmarks, before_identity_loss.appearance,
               before_identity_loss.resources, before_identity_loss.source.identity,
               17'075'000'000).disposition == SignalDisposition::bypass_appearance,
           "identity reset test starts a candidate");
    auto identity_lost = make_request(157U, 157U, 17'140'000'000);
    shift_mouth_geometry(identity_lost, 0.10);
    identity_lost.appearance.identity_locked = false;
    expect(identity_adapter.adapt(
               identity_lost.landmarks, identity_lost.appearance, identity_lost.resources,
               identity_lost.source.identity, 17'145'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "identity uncertainty interrupts geometry consensus");
    auto after_identity_one = make_request(158U, 158U, 17'210'000'000);
    shift_mouth_geometry(after_identity_one, 0.10);
    expect(identity_adapter.adapt(
               after_identity_one.landmarks, after_identity_one.appearance,
               after_identity_one.resources, after_identity_one.source.identity,
               17'215'000'000).disposition == SignalDisposition::bypass_appearance,
           "the first moved observation after identity loss cannot reuse an old vote");
    auto after_identity_two = make_request(159U, 159U, 17'280'000'000);
    shift_mouth_geometry(after_identity_two, 0.10);
    expect(identity_adapter.adapt(
               after_identity_two.landmarks, after_identity_two.appearance,
               after_identity_two.resources, after_identity_two.source.identity,
               17'285'000'000).accepted(),
           "fresh post-identity consensus can reacquire geometry");

    OpenSeeFaceSignalAdapter invalid_adapter;
    auto invalid_initial = make_request(160U, 160U, 18'000'000'000);
    const auto invalid_stable = invalid_adapter.adapt(
        invalid_initial.landmarks, invalid_initial.appearance, invalid_initial.resources,
        invalid_initial.source.identity, 18'005'000'000);
    expect(invalid_stable.accepted(), "invalid reset test establishes stable geometry");
    auto before_invalid = make_request(161U, 161U, 18'070'000'000);
    shift_mouth_geometry(before_invalid, 0.10);
    expect(invalid_adapter.adapt(
               before_invalid.landmarks, before_invalid.appearance, before_invalid.resources,
               before_invalid.source.identity, 18'075'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "invalid reset test starts a candidate");
    auto low_confidence = make_request(162U, 162U, 18'140'000'000);
    shift_mouth_geometry(low_confidence, 0.10);
    low_confidence.landmarks.landmark_confidence = 0.50;
    expect(invalid_adapter.adapt(
               low_confidence.landmarks, low_confidence.appearance,
               low_confidence.resources, low_confidence.source.identity,
               18'145'000'000).disposition == SignalDisposition::bypass_invalid_packet,
           "invalid confidence clears the track and pending geometry candidate");
    expect(!invalid_adapter.appearance_latched(),
           "invalid confidence removes the prior appearance latch");

    OpenSeeFaceSignalAdapter cancelled_adapter;
    auto cancel_initial = make_request(163U, 163U, 19'000'000'000);
    expect(cancelled_adapter.adapt(
               cancel_initial.landmarks, cancel_initial.appearance, cancel_initial.resources,
               cancel_initial.source.identity, 19'005'000'000).accepted(),
           "cancellation reset test establishes stable geometry");
    auto before_cancel = make_request(164U, 164U, 19'070'000'000);
    shift_mouth_geometry(before_cancel, 0.10);
    expect(cancelled_adapter.adapt(
               before_cancel.landmarks, before_cancel.appearance, before_cancel.resources,
               before_cancel.source.identity, 19'075'000'000).disposition ==
               SignalDisposition::bypass_appearance,
           "cancellation reset test starts a candidate");
    expect(cancelled_adapter.cancel_to(2U),
           "generation cancellation clears stable and candidate geometry");
    auto cancelled_old_packet = make_request(165U, 165U, 19'140'000'000);
    shift_mouth_geometry(cancelled_old_packet, 0.10);
    expect(cancelled_adapter.adapt(
               cancelled_old_packet.landmarks, cancelled_old_packet.appearance,
               cancelled_old_packet.resources, cancelled_old_packet.source.identity,
               19'145'000'000).disposition == SignalDisposition::bypass_cancelled,
           "an old-generation packet cannot continue pre-cancellation consensus");
}

void test_product_queue_receipts_and_exact_current_frame() {
    MouthProductRuntime runtime;
    auto first = make_request(1U, 10U, 3'000'000'000);
    const auto first_frame = first.source.identity;
    const auto queued = runtime.submit(std::move(first), first_frame, 3'005'000'000);
    expect(queued.receipt.disposition == PresentationDisposition::queued,
           "accepted product request returns a queued receipt");
    expect(!queued.replaced.has_value(), "first queue submission replaces nothing");

    auto second = make_request(2U, 11U, 3'070'000'000);
    const auto second_frame = second.source.identity;
    const auto replaced = runtime.submit(std::move(second), second_frame, 3'075'000'000);
    expect(replaced.receipt.disposition == PresentationDisposition::queued,
           "newest current frame remains queued");
    expect(replaced.replaced && replaced.replaced->request.request_id == 1U &&
               replaced.replaced->disposition ==
                   PresentationDisposition::replaced_before_processing,
           "queue depth one emits an explicit replacement receipt");

    auto wrong_frame = second_frame;
    ++wrong_frame.sequence;
    const auto bypassed = runtime.process_latest(wrong_frame, 3'078'000'000);
    expect(bypassed.disposition == PresentationDisposition::bypassed_worker &&
               bypassed.worker_disposition == Disposition::bypass_wrong_frame,
           "late output for anything except the current frame is consumed and bypassed");

    MouthProductRuntime successful;
    auto request = make_request(3U, 12U, 4'000'000'000);
    set_valid_schema_four_geometry(request);
    const auto frame = request.source.identity;
    const auto submit = successful.submit(std::move(request), frame, 4'005'000'000);
    expect(submit.receipt.disposition == PresentationDisposition::queued,
           "valid current frame reaches the compositor queue");
    const auto rendered = successful.process_latest(frame, 4'010'000'000);
    expect(rendered.proposed_residual(), "product receipt carries a mouth-only residual proposal");
    expect(rendered.residual->track.actor_id == 41U &&
               rendered.residual->source_frame == frame &&
               rendered.residual->audio_clock.segment_id == 3U &&
               rendered.residual->audio_clock.sample_count == 800U,
           "residual receipt preserves exact actor, source-frame, and audio interval identity");
    expect(rendered.residual->normalized_bounds.x >= 0.0 &&
               rendered.residual->normalized_bounds.y >= 0.0 &&
               rendered.residual->normalized_bounds.right() <= 1.0 &&
               rendered.residual->normalized_bounds.bottom() <= 1.0 &&
               rendered.residual->normalized_bounds.width <= 0.90 &&
               rendered.residual->normalized_bounds.height <= 0.90,
           "source-only residual remains normalized and bounded to selected-face support");
}

void test_routine_rate_limit_preserves_current_pixel_history() {
    constexpr Nanoseconds first_at = 20'000'000'000;
    constexpr Nanoseconds limited_at = first_at + 30'000'000;
    constexpr Nanoseconds second_at = first_at + 70'000'000;
    constexpr std::uint64_t segment_id = 700U;

    const auto first_request = [&] {
        auto request = make_request(900U, 900U, first_at);
        set_valid_schema_four_geometry(request);
        request.drive.viseme = Viseme::open_vowel;
        request.drive.viseme_strength = 1.0;
        set_stream_sample(request, segment_id, 0U);
        return request;
    };
    const auto second_request = [&] {
        auto request = make_request(901U, 901U, second_at);
        set_valid_schema_four_geometry(request);
        request.drive.viseme = Viseme::open_vowel;
        request.drive.viseme_strength = 1.0;
        set_stream_sample(request, segment_id, 1'680U);
        return request;
    };

    MouthProductRuntime admitted_only;
    MouthProductRuntime interleaved;
    expect(admitted_only.install_atlas(make_source_only_schema_four_atlas()) &&
               interleaved.install_atlas(make_source_only_schema_four_atlas()),
           "rate-limit cadence controls install identical schema-four atlases");
    static_cast<void>(submit_and_process(admitted_only, first_request(),
                                         first_at + 5'000'000));
    static_cast<void>(submit_and_process(interleaved, first_request(),
                                         first_at + 5'000'000));

    auto routine_drop = make_request(999U, 999U, limited_at);
    set_valid_schema_four_geometry(routine_drop);
    routine_drop.drive.viseme = Viseme::dental;
    set_stream_sample(routine_drop, segment_id, 720U);
    const auto limited_frame = routine_drop.source.identity;
    const auto limited = interleaved.submit(std::move(routine_drop), limited_frame,
                                             limited_at + 5'000'000);
    expect(limited.receipt.signal_disposition ==
               SignalDisposition::bypass_rate_limited,
           "30 Hz interleaving produces the expected routine 15 Hz cadence drop");

    const auto control = submit_and_process(admitted_only, second_request(),
                                            second_at + 5'000'000);
    const auto with_drop = submit_and_process(interleaved, second_request(),
                                              second_at + 5'000'000);
    expect(control.proposed_residual() && with_drop.proposed_residual(),
           "both accepted 15 Hz samples produce schema-four residuals");
    expect(control.residual->coefficients.jaw_open ==
               with_drop.residual->coefficients.jaw_open &&
               control.residual->coefficients.funnel ==
                   with_drop.residual->coefficients.funnel &&
               control.residual->premultiplied_bgra ==
                   with_drop.residual->premultiplied_bgra,
           "routine rate limiting preserves the same shape and trajectory as admitted-only input");

    MouthProductRuntime invalid_interleaved;
    MouthProductRuntime fresh_second;
    expect(invalid_interleaved.install_atlas(make_source_only_schema_four_atlas()) &&
               fresh_second.install_atlas(make_source_only_schema_four_atlas()),
           "invalid-identity reset controls install identical schema-four atlases");
    static_cast<void>(submit_and_process(invalid_interleaved, first_request(),
                                         first_at + 5'000'000));
    auto untraceable_drop = make_request(998U, 998U, limited_at);
    set_valid_schema_four_geometry(untraceable_drop);
    set_stream_sample(untraceable_drop, segment_id, 720U);
    untraceable_drop.identity.turn_id_high = 0U;
    untraceable_drop.identity.turn_id_low = 0U;
    const auto untraceable_frame = untraceable_drop.source.identity;
    const auto untraceable = invalid_interleaved.submit(
        std::move(untraceable_drop), untraceable_frame,
        limited_at + 5'000'000);
    expect(untraceable.receipt.signal_disposition ==
               SignalDisposition::bypass_invalid_packet,
           "invalid identity remains a hard reset even on a rate-limited signal");

    const auto after_invalid = submit_and_process(
        invalid_interleaved, second_request(), second_at + 5'000'000);
    const auto fresh = submit_and_process(fresh_second, second_request(),
                                          second_at + 5'000'000);
    expect(after_invalid.proposed_residual() && fresh.proposed_residual() &&
               after_invalid.residual->coefficients.jaw_open ==
                   fresh.residual->coefficients.jaw_open &&
               after_invalid.residual->coefficients.funnel ==
                   fresh.residual->coefficients.funnel &&
               after_invalid.residual->premultiplied_bgra ==
                   fresh.residual->premultiplied_bgra,
           "invalid identity clears history exactly like a fresh schema-four stream");
    expect(after_invalid.residual->premultiplied_bgra !=
               control.residual->premultiplied_bgra,
           "invalid-identity regression is sensitive to whether trajectory history was reset");
}

void test_cancel_pressure_and_invalid_identity_receipts() {
    MouthProductRuntime runtime;
    auto request = make_request(1U, 20U, 5'000'000'000);
    const auto frame = request.source.identity;
    expect(runtime.submit(std::move(request), frame, 5'005'000'000).receipt.disposition ==
               PresentationDisposition::queued,
           "request is pending before cancellation");
    const auto cancelled = runtime.cancel_to(2U, 5'006'000'000);
    expect(cancelled && cancelled->disposition == PresentationDisposition::cancelled &&
               cancelled->worker_disposition == Disposition::bypass_cancelled,
           "generation cancellation synchronously returns a final receipt");
    expect(runtime.active_generation() == 2U, "generation advances monotonically");
    expect(!runtime.cancel_to(2U, 5'007'000'000), "replayed cancellation is rejected");

    MouthProductRuntime pressured;
    auto suspended = make_request(2U, 21U, 6'000'000'000);
    suspended.resources.pressure = VisualPressure::critical;
    const auto suspended_frame = suspended.source.identity;
    const auto result = pressured.submit(std::move(suspended), suspended_frame, 6'005'000'000);
    expect(result.receipt.disposition == PresentationDisposition::bypassed_signal &&
               result.receipt.signal_disposition == SignalDisposition::bypass_pressure,
           "critical resource pressure returns an honest no-residual receipt");

    MouthProductRuntime malformed;
    auto invalid = make_request(3U, 22U, 7'000'000'000);
    invalid.identity.turn_id_high = 0U;
    invalid.identity.turn_id_low = 0U;
    const auto invalid_frame = invalid.source.identity;
    const auto invalid_result = malformed.submit(std::move(invalid), invalid_frame, 7'005'000'000);
    expect(invalid_result.receipt.signal_disposition ==
               SignalDisposition::bypass_invalid_packet,
           "untraceable product requests cannot produce a residual");
}

void mark_sealed_click_source_only(ProductFrameRequest& request) {
    request.sealed_click_source_only = true;
    request.appearance.expected_descriptor_digest_high = 0U;
    request.appearance.expected_descriptor_digest_low = 0U;
    request.appearance.observed_descriptor_digest_high = 0U;
    request.appearance.observed_descriptor_digest_low = 0U;
    request.appearance.similarity = 0.0;
    request.appearance.temporal_iou = 0.0;
    request.appearance.identity_locked = false;
}

void test_sealed_click_source_only_requires_no_installed_atlas() {
    MouthProductRuntime source_only;
    auto admitted = make_request(170U, 170U, 20'000'000'000);
    mark_sealed_click_source_only(admitted);
    const auto admitted_frame = admitted.source.identity;
    const auto admitted_result =
        source_only.submit(std::move(admitted), admitted_frame, 20'005'000'000);
    expect(admitted_result.receipt.disposition == PresentationDisposition::queued &&
               admitted_result.receipt.signal_disposition == SignalDisposition::accepted,
           "sealed native click admits source-only geometry without inventing appearance identity");

    MouthProductRuntime unvouched;
    auto strict = make_request(171U, 171U, 20'100'000'000);
    mark_sealed_click_source_only(strict);
    strict.sealed_click_source_only = false;
    const auto strict_frame = strict.source.identity;
    const auto strict_result = unvouched.submit(std::move(strict), strict_frame, 20'105'000'000);
    expect(strict_result.receipt.disposition == PresentationDisposition::bypassed_signal &&
               strict_result.receipt.signal_disposition == SignalDisposition::bypass_appearance,
           "zero appearance evidence remains rejected without explicit source-only authority");

    MouthProductRuntime atlas_bound;
    expect(atlas_bound.install_atlas(make_source_only_schema_four_atlas()),
           "source-only scope guard fixture installs an actor atlas");
    auto conflicting = make_request(172U, 172U, 20'200'000'000);
    mark_sealed_click_source_only(conflicting);
    const auto conflicting_frame = conflicting.source.identity;
    const auto conflicting_result =
        atlas_bound.submit(std::move(conflicting), conflicting_frame, 20'205'000'000);
    expect(conflicting_result.receipt.disposition == PresentationDisposition::bypassed_signal &&
               conflicting_result.receipt.signal_disposition ==
                   SignalDisposition::bypass_appearance,
           "source-only authority fails closed while any character atlas remains installed");
}

} // namespace

int main() {
    test_source_preserving_geometry_follows_face_motion();
    test_typed_openseeface_mapping_and_rate_policy();
    test_qualified_detector_confidence_floor();
    test_padded_mask_may_cross_detector_edge_but_lip_contour_may_not();
    test_closed_mouth_landmark_jitter_is_canonicalized();
    test_appearance_occlusion_latch_and_recovery();
    test_persistent_moved_geometry_reacquires_without_old_position_deadlock();
    test_geometry_reacquisition_requires_consecutive_fresh_consensus();
    test_geometry_reacquisition_rejects_duplicate_fast_and_old_votes();
    test_original_geometry_recovery_also_requires_fresh_consensus();
    test_geometry_reacquisition_candidate_resets_at_safety_boundaries();
    test_product_queue_receipts_and_exact_current_frame();
    test_routine_rate_limit_preserves_current_pixel_history();
    test_cancel_pressure_and_invalid_identity_receipts();
    test_sealed_click_source_only_requires_no_installed_atlas();
    if (failures != 0) {
        std::cerr << failures << " product runtime assertion(s) failed\n";
        return 1;
    }
    std::cout << "PASS: typed OpenSeeFace adapter, guarded product queue, and receipts\n";
    return 0;
}
