#pragma once

#include "npc/mouth_worker/types.hpp"

#include <array>
#include <cstdint>
#include <optional>
#include <string_view>

namespace npc::mouth {

inline constexpr std::size_t openseeface_landmark_count_v1 = 66U;

// This is the exact, typed boundary emitted by the qualified OpenSeeFace
// MNV3 + lm_model1 signal pack. The worker never loads arbitrary Python or
// accepts an opaque tracker blob. Model-native indices are converted here.
struct OpenSeeFaceLandmarkPacketV1 {
    std::uint32_t schema_version{1};
    std::uint64_t provider_instance_id{};
    TrackBinding track;
    FrameIdentity frame;
    std::uint64_t source_frame_qpc{};
    std::uint64_t qpc_frequency{};
    NormalizedRect face_bounds;
    std::array<NormalizedLandmark, openseeface_landmark_count_v1> landmarks;
    HeadPoseDegrees pose;
    double detector_confidence{};
    double landmark_confidence{};
    double visibility_ratio{};
    bool mouth_occluded{};
    Nanoseconds measured_at_ns{};
};

// Identity is authoritative outside the landmark model. OpenSeeFace cannot
// identify a character, so a packet is usable only while the product runtime
// proves the selected actor and a temporal appearance latch agree.
struct AppearanceGateEvidenceV1 {
    std::uint32_t schema_version{1};
    std::uint64_t runtime_actor_id{};
    std::uint64_t descriptor_revision{};
    std::uint64_t expected_descriptor_digest_high{};
    std::uint64_t expected_descriptor_digest_low{};
    std::uint64_t observed_descriptor_digest_high{};
    std::uint64_t observed_descriptor_digest_low{};
    double similarity{};
    double temporal_iou{};
    double blocker_coverage{};
    bool identity_locked{};
    bool target_visible{};
    bool scene_transition{};
};

enum class VisualPressure : std::uint8_t {
    nominal,
    elevated_cpu,
    elevated_memory,
    critical,
    suspended,
};

struct VisualResourceStateV1 {
    std::uint32_t schema_version{1};
    VisualPressure pressure{VisualPressure::nominal};
    std::uint32_t admitted_signal_rate_hz{15};
    bool local_visuals_admitted{true};
};

enum class SignalDisposition : std::uint8_t {
    accepted,
    bypass_cancelled,
    bypass_wrong_actor,
    bypass_wrong_frame,
    bypass_invalid_packet,
    bypass_appearance,
    bypass_occluded,
    bypass_stale,
    bypass_rate_limited,
    bypass_pressure,
    bypass_unsafe_roi,
};

[[nodiscard]] constexpr std::string_view to_string(const SignalDisposition value) noexcept {
    switch (value) {
    case SignalDisposition::accepted: return "accepted";
    case SignalDisposition::bypass_cancelled: return "bypass_cancelled";
    case SignalDisposition::bypass_wrong_actor: return "bypass_wrong_actor";
    case SignalDisposition::bypass_wrong_frame: return "bypass_wrong_frame";
    case SignalDisposition::bypass_invalid_packet: return "bypass_invalid_packet";
    case SignalDisposition::bypass_appearance: return "bypass_appearance";
    case SignalDisposition::bypass_occluded: return "bypass_occluded";
    case SignalDisposition::bypass_stale: return "bypass_stale";
    case SignalDisposition::bypass_rate_limited: return "bypass_rate_limited";
    case SignalDisposition::bypass_pressure: return "bypass_pressure";
    case SignalDisposition::bypass_unsafe_roi: return "bypass_unsafe_roi";
    }
    return "unknown";
}

struct SignalDecision {
    SignalDisposition disposition{SignalDisposition::bypass_invalid_packet};
    std::optional<TrackingEvidence> tracking;
    std::uint32_t admitted_signal_rate_hz{};
    std::uint64_t latch_generation{};

    [[nodiscard]] bool accepted() const noexcept {
        return disposition == SignalDisposition::accepted && tracking.has_value();
    }
};

struct OpenSeeFaceAdapterPolicy {
    Nanoseconds maximum_measurement_age_ns{150'000'000};
    double minimum_detector_confidence{0.70};
    double minimum_landmark_confidence{0.82};
    double minimum_visibility_ratio{0.72};
    double minimum_appearance_similarity{0.82};
    double minimum_temporal_iou{0.42};
    double maximum_blocker_coverage{0.35};
    double maximum_center_motion_face_fraction{0.12};
    double maximum_area_delta_fraction{0.60};
    double smoothing_alpha{0.42};
    std::uint32_t recovery_matches_after_rejection{2};
};

class OpenSeeFaceSignalAdapter final {
public:
    explicit OpenSeeFaceSignalAdapter(std::uint64_t initial_generation = 1,
                                      OpenSeeFaceAdapterPolicy policy = {});

    [[nodiscard]] SignalDecision adapt(const OpenSeeFaceLandmarkPacketV1& packet,
                                       const AppearanceGateEvidenceV1& appearance,
                                       const VisualResourceStateV1& resources,
                                       const FrameIdentity& current_frame,
                                       Nanoseconds now_ns);

    [[nodiscard]] bool cancel_to(std::uint64_t generation) noexcept;
    void reset_track() noexcept;
    // Source-preserving renderers filter shape in current mouth coordinates;
    // they must not inherit a lagging screen-space pose from the legacy EMA.
    void set_current_frame_geometry(bool enabled) noexcept;

    [[nodiscard]] std::uint64_t active_generation() const noexcept;
    [[nodiscard]] bool appearance_latched() const noexcept;

private:
    struct StableState {
        TrackBinding track;
        NormalizedRect mouth_bounds;
        NormalizedRect face_bounds;
        MouthLandmarks mouth_landmarks;
        Nanoseconds last_accepted_at_ns{};
        bool rejected_since_accept{};
    };

    struct ReacquisitionCandidate {
        TrackBinding track;
        NormalizedRect mouth_bounds;
        NormalizedRect face_bounds;
        MouthLandmarks mouth_landmarks;
        Nanoseconds last_observed_at_ns{};
        std::uint32_t consecutive_matches{};
    };

    [[nodiscard]] SignalDecision bypass(SignalDisposition disposition,
                                        std::uint32_t rate) const noexcept;
    void clear_reacquisition_candidate() noexcept;

    std::uint64_t active_generation_{};
    std::uint64_t latch_generation_{1};
    OpenSeeFaceAdapterPolicy policy_;
    bool current_frame_geometry_{};
    std::optional<StableState> stable_;
    std::optional<ReacquisitionCandidate> reacquisition_candidate_;
};

} // namespace npc::mouth
