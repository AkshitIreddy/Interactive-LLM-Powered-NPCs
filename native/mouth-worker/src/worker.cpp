#include "npc/mouth_worker/worker.hpp"

#include "npc/mouth_worker/compositor.hpp"

#include <algorithm>
#include <cmath>
#include <utility>

namespace npc::mouth {
namespace {

constexpr Nanoseconds hard_maximum_source_age_ns = 150'000'000;
constexpr Nanoseconds hard_maximum_tracking_age_ns = 150'000'000;
constexpr Nanoseconds hard_maximum_audio_skew_ns = 80'000'000;
constexpr double hard_minimum_face_confidence = 0.70;
constexpr double hard_minimum_landmark_confidence = 0.82;
constexpr double hard_minimum_semantic_landmark_confidence = 0.55;
constexpr double hard_minimum_visibility_ratio = 0.72;
constexpr double hard_maximum_absolute_yaw_degrees = 35.0;
constexpr double hard_maximum_absolute_pitch_degrees = 25.0;
constexpr double hard_maximum_absolute_roll_degrees = 45.0;
constexpr double hard_maximum_mouth_width_fraction = 0.45;
constexpr double hard_maximum_mouth_height_fraction = 0.35;
constexpr double hard_maximum_mouth_area_fraction = 0.12;
constexpr double hard_maximum_mask_feather_face_fraction = 0.08;
constexpr Nanoseconds smoothing_continuity_limit_ns = 180'000'000;

[[nodiscard]] double blend_toward(const double current,
                                  const double target,
                                  const double rising_alpha,
                                  const double falling_alpha) noexcept {
    const double alpha = target >= current ? rising_alpha : falling_alpha;
    return current + (target - current) * alpha;
}

[[nodiscard]] bool finite_rect(const NormalizedRect& rect) noexcept {
    return std::isfinite(rect.x) && std::isfinite(rect.y) &&
           std::isfinite(rect.width) && std::isfinite(rect.height);
}

[[nodiscard]] bool normalized_rect(const NormalizedRect& rect) noexcept {
    return finite_rect(rect) && rect.x >= 0.0 && rect.y >= 0.0 &&
           rect.width > 0.0 && rect.height > 0.0 &&
           rect.right() <= 1.0 && rect.bottom() <= 1.0;
}

[[nodiscard]] bool finite_pose(const HeadPoseDegrees& pose) noexcept {
    return std::isfinite(pose.yaw) && std::isfinite(pose.pitch) && std::isfinite(pose.roll);
}

[[nodiscard]] bool finite_confidences(const TrackingEvidence& tracking) noexcept {
    const auto probability = [](const double value) {
        return std::isfinite(value) && value >= 0.0 && value <= 1.0;
    };
    return probability(tracking.face_confidence) &&
           probability(tracking.landmark_confidence) &&
           probability(tracking.visibility_ratio);
}

[[nodiscard]] bool valid_landmark(const NormalizedLandmark& landmark,
                                  const NormalizedRect& mouth_bounds,
                                  const double minimum_confidence) noexcept {
    return std::isfinite(landmark.x) && std::isfinite(landmark.y) &&
           std::isfinite(landmark.confidence) && landmark.confidence <= 1.0 &&
           landmark.x >= mouth_bounds.x &&
           landmark.x <= mouth_bounds.right() && landmark.y >= mouth_bounds.y &&
           landmark.y <= mouth_bounds.bottom() && landmark.confidence >= minimum_confidence;
}

[[nodiscard]] bool valid_landmarks(const MouthLandmarks& landmarks,
                                   const NormalizedRect& mouth_bounds,
                                   const double minimum_confidence) noexcept {
    return landmarks.schema_version == 1U && landmarks.provider_instance_id != 0U &&
           valid_landmark(landmarks.left_corner, mouth_bounds, minimum_confidence) &&
           valid_landmark(landmarks.right_corner, mouth_bounds, minimum_confidence) &&
           valid_landmark(landmarks.upper_lip_center, mouth_bounds, minimum_confidence) &&
           valid_landmark(landmarks.lower_lip_center, mouth_bounds, minimum_confidence) &&
           landmarks.left_corner.x < landmarks.right_corner.x &&
           landmarks.upper_lip_center.y < landmarks.lower_lip_center.y;
}

[[nodiscard]] bool landmark_inside(const NormalizedLandmark& landmark,
                                   const NormalizedRect& bounds) noexcept {
    return landmark.x >= bounds.x && landmark.x <= bounds.right() &&
           landmark.y >= bounds.y && landmark.y <= bounds.bottom();
}

[[nodiscard]] bool padded_mouth_is_face_bound(const TrackingEvidence& tracking) noexcept {
    const auto& face = tracking.face_bounds;
    const auto& mouth = tracking.mouth_bounds;
    const double margin_x = face.width * hard_maximum_mask_feather_face_fraction;
    const double margin_y = face.height * hard_maximum_mask_feather_face_fraction;
    const double mouth_center_x = mouth.x + mouth.width * 0.5;
    const double mouth_center_y = mouth.y + mouth.height * 0.5;
    const auto& landmarks = tracking.mouth_landmarks;
    return mouth.x >= std::max(0.0, face.x - margin_x) &&
           mouth.y >= std::max(0.0, face.y - margin_y) &&
           mouth.right() <= std::min(1.0, face.right() + margin_x) &&
           mouth.bottom() <= std::min(1.0, face.bottom() + margin_y) &&
           mouth_center_x >= face.x && mouth_center_x <= face.right() &&
           mouth_center_y >= face.y && mouth_center_y <= face.bottom() &&
           landmark_inside(landmarks.left_corner, face) &&
           landmark_inside(landmarks.right_corner, face) &&
           landmark_inside(landmarks.upper_lip_center, face) &&
           landmark_inside(landmarks.lower_lip_center, face);
}

[[nodiscard]] std::uint64_t absolute_delta(const Nanoseconds first,
                                           const Nanoseconds second) noexcept {
    return first >= second
        ? static_cast<std::uint64_t>(first - second)
        : static_cast<std::uint64_t>(second - first);
}

[[nodiscard]] bool valid_drive(const MouthDrive& drive) noexcept {
    if (drive.clock.segment_id == 0U || drive.clock.sample_rate < 8'000U ||
        drive.clock.sample_rate > 192'000U || drive.clock.channels == 0U ||
        drive.clock.channels > 8U) {
        return false;
    }
    switch (drive.kind) {
    case DriveKind::explicit_coefficients:
        return true;
    case DriveKind::timed_viseme:
        return static_cast<std::uint8_t>(drive.viseme) <=
               static_cast<std::uint8_t>(Viseme::spread_vowel) &&
               std::isfinite(drive.viseme_strength);
    case DriveKind::pcm_window: {
        const auto channels = static_cast<std::size_t>(drive.clock.channels);
        const auto maximum_frames = static_cast<std::size_t>(drive.clock.sample_rate / 5U);
        return !drive.interleaved_pcm.empty() &&
               drive.interleaved_pcm.size() % channels == 0U &&
               drive.interleaved_pcm.size() / channels <= maximum_frames;
    }
    }
    return false;
}

} // namespace

ReferenceMouthWorker::ReferenceMouthWorker(const std::uint64_t initial_generation,
                                           WorkerPolicy policy)
    : active_generation_(initial_generation), policy_(std::move(policy)) {}

bool ReferenceMouthWorker::submit(WorkItem item) {
    ++stats_.submitted;
    if (item.track.cancellation_generation != active_generation_) {
        ++stats_.cancelled;
        return false;
    }
    if (pending_) {
        ++stats_.replaced_before_processing;
    }
    pending_ = std::move(item);
    return true;
}

bool ReferenceMouthWorker::cancel_to(const std::uint64_t new_generation) noexcept {
    if (new_generation <= active_generation_) {
        return false;
    }
    active_generation_ = new_generation;
    if (pending_) {
        pending_.reset();
        ++stats_.cancelled;
    }
    reset_pcm_smoothing();
    return true;
}

ProcessResult ReferenceMouthWorker::process_latest(const FrameIdentity& current_frame,
                                                   const Nanoseconds now_ns) {
    if (!pending_) {
        return bypass(Disposition::bypass_no_work);
    }
    WorkItem item = std::move(*pending_);
    pending_.reset();
    ++stats_.processed;

    const auto validation = validate(item, current_frame, now_ns);
    if (validation != Disposition::residual_ready) {
        return bypass(validation);
    }

    MouthCoefficients coefficients{};
    switch (item.drive.kind) {
    case DriveKind::explicit_coefficients:
        coefficients = item.drive.coefficients;
        break;
    case DriveKind::timed_viseme:
        coefficients = coefficients_for_viseme(item.drive.viseme, item.drive.viseme_strength);
        break;
    case DriveKind::pcm_window:
        coefficients = coefficients_from_pcm(item.drive.interleaved_pcm,
                                             item.drive.clock.sample_rate,
                                             item.drive.clock.channels);
        coefficients = smooth_pcm_coefficients(coefficients, item);
        break;
    }

    auto residual = compose_current_frame_residual(item.source, item.track, item.tracking,
                                                   coefficients, now_ns);
    if (residual.premultiplied_bgra.empty()) {
        return bypass(Disposition::bypass_unsafe_bounds);
    }
    ++stats_.residuals;
    return {Disposition::residual_ready, std::move(residual)};
}

MouthCoefficients ReferenceMouthWorker::smooth_pcm_coefficients(
    const MouthCoefficients& target,
    const WorkItem& item) noexcept {
    const bool continuous = smoothed_pcm_coefficients_.has_value() &&
                            smoothed_pcm_track_.has_value() &&
                            *smoothed_pcm_track_ == item.track &&
                            smoothed_pcm_segment_id_ == item.drive.clock.segment_id &&
                            item.source.identity.captured_at_ns > smoothed_pcm_at_ns_ &&
                            item.source.identity.captured_at_ns - smoothed_pcm_at_ns_ <=
                                smoothing_continuity_limit_ns;
    if (!continuous) {
        smoothed_pcm_coefficients_ = target;
    } else {
        auto& value = *smoothed_pcm_coefficients_;
        value.jaw_open = blend_toward(value.jaw_open, target.jaw_open, 0.74, 0.48);
        // Closing should remain decisive enough for bilabials and silence;
        // opening the lip seal may be quicker without producing chatter.
        value.lip_close = blend_toward(value.lip_close, target.lip_close, 0.66, 0.78);
        value.funnel = blend_toward(value.funnel, target.funnel, 0.68, 0.44);
        value.pucker = blend_toward(value.pucker, target.pucker, 0.68, 0.44);
        value.smile_left = blend_toward(value.smile_left, target.smile_left, 0.62, 0.42);
        value.smile_right = blend_toward(value.smile_right, target.smile_right, 0.62, 0.42);
        value.upper_lip_raise = blend_toward(
            value.upper_lip_raise, target.upper_lip_raise, 0.68, 0.46);
        value.lower_lip_depress = blend_toward(
            value.lower_lip_depress, target.lower_lip_depress, 0.72, 0.48);
    }
    smoothed_pcm_track_ = item.track;
    smoothed_pcm_segment_id_ = item.drive.clock.segment_id;
    smoothed_pcm_at_ns_ = item.source.identity.captured_at_ns;
    return *smoothed_pcm_coefficients_;
}

void ReferenceMouthWorker::reset_pcm_smoothing() noexcept {
    smoothed_pcm_coefficients_.reset();
    smoothed_pcm_track_.reset();
    smoothed_pcm_segment_id_ = 0U;
    smoothed_pcm_at_ns_ = 0;
}

std::uint64_t ReferenceMouthWorker::active_generation() const noexcept {
    return active_generation_;
}

bool ReferenceMouthWorker::has_pending_work() const noexcept {
    return pending_.has_value();
}

const WorkerStats& ReferenceMouthWorker::stats() const noexcept {
    return stats_;
}

ProcessResult ReferenceMouthWorker::bypass(const Disposition disposition) noexcept {
    if (disposition != Disposition::bypass_no_work) {
        ++stats_.bypasses;
    }
    return {disposition, {}};
}

Disposition ReferenceMouthWorker::validate(const WorkItem& item,
                                           const FrameIdentity& current_frame,
                                           const Nanoseconds now_ns) const noexcept {
    if (item.track.cancellation_generation != active_generation_) {
        return Disposition::bypass_cancelled;
    }
    if (item.track.actor_id == 0U || item.track.track_id == 0U || item.track.track_epoch == 0U ||
        current_frame.sequence == 0U || current_frame.geometry_epoch == 0U ||
        current_frame.captured_at_ns <= 0 || now_ns <= 0 ||
        item.tracking.measured_at_ns <= 0 || item.drive.clock.playback_at_ns <= 0) {
        return Disposition::bypass_invalid_tracking;
    }
    if (item.source.identity != current_frame || item.tracking.frame != current_frame ||
        item.tracking.track != item.track) {
        return Disposition::bypass_wrong_frame;
    }
    if (!valid_cpu_frame(item.source)) {
        return Disposition::bypass_invalid_frame;
    }
    if (item.source.lease.expires_at_ns < now_ns) {
        return Disposition::bypass_expired_lease;
    }
    if (item.deadline_ns < now_ns) {
        return Disposition::bypass_deadline;
    }
    if (item.source.identity.captured_at_ns > now_ns ||
        now_ns - item.source.identity.captured_at_ns >
            std::min(policy_.maximum_source_age_ns, hard_maximum_source_age_ns)) {
        return Disposition::bypass_stale_frame;
    }
    if (item.tracking.measured_at_ns > now_ns ||
        now_ns - item.tracking.measured_at_ns >
            std::min(policy_.maximum_tracking_age_ns, hard_maximum_tracking_age_ns)) {
        return Disposition::bypass_invalid_tracking;
    }
    if (!valid_drive(item.drive) ||
        item.drive.clock.stream_generation != item.track.cancellation_generation ||
        absolute_delta(item.drive.clock.playback_at_ns, item.source.identity.captured_at_ns) >
            static_cast<std::uint64_t>(std::max<Nanoseconds>(
                0, std::min(policy_.maximum_audio_skew_ns, hard_maximum_audio_skew_ns)))) {
        return Disposition::bypass_audio_clock;
    }
    if (!normalized_rect(item.tracking.face_bounds) ||
        !normalized_rect(item.tracking.mouth_bounds) ||
        !padded_mouth_is_face_bound(item.tracking) ||
        !valid_landmarks(item.tracking.mouth_landmarks, item.tracking.mouth_bounds,
                         std::max(policy_.minimum_semantic_landmark_confidence,
                                  hard_minimum_semantic_landmark_confidence)) ||
        !finite_pose(item.tracking.pose) || !finite_confidences(item.tracking)) {
        return Disposition::bypass_invalid_tracking;
    }
    if (item.tracking.mouth_occluded) {
        return Disposition::bypass_occluded;
    }
    if (std::abs(item.tracking.pose.yaw) >
            std::min(policy_.maximum_absolute_yaw_degrees,
                     hard_maximum_absolute_yaw_degrees) ||
        std::abs(item.tracking.pose.pitch) >
            std::min(policy_.maximum_absolute_pitch_degrees,
                     hard_maximum_absolute_pitch_degrees) ||
        std::abs(item.tracking.pose.roll) >
            std::min(policy_.maximum_absolute_roll_degrees,
                     hard_maximum_absolute_roll_degrees)) {
        return Disposition::bypass_pose;
    }
    if (item.tracking.face_confidence <
            std::max(policy_.minimum_face_confidence, hard_minimum_face_confidence) ||
        item.tracking.landmark_confidence <
            std::max(policy_.minimum_landmark_confidence, hard_minimum_landmark_confidence) ||
        item.tracking.visibility_ratio <
            std::max(policy_.minimum_visibility_ratio, hard_minimum_visibility_ratio)) {
        return Disposition::bypass_low_confidence;
    }
    const auto& bounds = item.tracking.mouth_bounds;
    if (bounds.width > std::min(policy_.maximum_mouth_width_fraction,
                                hard_maximum_mouth_width_fraction) ||
        bounds.height > std::min(policy_.maximum_mouth_height_fraction,
                                 hard_maximum_mouth_height_fraction) ||
        bounds.width * bounds.height > std::min(policy_.maximum_mouth_area_fraction,
                                                hard_maximum_mouth_area_fraction)) {
        return Disposition::bypass_unsafe_bounds;
    }
    return Disposition::residual_ready;
}

} // namespace npc::mouth
