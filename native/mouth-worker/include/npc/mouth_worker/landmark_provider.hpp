#pragma once

#include "npc/mouth_worker/signal_adapter.hpp"
#include "npc/mouth_worker/worker.hpp"

#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace npc::mouth {

inline constexpr std::string_view admitted_openseeface_pack_id_v1 =
    "openseeface-mnv3-lm1-mouth-signal";
inline constexpr std::string_view admitted_yunet_openseeface_pack_id_v1 =
    "openseeface-yunet640-lm1-mouth-signal";
inline constexpr std::string_view admitted_openseeface_revision_v1 =
    "85aa70fc67582d046e771ea73625182a0d8f7475";
inline constexpr std::string_view admitted_yunet_detector_sha256_v1 =
    "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";
inline constexpr std::string_view admitted_openseeface_lm1_sha256_v1 =
    "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f";
inline constexpr std::string_view admitted_openseeface_runtime_revision_v1 = "1.22.1";
inline constexpr std::string_view admitted_openseeface_backend_v1 =
    "cpu-execution-provider-one-thread";

struct AdmittedLandmarkProviderLaunchV1 {
    std::uint32_t schema_version{1};
    std::string pack_id;
    std::string pack_revision;
    std::filesystem::path artifact_root;
    std::filesystem::path detector_model;
    std::filesystem::path landmark_model;
    std::filesystem::path runtime_library;
    std::filesystem::path runtime_shared_library;
    std::uint64_t detector_size_bytes{};
    std::uint64_t landmark_size_bytes{};
    std::uint64_t runtime_size_bytes{};
    std::uint64_t runtime_shared_size_bytes{};
    std::string detector_sha256;
    std::string landmark_sha256;
    std::string runtime_sha256;
    std::string runtime_shared_sha256;
    std::string measured_envelope_sha256;
    std::string runtime_revision;
    std::string backend;
    std::uint32_t maximum_signal_rate_hz{};
    std::uint32_t inference_threads{};
    std::uint32_t exact_target_process_id{};
};

struct LandmarkInferenceWorkV1 {
    std::uint32_t schema_version{1};
    TrackBinding track;
    FrameIdentity frame;
    std::uint64_t source_frame_qpc{};
    std::uint64_t qpc_frequency{};
    NormalizedRect seed_face_bounds;
    CpuFrame source;
    Nanoseconds deadline_ns{};
};

// Cross-platform, allocation-bounded YuNet decoding contract shared by the
// Windows ORT provider and deterministic tests. Each level contains one
// class/object score and four box values per grid cell. Coordinates returned
// by the decoder are normalized to the source frame, after excluding the
// square model tensor's right/bottom padding.
struct YuNetDetectorLevelV1 {
    std::uint32_t stride{};
    std::uint32_t grid_width{};
    std::uint32_t grid_height{};
    std::span<const float> class_scores;
    std::span<const float> object_scores;
    std::span<const float> boxes;
};

struct YuNetFaceCandidateV1 {
    NormalizedRect bounds;
    double confidence{};
};

struct YuNetTrackingPolicyV1 {
    // A full-frame detector refresh is intentionally bounded: shorter periods
    // cannot sustain the capture freshness contract on the admitted CPU pack,
    // while longer periods leave too much time for an ROI to drift.
    std::uint32_t detector_refresh_interval_frames{12U};
};

inline constexpr std::uint32_t yunet_min_detector_refresh_interval_v1 = 10U;
inline constexpr std::uint32_t yunet_max_detector_refresh_interval_v1 = 15U;

enum class YuNetTrackingActionV1 : std::uint8_t {
    track_landmarks,
    reacquire_face,
};

struct YuNetTrackedFaceUpdateV1 {
    NormalizedRect face_bounds;
    double center_motion_face_fraction{};
    double scale_ratio{};
};

struct LandmarkProviderInferenceDiagnosticsV1 {
    bool detector_ran{};
    bool detector_refresh_due{};
    bool quality_reacquisition{};
    bool used_tracked_roi{};
    std::uint32_t landmark_runs{};
    double detector_ms{};
    double landmark_ms{};
};

[[nodiscard]] std::optional<YuNetFaceCandidateV1>
decode_and_select_yunet_face_v1(
    std::span<const YuNetDetectorLevelV1> levels,
    std::uint32_t model_width,
    std::uint32_t model_height,
    std::uint32_t content_width,
    std::uint32_t content_height,
    std::uint32_t source_width,
    std::uint32_t source_height,
    const NormalizedRect& seed_face_bounds,
    double score_threshold = 0.50,
    double nms_threshold = 0.30) noexcept;

[[nodiscard]] bool validate_yunet_tracking_policy_v1(
    const YuNetTrackingPolicyV1& policy) noexcept;
[[nodiscard]] YuNetTrackingActionV1 choose_yunet_tracking_action_v1(
    bool has_tracked_face,
    std::uint32_t frames_since_detector,
    const YuNetTrackingPolicyV1& policy) noexcept;
[[nodiscard]] bool yunet_face_matches_locked_actor_v1(
    const NormalizedRect& candidate,
    const NormalizedRect& locked_face) noexcept;
