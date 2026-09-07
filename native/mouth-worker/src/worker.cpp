#include "npc/mouth_worker/worker.hpp"

#include "npc/mouth_worker/compositor.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
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
constexpr std::size_t minimum_atlas_states = 4U;
constexpr std::size_t maximum_atlas_states = 64U;
constexpr std::uint32_t maximum_atlas_dimension = 512U;
constexpr Nanoseconds timed_coarticulation_tau_ns = 42'000'000;
constexpr Nanoseconds estimated_coarticulation_tau_ns = 62'000'000;

[[nodiscard]] double exponential_alpha(const Nanoseconds elapsed_ns,
                                       const Nanoseconds tau_ns) noexcept {
    if (elapsed_ns <= 0 || tau_ns <= 0) {
        return 1.0;
    }
    return 1.0 - std::exp(-static_cast<double>(elapsed_ns) /
                          static_cast<double>(tau_ns));
}

[[nodiscard]] double blend_toward(const double current,
                                  const double target,
                                  const double alpha) noexcept {
    return current + (target - current) * alpha;
}

[[nodiscard]] bool contact_topology(const MouthCoefficients& value) noexcept {
    return value.lip_close >= 0.62 && value.jaw_open <= 0.32;
}

[[nodiscard]] bool exact_contact_closure(
    const MouthCoefficients& value) noexcept {
    return value.lip_close >= 0.88 && value.jaw_open <= 0.04;
}

[[nodiscard]] bool neutral_reference_coefficients(
    const MouthCoefficients& value) noexcept {
    return value.jaw_open <= 0.05 && value.lip_close >= 0.95 &&
           value.funnel <= 0.05 && value.pucker <= 0.05 &&
           value.smile_left <= 0.05 && value.smile_right <= 0.05 &&
           value.upper_lip_raise <= 0.05 && value.lower_lip_depress <= 0.05;
}

[[nodiscard]] bool exact_silence_coefficients(
    const MouthCoefficients& value) noexcept {
    return value.jaw_open <= 1.0e-6 && value.lip_close >= 1.0 - 1.0e-6 &&
           value.funnel <= 1.0e-6 && value.pucker <= 1.0e-6 &&
           value.smile_left <= 1.0e-6 && value.smile_right <= 1.0e-6 &&
           value.upper_lip_raise <= 1.0e-6 &&
           value.lower_lip_depress <= 1.0e-6;
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
    if ((landmarks.schema_version != 1U && landmarks.schema_version != 2U) ||
        landmarks.provider_instance_id == 0U ||
        !valid_landmark(landmarks.left_corner, mouth_bounds, minimum_confidence) ||
        !valid_landmark(landmarks.right_corner, mouth_bounds, minimum_confidence) ||
        !valid_landmark(landmarks.upper_lip_center, mouth_bounds, minimum_confidence) ||
        !valid_landmark(landmarks.lower_lip_center, mouth_bounds, minimum_confidence) ||
        landmarks.left_corner.x >= landmarks.right_corner.x ||
        landmarks.upper_lip_center.y >= landmarks.lower_lip_center.y) {
        return false;
    }
    if (landmarks.schema_version == 1U) {
        return landmarks.contour_points == 0U;
    }
    if (landmarks.contour_points != landmarks.contour.size()) {
        return false;
    }
    return std::all_of(landmarks.contour.begin(), landmarks.contour.end(),
                       [&](const NormalizedLandmark& point) {
                           return valid_landmark(point, mouth_bounds, minimum_confidence);
                       });
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
        drive.clock.channels > 8U || drive.clock.sample_count == 0U ||
        drive.clock.first_sample_index >
            std::numeric_limits<std::uint64_t>::max() - drive.clock.sample_count) {
        return false;
    }
    const auto end_sample_index =
        drive.clock.first_sample_index + drive.clock.sample_count;
    if (drive.clock.playback_sample_index < drive.clock.first_sample_index ||
        drive.clock.playback_sample_index >= end_sample_index) {
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
               drive.interleaved_pcm.size() / channels <= maximum_frames &&
               drive.interleaved_pcm.size() / channels == drive.clock.sample_count;
    }
    }
    return false;
}

[[nodiscard]] bool unit_coefficient(const double value) noexcept {
    return std::isfinite(value) && value >= 0.0 && value <= 1.0;
}

