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
    for (std::size_t index = 60U; index < 66U; ++index) {
        request.landmarks.landmarks[index] = {
            0.47 + static_cast<double>(index - 60U) * 0.012,
            index < 63U ? 0.595 : 0.63,
            0.96,
        };
    }
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
    request.drive.clock = {generation, request_id, 0U, 24'000U, 1U, captured_at_ns};
    request.drive.viseme = Viseme::open_vowel;
    request.drive.viseme_strength = 0.75;
    request.deadline_ns = captured_at_ns + 70'000'000;
    return request;
}

void test_typed_openseeface_mapping_and_rate_policy() {
    OpenSeeFaceSignalAdapter adapter;
    auto request = make_request(1U, 9U, 1'000'000'000);
    const auto accepted = adapter.adapt(request.landmarks, request.appearance, request.resources,
                                        request.source.identity, 1'010'000'000);
    expect(accepted.accepted(), "qualified typed OpenSeeFace packet is accepted");
    expect(accepted.admitted_signal_rate_hz == 15U, "nominal signal is capped at 15 Hz");
    expect(accepted.tracking->mouth_landmarks.left_corner.x ==
               request.landmarks.landmarks[48U].x &&
               accepted.tracking->mouth_landmarks.left_corner.y ==
                   request.landmarks.landmarks[48U].y,
           "index 48 maps to the semantic left mouth corner");
    expect(accepted.tracking->mouth_landmarks.right_corner.x ==
               request.landmarks.landmarks[54U].x &&
               accepted.tracking->mouth_landmarks.right_corner.y ==
                   request.landmarks.landmarks[54U].y,
           "index 54 maps to the semantic right mouth corner");
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
    const auto frame = request.source.identity;
    const auto submit = successful.submit(std::move(request), frame, 4'005'000'000);
    expect(submit.receipt.disposition == PresentationDisposition::queued,
           "valid current frame reaches the compositor queue");
    const auto rendered = successful.process_latest(frame, 4'010'000'000);
    expect(rendered.proposed_residual(), "product receipt carries a mouth-only residual proposal");
    expect(rendered.residual->track.actor_id == 41U &&
               rendered.residual->source_frame == frame,
           "residual receipt preserves exact actor and source-frame identity");
    expect(rendered.residual->normalized_bounds.width <= 0.45 &&
               rendered.residual->normalized_bounds.height <= 0.35,
           "product residual remains inside hard presentation ceilings");
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

} // namespace

int main() {
    test_typed_openseeface_mapping_and_rate_policy();
    test_qualified_detector_confidence_floor();
    test_padded_mask_may_cross_detector_edge_but_lip_contour_may_not();
    test_appearance_occlusion_latch_and_recovery();
    test_product_queue_receipts_and_exact_current_frame();
    test_cancel_pressure_and_invalid_identity_receipts();
    if (failures != 0) {
        std::cerr << failures << " product runtime assertion(s) failed\n";
        return 1;
    }
    std::cout << "PASS: typed OpenSeeFace adapter, guarded product queue, and receipts\n";
    return 0;
}