[[nodiscard]] std::optional<YuNetTrackedFaceUpdateV1>
update_yunet_tracked_face_v1(
    const NormalizedRect& previous_face,
    std::span<const NormalizedLandmark> previous_landmarks,
    std::span<const NormalizedLandmark> current_landmarks) noexcept;

enum class LandmarkProviderDispositionV1 : std::uint8_t {
    ready,
    packet_produced,
    replaced_before_processing,
    bypass_not_loaded,
    bypass_invalid,
    bypass_stale,
    bypass_cancelled,
    bypass_provider_failure,
    unloaded,
};

struct LandmarkProviderReceiptV1 {
    std::uint32_t schema_version{1};
    LandmarkProviderDispositionV1 disposition{LandmarkProviderDispositionV1::bypass_not_loaded};
    TrackBinding track;
    FrameIdentity frame;
    std::uint64_t source_frame_qpc{};
    std::uint64_t provider_generation{};
    std::uint64_t queue_replacements{};
    Nanoseconds submitted_at_ns{};
    Nanoseconds completed_at_ns{};
    std::optional<OpenSeeFaceLandmarkPacketV1> packet;
    // Returned only with a valid packet so the service can composite from the
    // exact same CPU frame without a second frame-sized copy or another lease.
    std::optional<LandmarkInferenceWorkV1> completed_work;
    std::string detail;

    [[nodiscard]] bool produced_packet() const noexcept {
        return disposition == LandmarkProviderDispositionV1::packet_produced && packet.has_value();
    }
};

// The real implementation dynamically loads the separately installed ORT
// runtime. Tests inject a fake through this exact boundary; the product never
// substitutes synthetic landmarks when a real provider is unavailable.
class NativeLandmarkProviderV1 {
public:
    virtual ~NativeLandmarkProviderV1() = default;
    [[nodiscard]] virtual bool load(const AdmittedLandmarkProviderLaunchV1& launch,
                                    std::uint64_t generation,
                                    std::string& failure) = 0;
    [[nodiscard]] virtual std::optional<OpenSeeFaceLandmarkPacketV1> infer(
        const LandmarkInferenceWorkV1& work,
        Nanoseconds now_ns,
        std::string& failure) = 0;
    [[nodiscard]] virtual bool cancel_to(std::uint64_t generation) noexcept = 0;
    virtual void unload() noexcept = 0;
    [[nodiscard]] virtual bool loaded() const noexcept = 0;
    [[nodiscard]] virtual LandmarkProviderInferenceDiagnosticsV1
    last_inference_diagnostics() const noexcept { return {}; }
};

// Queue-depth-one native owner. It revalidates provider output against the
// exact leased actor/track/frame after inference, so even an authenticated
// provider cannot return a packet for another frame or generation.
class NativeLandmarkCoordinatorV1 final {
public:
    explicit NativeLandmarkCoordinatorV1(std::unique_ptr<NativeLandmarkProviderV1> provider);

    [[nodiscard]] LandmarkProviderReceiptV1 load(
        const AdmittedLandmarkProviderLaunchV1& launch,
        std::uint64_t generation,
        Nanoseconds now_ns);
    [[nodiscard]] LandmarkProviderReceiptV1 submit(LandmarkInferenceWorkV1 work,
                                                   Nanoseconds now_ns);
    [[nodiscard]] LandmarkProviderReceiptV1 process_latest(Nanoseconds now_ns);
    [[nodiscard]] LandmarkProviderReceiptV1 cancel_to(std::uint64_t generation,
                                                      Nanoseconds now_ns) noexcept;
    [[nodiscard]] LandmarkProviderReceiptV1 unload(Nanoseconds now_ns) noexcept;
    [[nodiscard]] std::uint64_t active_generation() const noexcept;
    [[nodiscard]] std::uint64_t queue_replacements() const noexcept;

private:
    struct PendingWork {
        LandmarkInferenceWorkV1 work;
        Nanoseconds submitted_at_ns{};
    };

    [[nodiscard]] LandmarkProviderReceiptV1 bypass(
        LandmarkProviderDispositionV1 disposition,
        std::string detail,
        Nanoseconds now_ns,
        const LandmarkInferenceWorkV1* work = nullptr) const;

    std::unique_ptr<NativeLandmarkProviderV1> provider_;
    std::optional<PendingWork> pending_;
    std::uint64_t generation_{};
    std::uint64_t queue_replacements_{};
};

[[nodiscard]] bool validate_landmark_provider_launch_v1(
    const AdmittedLandmarkProviderLaunchV1& launch) noexcept;
[[nodiscard]] bool validate_landmark_inference_work_v1(
    const LandmarkInferenceWorkV1& work,
    std::uint64_t generation,
    Nanoseconds now_ns) noexcept;

// Project-owned native provider. It has no linked ORT import: load() first
// re-hashes and locks the governor-authorized optional files, then resolves
// the exact pinned ORT v22 ABI from that DLL. A missing/changed runtime returns
// a visual-only provider failure.
[[nodiscard]] std::unique_ptr<NativeLandmarkProviderV1>
make_windows_ort_landmark_provider_v1(
    YuNetTrackingPolicyV1 yunet_tracking = {});

} // namespace npc::mouth