[[nodiscard]] bool valid_coefficients(const MouthCoefficients& coefficients) noexcept {
    return unit_coefficient(coefficients.jaw_open) &&
           unit_coefficient(coefficients.lip_close) &&
           unit_coefficient(coefficients.funnel) &&
           unit_coefficient(coefficients.pucker) &&
           unit_coefficient(coefficients.smile_left) &&
           unit_coefficient(coefficients.smile_right) &&
           unit_coefficient(coefficients.upper_lip_raise) &&
           unit_coefficient(coefficients.lower_lip_depress);
}

[[nodiscard]] bool valid_atlas_patch_for_install(const CanonicalMouthPatch& patch) noexcept {
    if (patch.width < 16U || patch.height < 16U ||
        patch.width > maximum_atlas_dimension || patch.height > maximum_atlas_dimension ||
        patch.stride_bytes < patch.width * 4U ||
        patch.stride_bytes > maximum_atlas_dimension * 4U ||
        patch.premultiplied_bgra.size() !=
            static_cast<std::size_t>(patch.stride_bytes) * patch.height ||
        !finite_pose(patch.enrolled_pose)) {
        return false;
    }
    for (std::size_t offset = 0U; offset < patch.premultiplied_bgra.size(); offset += 4U) {
        const auto alpha = patch.premultiplied_bgra[offset + 3U];
        if (patch.premultiplied_bgra[offset] > alpha ||
            patch.premultiplied_bgra[offset + 1U] > alpha ||
            patch.premultiplied_bgra[offset + 2U] > alpha) {
            return false;
        }
    }
    return true;
}

[[nodiscard]] double coefficient_distance(const MouthCoefficients& first,
                                          const MouthCoefficients& second) noexcept {
    const auto squared = [](const double value) { return value * value; };
    // Jaw and closure dominate perceived timing. Shape dimensions retain
    // enough weight to distinguish rounded, spread, and labiodental states.
    return 2.8 * squared(first.jaw_open - second.jaw_open) +
           3.2 * squared(first.lip_close - second.lip_close) +
           1.5 * squared(first.funnel - second.funnel) +
           1.2 * squared(first.pucker - second.pucker) +
           0.7 * squared(first.smile_left - second.smile_left) +
           0.7 * squared(first.smile_right - second.smile_right) +
           0.8 * squared(first.upper_lip_raise - second.upper_lip_raise) +
           1.0 * squared(first.lower_lip_depress - second.lower_lip_depress);
}

struct AtlasSelection final {
    std::size_t index{};
    double distance{std::numeric_limits<double>::infinity()};
};

[[nodiscard]] double atlas_state_distance(const MouthAtlasState& state,
                                          const MouthCoefficients& target,
                                          const HeadPoseDegrees& target_pose) noexcept {
    const double yaw_delta = (state.appearance.enrolled_pose.yaw - target_pose.yaw) / 35.0;
    const double pitch_delta = (state.appearance.enrolled_pose.pitch - target_pose.pitch) / 25.0;
    const double roll_delta = (state.appearance.enrolled_pose.roll - target_pose.roll) / 45.0;
    // Teeth-bearing/open textures must not be averaged into contact states.
    // The finite penalty still permits a sparse atlas to fail softly to its
    // nearest available observation instead of producing no residual.
    const double topology_penalty =
        contact_topology(state.coefficients) == contact_topology(target) ? 0.0 : 20.0;
    return coefficient_distance(state.coefficients, target) + topology_penalty +
           0.45 * yaw_delta * yaw_delta + 0.30 * pitch_delta * pitch_delta +
           0.10 * roll_delta * roll_delta;
}

[[nodiscard]] AtlasSelection select_atlas_state(
    const CharacterMouthAtlas& atlas,
    const MouthCoefficients& target,
    const HeadPoseDegrees& target_pose) noexcept {
    std::size_t primary_index{};
    double primary_distance = std::numeric_limits<double>::infinity();
    for (std::size_t index = 0U; index < atlas.states.size(); ++index) {
        const auto& state = atlas.states[index];
        const double distance = atlas_state_distance(state, target, target_pose);
        if (distance < primary_distance) {
            primary_distance = distance;
            primary_index = index;
        }
    }
    if (!std::isfinite(primary_distance)) {
        return {};
    }
    return {primary_index, primary_distance};
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
    atlas_.reset();
    reset_drive_smoothing();
    reset_atlas_selection();
    return true;
}

