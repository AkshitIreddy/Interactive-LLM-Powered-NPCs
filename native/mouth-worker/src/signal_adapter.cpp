#include "npc/mouth_worker/signal_adapter.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace npc::mouth {
namespace {

// CPU ONNX detection + 66-point landmarks typically completes inside one
// 15 Hz visual interval, but the leased WGC frame can already be one interval
// old. A 150 ms ceiling keeps two-frame fail-open behavior without rejecting
// the measured 95-115 ms Windows laptop path.
constexpr Nanoseconds hard_maximum_measurement_age_ns = 150'000'000;
constexpr std::uint32_t hard_maximum_signal_rate_hz = 15U;
// The pinned OpenSeeFace MNV3 detector admits candidates at 0.60. Keep a
// stricter product floor, but do not discard correctly bound faces in the
// 0.70-0.78 band after the independent identity/appearance gates pass.
constexpr double hard_minimum_detector_confidence = 0.70;
constexpr double hard_minimum_landmark_confidence = 0.82;
constexpr double hard_minimum_visibility_ratio = 0.72;
constexpr double hard_minimum_appearance_similarity = 0.82;
constexpr double hard_minimum_temporal_iou = 0.42;
constexpr double hard_maximum_blocker_coverage = 0.35;
constexpr double hard_maximum_center_motion_face_fraction = 0.12;
constexpr double hard_maximum_area_delta_fraction = 0.60;

[[nodiscard]] bool finite_probability(const double value) noexcept {
    return std::isfinite(value) && value >= 0.0 && value <= 1.0;
}

[[nodiscard]] bool valid_rect(const NormalizedRect& value) noexcept {
    return std::isfinite(value.x) && std::isfinite(value.y) &&
           std::isfinite(value.width) && std::isfinite(value.height) &&
           value.x >= 0.0 && value.y >= 0.0 && value.width > 0.0 && value.height > 0.0 &&
           value.right() <= 1.0 && value.bottom() <= 1.0;
}

[[nodiscard]] bool contains(const NormalizedRect& outer,
                            const NormalizedRect& inner) noexcept {
    return inner.x >= outer.x && inner.y >= outer.y &&
           inner.right() <= outer.right() && inner.bottom() <= outer.bottom();
}

[[nodiscard]] bool valid_landmark(const NormalizedLandmark& value) noexcept {
    return std::isfinite(value.x) && std::isfinite(value.y) &&
           finite_probability(value.confidence) && value.x >= 0.0 && value.x <= 1.0 &&
           value.y >= 0.0 && value.y <= 1.0;
}

[[nodiscard]] NormalizedLandmark average(const NormalizedLandmark& first,
                                         const NormalizedLandmark& second,
                                         const NormalizedLandmark& third) noexcept {
    return {
        (first.x + second.x + third.x) / 3.0,
        (first.y + second.y + third.y) / 3.0,
        std::min({first.confidence, second.confidence, third.confidence}),
    };
}

[[nodiscard]] NormalizedLandmark smooth(const NormalizedLandmark& current,
                                        const NormalizedLandmark& previous,
                                        const double alpha) noexcept {
    return {
        previous.x + (current.x - previous.x) * alpha,
        previous.y + (current.y - previous.y) * alpha,
        std::min(current.confidence, previous.confidence),
    };
}

[[nodiscard]] NormalizedRect smooth(const NormalizedRect& current,
                                    const NormalizedRect& previous,
                                    const double alpha) noexcept {
    return {
        previous.x + (current.x - previous.x) * alpha,
        previous.y + (current.y - previous.y) * alpha,
        previous.width + (current.width - previous.width) * alpha,
        previous.height + (current.height - previous.height) * alpha,
    };
}

[[nodiscard]] double area(const NormalizedRect& value) noexcept {
    return value.width * value.height;
}

[[nodiscard]] double center_distance_over_face(const NormalizedRect& first,
                                               const NormalizedRect& second,
                                               const NormalizedRect& face) noexcept {
    const double dx = (first.x + first.width * 0.5) - (second.x + second.width * 0.5);
    const double dy = (first.y + first.height * 0.5) - (second.y + second.height * 0.5);
    const double diagonal = std::hypot(face.width, face.height);
    return diagonal > std::numeric_limits<double>::epsilon()
        ? std::hypot(dx, dy) / diagonal
        : std::numeric_limits<double>::infinity();
}

[[nodiscard]] NormalizedRect mouth_contour_bounds_from_landmarks(
    const std::array<NormalizedLandmark, openseeface_landmark_count_v1>& points) noexcept {
    // The qualified lm_model1 output follows the 66-point LS3D-W layout. Its
    // outer and inner lip contour is indices 48..65 inclusive.
    double left = 1.0;
    double top = 1.0;
    double right = 0.0;
    double bottom = 0.0;
    for (std::size_t index = 48U; index < 66U; ++index) {
        left = std::min(left, points[index].x);
        top = std::min(top, points[index].y);
        right = std::max(right, points[index].x);
        bottom = std::max(bottom, points[index].y);
    }
    const double width = right - left;
    const double height = bottom - top;
    if (!(width > 0.0) || !(height > 0.0)) {
        return {};
    }
    return {left, top, width, height};
}

[[nodiscard]] NormalizedRect padded_mouth_bounds_from_contour(
    const NormalizedRect& contour) noexcept {
    if (!valid_rect(contour)) {
        return {};
    }
    const double left = contour.x;
    const double top = contour.y;
    const double right = contour.right();
    const double bottom = contour.bottom();
    const double width = contour.width;
    const double height = contour.height;
    const double horizontal_padding = width * 0.20;
    const double vertical_padding = height * 0.35;
    const double padded_left = std::max(0.0, left - horizontal_padding);
    const double padded_top = std::max(0.0, top - vertical_padding);
    const double padded_right = std::min(1.0, right + horizontal_padding);
    const double padded_bottom = std::min(1.0, bottom + vertical_padding);
    return {padded_left, padded_top, padded_right - padded_left, padded_bottom - padded_top};
}

[[nodiscard]] MouthLandmarks semantic_mouth_landmarks(
    const OpenSeeFaceLandmarkPacketV1& packet) noexcept {
    MouthLandmarks result{};
    result.schema_version = 2U;
    result.provider_instance_id = packet.provider_instance_id;
    // OpenSeeFace's 66-point layout is close to iBUG-68 but removes two mouth
    // corner points; it is not the ordinary dlib 68-point ordering. Its own
    // FeatureExtractor measures mouth width from 58/62, upper opening from
    // 59..61, and lower opening from 63..65. The previous dlib-style 48/54
    // mapping selected unrelated outer-contour points and collapsed the
    // renderer's effective aperture.
    const auto& first_corner = packet.landmarks[58U];
    const auto& second_corner = packet.landmarks[62U];
    result.left_corner = first_corner.x < second_corner.x ? first_corner : second_corner;
    result.right_corner = first_corner.x < second_corner.x ? second_corner : first_corner;
    result.upper_lip_center = average(packet.landmarks[59U], packet.landmarks[60U],
                                      packet.landmarks[61U]);
    result.lower_lip_center = average(packet.landmarks[63U], packet.landmarks[64U],
                                      packet.landmarks[65U]);
    result.contour_points = static_cast<std::uint32_t>(result.contour.size());
    std::copy_n(packet.landmarks.begin() + 48U, result.contour.size(), result.contour.begin());
    return result;
}

[[nodiscard]] std::uint32_t admitted_rate(const VisualResourceStateV1& resources) noexcept {
    if (resources.schema_version != 1U || !resources.local_visuals_admitted ||
        resources.pressure == VisualPressure::critical ||
        resources.pressure == VisualPressure::suspended) {
        return 0U;
    }
    const std::uint32_t pressure_cap = [&] {
        switch (resources.pressure) {
        case VisualPressure::nominal: return 15U;
        case VisualPressure::elevated_cpu: return 10U;
        case VisualPressure::elevated_memory: return 5U;
        case VisualPressure::critical:
        case VisualPressure::suspended: return 0U;
        }
        return 0U;
    }();
    return std::min({resources.admitted_signal_rate_hz, pressure_cap,
                     hard_maximum_signal_rate_hz});
}

[[nodiscard]] bool appearance_valid(const AppearanceGateEvidenceV1& value,
                                    const TrackBinding& track,
                                    const OpenSeeFaceAdapterPolicy& policy) noexcept {
    const bool has_expected_digest = value.expected_descriptor_digest_high != 0U ||
                                     value.expected_descriptor_digest_low != 0U;
    const bool has_observed_digest = value.observed_descriptor_digest_high != 0U ||
                                     value.observed_descriptor_digest_low != 0U;
    return value.schema_version == 1U && value.runtime_actor_id == track.actor_id &&
           value.descriptor_revision != 0U && has_expected_digest && has_observed_digest &&
           value.identity_locked && value.target_visible && !value.scene_transition &&
           finite_probability(value.similarity) &&
           value.similarity >= std::max(policy.minimum_appearance_similarity,
                                        hard_minimum_appearance_similarity) &&
           finite_probability(value.temporal_iou) &&
           value.temporal_iou >= std::max(policy.minimum_temporal_iou,
                                          hard_minimum_temporal_iou) &&
           finite_probability(value.blocker_coverage) &&
           value.blocker_coverage <= std::min(policy.maximum_blocker_coverage,
                                              hard_maximum_blocker_coverage);
}

} // namespace