bool ReferenceMouthWorker::install_atlas(CharacterMouthAtlas atlas) {
    if ((atlas.schema_version != 1U && atlas.schema_version != 2U &&
         atlas.schema_version != 3U) ||
        atlas.cancellation_generation != active_generation_ ||
        atlas.actor_id == 0U || atlas.identity_revision == 0U ||
        atlas.states.size() < minimum_atlas_states ||
        atlas.states.size() > maximum_atlas_states ||
        (atlas.schema_version == 3U && atlas.states.size() > 16U)) {
        return false;
    }
    const auto width = atlas.states.front().appearance.width;
    const auto height = atlas.states.front().appearance.height;
    const auto stride = atlas.states.front().appearance.stride_bytes;
    const auto expected_representation = atlas.schema_version == 2U
        ? MouthPatchRepresentation::normalized_oral_interior_v1
        : atlas.schema_version == 3U
            ? MouthPatchRepresentation::photometric_full_lip_reference_v1
            : MouthPatchRepresentation::full_lip_observation_v1;
    const bool all_valid = std::all_of(
        atlas.states.begin(), atlas.states.end(),
        [width, height, stride, expected_representation](const MouthAtlasState& state) {
            return state.appearance.representation == expected_representation &&
                   valid_coefficients(state.coefficients) &&
                   valid_atlas_patch_for_install(state.appearance) &&
                   state.appearance.width == width &&
                   state.appearance.height == height &&
                   state.appearance.stride_bytes == stride;
        });
    if (!all_valid) {
        return false;
    }
    if (atlas.schema_version == 3U) {
        const auto& neutral = atlas.states.front();
        if (!neutral_reference_coefficients(neutral.coefficients)) return false;
        std::size_t transparent_pixels{};
        std::size_t opaque_pixels{};
        const auto& reference_pixels = neutral.appearance.premultiplied_bgra;
        for (std::uint32_t y = 0U; y < neutral.appearance.height; ++y) {
            for (std::uint32_t x = 0U; x < neutral.appearance.width; ++x) {
                const auto offset = static_cast<std::size_t>(y) *
                                        neutral.appearance.stride_bytes +
                                    static_cast<std::size_t>(x) * 4U;
                const auto alpha = reference_pixels[offset + 3U];
                transparent_pixels += alpha == 0U ? 1U : 0U;
                opaque_pixels += alpha >= 250U ? 1U : 0U;
            }
        }
        if (transparent_pixels < 16U || opaque_pixels < 16U) return false;
        const auto& neutral_pose = neutral.appearance.enrolled_pose;
        for (const auto& state : atlas.states) {
            if (state.appearance.enrolled_pose.yaw != neutral_pose.yaw ||
                state.appearance.enrolled_pose.pitch != neutral_pose.pitch ||
                state.appearance.enrolled_pose.roll != neutral_pose.roll) {
                return false;
            }
            const auto& pixels = state.appearance.premultiplied_bgra;
            for (std::size_t offset = 3U; offset < pixels.size(); offset += 4U) {
                if (pixels[offset] != reference_pixels[offset]) return false;
            }
        }
    }
    atlas_ = std::move(atlas);
    reset_drive_smoothing();
    reset_atlas_selection();
    return true;
}

void ReferenceMouthWorker::clear_atlas() noexcept {
    atlas_.reset();
    reset_atlas_selection();
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
        break;
    }
    const auto target_coefficients = coefficients;
    coefficients = smooth_drive_coefficients(coefficients, item);

    ResidualPatch residual{};
    if (atlas_.has_value() &&
        atlas_->cancellation_generation == item.track.cancellation_generation &&
        atlas_->actor_id == item.track.actor_id) {
        const bool pure_silence = exact_silence_coefficients(target_coefficients);
        if (pure_silence ||
            (atlas_->schema_version != 3U && exact_contact_closure(coefficients))) {
            // A bilabial/silence cue is a hard anatomical constraint. Use the
            // current-frame compositor to close an already-open source mouth,
            // while excluding photographed teeth and prior atlas textures.
            residual = compose_current_frame_residual(
                item.source, item.track, item.tracking, coefficients, now_ns);
        } else {
            // Schema-three reference textures are the pixel realization of the
            // causal coefficient trajectory. Selecting them from the raw cue
            // bypasses the drive smoother and can chatter between full-lip
            // observations even though the residual metadata is continuous.
            // Legacy atlas selection remains unchanged for wire-compatible
            // behavior; exact contact targets already reset the coefficient
            // smoother above and topology boundaries still reset appearance.
            const auto& selection_coefficients = atlas_->schema_version == 3U
                ? coefficients
                : target_coefficients;
            const auto selection = select_atlas_state(
                *atlas_, selection_coefficients, item.tracking.pose);
            if (std::isfinite(selection.distance) && selection.index < atlas_->states.size()) {
                const auto& state = atlas_->states[selection.index];
                const auto& appearance = smooth_atlas_appearance(state, item);
                if (atlas_->schema_version == 3U) {
                    residual = compose_photometric_atlas_residual(
                        item.source, item.track, item.tracking,
                        atlas_->states.front().appearance, appearance,
                        coefficients, now_ns);
                } else {
                    residual = compose_atlas_residual(item.source, item.track, item.tracking,
                                                      appearance,
                                                      coefficients, now_ns);
                }
            }
        }
    } else {
        residual = compose_current_frame_residual(item.source, item.track, item.tracking,
                                                  coefficients, now_ns);
    }
    if (residual.premultiplied_bgra.empty()) {
        return bypass(Disposition::bypass_unsafe_bounds);
    }
    residual.audio_clock = item.drive.clock;
    ++stats_.residuals;
    return {Disposition::residual_ready, std::move(residual)};
}

MouthCoefficients ReferenceMouthWorker::smooth_drive_coefficients(
    const MouthCoefficients& target,
    const WorkItem& item) noexcept {
    const auto source_at = item.source.identity.captured_at_ns;
    const auto playback_at = item.drive.clock.playback_at_ns;
    const bool continuous = smoothed_drive_coefficients_.has_value() &&
                            smoothed_drive_track_.has_value() &&
                            *smoothed_drive_track_ == item.track &&
                            smoothed_drive_segment_id_ == item.drive.clock.segment_id &&
                            source_at > smoothed_drive_source_at_ns_ &&
                            source_at - smoothed_drive_source_at_ns_ <=
                                smoothing_continuity_limit_ns &&
                            playback_at > smoothed_drive_playback_at_ns_ &&
                            playback_at - smoothed_drive_playback_at_ns_ <=
                                smoothing_continuity_limit_ns;
    if (!continuous || exact_contact_closure(target)) {
        // Contact closure is a constraint, not a value to ease toward. This
        // lets the current-frame compositor close the newest source mouth
        // immediately while ordinary vowel geometry remains time-continuous.
        smoothed_drive_coefficients_ = target;
    } else {
        auto& value = *smoothed_drive_coefficients_;
        const auto elapsed = playback_at - smoothed_drive_playback_at_ns_;
        const auto ordinary_tau = item.drive.kind == DriveKind::timed_viseme
            ? timed_coarticulation_tau_ns
            : estimated_coarticulation_tau_ns;
        const auto opening_tau = item.drive.kind == DriveKind::timed_viseme
            ? 34'000'000LL
            : 46'000'000LL;
        const auto closing_tau = item.drive.kind == DriveKind::timed_viseme
            ? 22'000'000LL
            : 32'000'000LL;
        const double ordinary_alpha = exponential_alpha(elapsed, ordinary_tau);
        const double jaw_alpha = exponential_alpha(
            elapsed, target.jaw_open >= value.jaw_open ? opening_tau : ordinary_tau);
        const double close_alpha = exponential_alpha(
            elapsed, target.lip_close >= value.lip_close ? closing_tau : opening_tau);
        value.jaw_open = blend_toward(value.jaw_open, target.jaw_open, jaw_alpha);
        value.lip_close = blend_toward(value.lip_close, target.lip_close, close_alpha);
        value.funnel = blend_toward(value.funnel, target.funnel, ordinary_alpha);
        value.pucker = blend_toward(value.pucker, target.pucker, ordinary_alpha);
        value.smile_left = blend_toward(value.smile_left, target.smile_left, ordinary_alpha);
        value.smile_right = blend_toward(value.smile_right, target.smile_right, ordinary_alpha);
        value.upper_lip_raise = blend_toward(
            value.upper_lip_raise, target.upper_lip_raise, ordinary_alpha);
        value.lower_lip_depress = blend_toward(
            value.lower_lip_depress, target.lower_lip_depress, ordinary_alpha);
    }
    smoothed_drive_track_ = item.track;
    smoothed_drive_segment_id_ = item.drive.clock.segment_id;
    smoothed_drive_source_at_ns_ = source_at;
    smoothed_drive_playback_at_ns_ = playback_at;
    return *smoothed_drive_coefficients_;
}