OpenSeeFaceSignalAdapter::OpenSeeFaceSignalAdapter(const std::uint64_t initial_generation,
                                                   OpenSeeFaceAdapterPolicy policy)
    : active_generation_(initial_generation), policy_(std::move(policy)) {}

SignalDecision OpenSeeFaceSignalAdapter::adapt(const OpenSeeFaceLandmarkPacketV1& packet,
                                               const AppearanceGateEvidenceV1& appearance,
                                               const VisualResourceStateV1& resources,
                                               const FrameIdentity& current_frame,
                                               const Nanoseconds now_ns) {
    const std::uint32_t rate = admitted_rate(resources);
    if (rate == 0U) {
        reset_track();
        return bypass(SignalDisposition::bypass_pressure, 0U);
    }
    if (packet.track.cancellation_generation != active_generation_) {
        return bypass(SignalDisposition::bypass_cancelled, rate);
    }
    if (packet.track.actor_id == 0U || packet.track.track_id == 0U ||
        packet.track.track_epoch == 0U || packet.provider_instance_id == 0U ||
        packet.frame.sequence == 0U || packet.frame.geometry_epoch == 0U ||
        packet.source_frame_qpc == 0U || packet.qpc_frequency == 0U ||
        packet.measured_at_ns <= 0 || now_ns <= 0) {
        reset_track();
        return bypass(SignalDisposition::bypass_invalid_packet, rate);
    }
    if (packet.frame != current_frame) {
        return bypass(SignalDisposition::bypass_wrong_frame, rate);
    }
    if (packet.schema_version != 1U || !valid_rect(packet.face_bounds) ||
        !finite_probability(packet.detector_confidence) ||
        !finite_probability(packet.landmark_confidence) ||
        !finite_probability(packet.visibility_ratio) || !std::isfinite(packet.pose.yaw) ||
        !std::isfinite(packet.pose.pitch) || !std::isfinite(packet.pose.roll) ||
        packet.detector_confidence < std::max(policy_.minimum_detector_confidence,
                                              hard_minimum_detector_confidence) ||
        packet.landmark_confidence < std::max(policy_.minimum_landmark_confidence,
                                              hard_minimum_landmark_confidence) ||
        packet.visibility_ratio < std::max(policy_.minimum_visibility_ratio,
                                           hard_minimum_visibility_ratio)) {
        reset_track();
        return bypass(SignalDisposition::bypass_invalid_packet, rate);
    }
    for (const auto& landmark : packet.landmarks) {
        if (!valid_landmark(landmark)) {
            reset_track();
            return bypass(SignalDisposition::bypass_invalid_packet, rate);
        }
    }
    const Nanoseconds maximum_age = std::min(policy_.maximum_measurement_age_ns,
                                             hard_maximum_measurement_age_ns);
    if (packet.measured_at_ns > now_ns || current_frame.captured_at_ns > now_ns ||
        now_ns - packet.measured_at_ns > maximum_age ||
        now_ns - current_frame.captured_at_ns > maximum_age) {
        return bypass(SignalDisposition::bypass_stale, rate);
    }
    if (packet.mouth_occluded || finite_probability(appearance.blocker_coverage) &&
        appearance.blocker_coverage > std::min(policy_.maximum_blocker_coverage,
                                               hard_maximum_blocker_coverage)) {
        if (stable_) {
            stable_->rejected_since_accept = true;
            stable_->recovery_matches = 0U;
        }
        return bypass(SignalDisposition::bypass_occluded, rate);
    }
    if (!appearance_valid(appearance, packet.track, policy_)) {
        if (appearance.runtime_actor_id != 0U && appearance.runtime_actor_id != packet.track.actor_id) {
            reset_track();
            return bypass(SignalDisposition::bypass_wrong_actor, rate);
        }
        if (stable_) {
            stable_->rejected_since_accept = true;
            stable_->recovery_matches = 0U;
        }
        return bypass(SignalDisposition::bypass_appearance, rate);
    }

    const auto mouth_contour_bounds = mouth_contour_bounds_from_landmarks(packet.landmarks);
    auto mouth_bounds = padded_mouth_bounds_from_contour(mouth_contour_bounds);
    auto semantic = semantic_mouth_landmarks(packet);
    // The compositing mask deliberately includes feathering outside the raw
    // lip contour. OpenSeeFace detector boxes can end just above that padding,
    // even though every model-produced lip point is still inside the detected
    // face. Bind safety to the unpadded contour and keep the padded mask
    // clamped to the exact source frame.
    if (!valid_rect(mouth_contour_bounds) || !valid_rect(mouth_bounds) ||
        !contains(packet.face_bounds, mouth_contour_bounds) ||
        semantic.left_corner.x >= semantic.right_corner.x ||
        semantic.upper_lip_center.y >= semantic.lower_lip_center.y) {
        reset_track();
        return bypass(SignalDisposition::bypass_unsafe_roi, rate);
    }

    const Nanoseconds minimum_interval = 1'000'000'000LL / static_cast<Nanoseconds>(rate);
    if (stable_ && stable_->track == packet.track) {
        if (packet.measured_at_ns <= stable_->last_accepted_at_ns ||
            packet.measured_at_ns - stable_->last_accepted_at_ns < minimum_interval) {
            return bypass(SignalDisposition::bypass_rate_limited, rate);
        }
        const double previous_area = area(stable_->mouth_bounds);
        const double area_delta = previous_area > std::numeric_limits<double>::epsilon()
            ? std::abs(area(mouth_bounds) - previous_area) / previous_area
            : std::numeric_limits<double>::infinity();
        if (center_distance_over_face(mouth_bounds, stable_->mouth_bounds, packet.face_bounds) >
                std::min(policy_.maximum_center_motion_face_fraction,
                         hard_maximum_center_motion_face_fraction) ||
            area_delta > std::min(policy_.maximum_area_delta_fraction,
                                  hard_maximum_area_delta_fraction)) {
            stable_->rejected_since_accept = true;
            stable_->recovery_matches = 0U;
            return bypass(SignalDisposition::bypass_appearance, rate);
        }
        if (stable_->rejected_since_accept) {
            ++stable_->recovery_matches;
            if (stable_->recovery_matches < std::max(1U, policy_.recovery_matches_after_rejection)) {
                return bypass(SignalDisposition::bypass_appearance, rate);
            }
            stable_->rejected_since_accept = false;
            stable_->recovery_matches = 0U;
        }
        const double alpha = std::clamp(policy_.smoothing_alpha, 0.0, 1.0);
        mouth_bounds = smooth(mouth_bounds, stable_->mouth_bounds, alpha);
        semantic.left_corner = smooth(semantic.left_corner, stable_->mouth_landmarks.left_corner, alpha);
        semantic.right_corner = smooth(semantic.right_corner, stable_->mouth_landmarks.right_corner, alpha);
        semantic.upper_lip_center = smooth(semantic.upper_lip_center,
                                           stable_->mouth_landmarks.upper_lip_center, alpha);
        semantic.lower_lip_center = smooth(semantic.lower_lip_center,
                                           stable_->mouth_landmarks.lower_lip_center, alpha);
        for (std::size_t index = 0; index < semantic.contour.size(); ++index) {
            semantic.contour[index] = smooth(semantic.contour[index],
                                             stable_->mouth_landmarks.contour[index], alpha);
        }
    } else {
        ++latch_generation_;
        stable_.reset();
    }

    TrackingEvidence tracking{};
    tracking.track = packet.track;
    tracking.frame = packet.frame;
    tracking.face_bounds = packet.face_bounds;
    tracking.mouth_bounds = mouth_bounds;
    tracking.mouth_landmarks = semantic;
    tracking.pose = packet.pose;
    tracking.face_confidence = packet.detector_confidence;
    tracking.landmark_confidence = packet.landmark_confidence;
    tracking.visibility_ratio = packet.visibility_ratio;
    tracking.mouth_occluded = false;
    tracking.measured_at_ns = packet.measured_at_ns;

    stable_ = StableState{
        packet.track,
        mouth_bounds,
        semantic,
        packet.measured_at_ns,
        0U,
        false,
    };
    return {SignalDisposition::accepted, std::move(tracking), rate, latch_generation_};
}

bool OpenSeeFaceSignalAdapter::cancel_to(const std::uint64_t generation) noexcept {
    if (generation <= active_generation_) {
        return false;
    }
    active_generation_ = generation;
    reset_track();
    return true;
}

void OpenSeeFaceSignalAdapter::reset_track() noexcept {
    if (stable_) {
        stable_.reset();
        ++latch_generation_;
    }
}

std::uint64_t OpenSeeFaceSignalAdapter::active_generation() const noexcept {
    return active_generation_;
}

bool OpenSeeFaceSignalAdapter::appearance_latched() const noexcept {
    return stable_.has_value() && !stable_->rejected_since_accept;
}

SignalDecision OpenSeeFaceSignalAdapter::bypass(const SignalDisposition disposition,
                                                const std::uint32_t rate) const noexcept {
    return {disposition, std::nullopt, rate, latch_generation_};
}

} // namespace npc::mouth