const CanonicalMouthPatch& ReferenceMouthWorker::smooth_atlas_appearance(
    const MouthAtlasState& target,
    const WorkItem& item) {
    const auto source_at = item.source.identity.captured_at_ns;
    const auto playback_at = item.drive.clock.playback_at_ns;
    const bool continuous = smoothed_atlas_appearance_.has_value() &&
                            smoothed_atlas_target_coefficients_.has_value() &&
                            smoothed_atlas_track_.has_value() &&
                            *smoothed_atlas_track_ == item.track &&
                            smoothed_atlas_segment_id_ == item.drive.clock.segment_id &&
                            source_at > smoothed_atlas_source_at_ns_ &&
                            source_at - smoothed_atlas_source_at_ns_ <=
                                smoothing_continuity_limit_ns &&
                            playback_at > smoothed_atlas_playback_at_ns_ &&
                            playback_at - smoothed_atlas_playback_at_ns_ <=
                                smoothing_continuity_limit_ns &&
                            smoothed_atlas_appearance_->representation ==
                                target.appearance.representation &&
                            smoothed_atlas_appearance_->width == target.appearance.width &&
                            smoothed_atlas_appearance_->height == target.appearance.height &&
                            smoothed_atlas_appearance_->stride_bytes ==
                                target.appearance.stride_bytes &&
                            smoothed_atlas_appearance_->premultiplied_bgra.size() ==
                                target.appearance.premultiplied_bgra.size();
    const bool compatible_topology = continuous &&
        contact_topology(*smoothed_atlas_target_coefficients_) ==
            contact_topology(target.coefficients);
    if (!compatible_topology) {
        // Contact and open/teeth-bearing observations are never cross-faded.
        // Their boundary is an anatomical event, while all ordinary vowel
        // changes within a topology use a short causal cross-fade.
        smoothed_atlas_appearance_ = target.appearance;
    } else {
        const auto elapsed = playback_at - smoothed_atlas_playback_at_ns_;
        const bool photometric_reference =
            target.appearance.representation ==
                MouthPatchRepresentation::photometric_full_lip_reference_v1;
        // Full-lip reference states carry more high-frequency edge contrast
        // than oral-only states. A slightly longer within-topology blend keeps
        // teeth and lipstick from popping while contact/open boundaries remain
        // immediate and are still never cross-faded.
        const auto tau = photometric_reference
            ? (item.drive.kind == DriveKind::timed_viseme
                   ? 54'000'000LL
                   : 70'000'000LL)
            : item.drive.kind == DriveKind::timed_viseme
                ? timed_coarticulation_tau_ns
                : estimated_coarticulation_tau_ns;
        const double alpha = exponential_alpha(elapsed, tau);
        auto& pixels = smoothed_atlas_appearance_->premultiplied_bgra;
        const auto& target_pixels = target.appearance.premultiplied_bgra;
        for (std::size_t index = 0U; index < pixels.size(); ++index) {
            pixels[index] = static_cast<std::uint8_t>(std::clamp(
                std::lround(blend_toward(
                    static_cast<double>(pixels[index]),
                    static_cast<double>(target_pixels[index]), alpha)),
                0L, 255L));
        }
        smoothed_atlas_appearance_->enrolled_pose = target.appearance.enrolled_pose;
    }
    smoothed_atlas_target_coefficients_ = target.coefficients;
    smoothed_atlas_track_ = item.track;
    smoothed_atlas_segment_id_ = item.drive.clock.segment_id;
    smoothed_atlas_source_at_ns_ = source_at;
    smoothed_atlas_playback_at_ns_ = playback_at;
    return *smoothed_atlas_appearance_;
}

void ReferenceMouthWorker::reset_drive_smoothing() noexcept {
    smoothed_drive_coefficients_.reset();
    smoothed_drive_track_.reset();
    smoothed_drive_segment_id_ = 0U;
    smoothed_drive_source_at_ns_ = 0;
    smoothed_drive_playback_at_ns_ = 0;
}

void ReferenceMouthWorker::reset_atlas_selection() noexcept {
    smoothed_atlas_appearance_.reset();
    smoothed_atlas_target_coefficients_.reset();
    smoothed_atlas_track_.reset();
    smoothed_atlas_segment_id_ = 0U;
    smoothed_atlas_source_at_ns_ = 0;
    smoothed_atlas_playback_at_ns_ = 0;
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
    if (!valid_drive(item.drive) || item.drive.clock.playback_at_ns > now_ns ||
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
