#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <string_view>
#include <vector>

namespace npc::mouth {

using Nanoseconds = std::int64_t;

struct TrackBinding {
    std::uint64_t cancellation_generation{};
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};

    [[nodiscard]] constexpr bool operator==(const TrackBinding&) const noexcept = default;
};

struct FrameIdentity {
    std::uint64_t sequence{};
    std::uint64_t device_generation{};
    std::uint64_t geometry_epoch{};
    Nanoseconds captured_at_ns{};

    [[nodiscard]] constexpr bool operator==(const FrameIdentity&) const noexcept = default;
};

struct NormalizedRect {
    double x{};
    double y{};
    double width{};
    double height{};

    [[nodiscard]] constexpr double right() const noexcept { return x + width; }
    [[nodiscard]] constexpr double bottom() const noexcept { return y + height; }
};

struct HeadPoseDegrees {
    double yaw{};
    double pitch{};
    double roll{};
};

struct NormalizedLandmark {
    double x{};
    double y{};
    double confidence{};
};

inline constexpr std::size_t mouth_contour_point_count = 18U;

// Model-independent semantic adapter. Schema 2 retains the tracker-neutral
// anchors and the complete ordered mouth contour. The contour is deliberately
// local to the worker contract: raw provider packets remain versioned at their
// ingress boundary, while the compositor gets enough geometry to follow the
// actual lip curves instead of drawing a four-anchor slit.
struct MouthLandmarks {
    std::uint32_t schema_version{1};
    std::uint64_t provider_instance_id{};
    NormalizedLandmark left_corner;
    NormalizedLandmark right_corner;
    NormalizedLandmark upper_lip_center;
    NormalizedLandmark lower_lip_center;
    std::uint32_t contour_points{};
    std::array<NormalizedLandmark, mouth_contour_point_count> contour{};
};

struct TrackingEvidence {
    TrackBinding track;
    FrameIdentity frame;
    NormalizedRect face_bounds;
    NormalizedRect mouth_bounds;
    MouthLandmarks mouth_landmarks;
    HeadPoseDegrees pose;
    double face_confidence{};
    double landmark_confidence{};
    double visibility_ratio{};
    bool mouth_occluded{};
    Nanoseconds measured_at_ns{};
};

enum class PixelFormat : std::uint32_t {
    bgra8_unorm_premultiplied = 87,
};

enum class LeaseTransport : std::uint32_t {
    cpu_reference = 0,
    d3d11_shared_nt_handle = 1,
};

// Protocol-ready metadata for either the in-process CPU reference path or a
// future authenticated D3D11 shared-handle transport. Numeric handles are not
// opened by this library and are never treated as pointers.
struct TextureLeaseDescriptor {
    std::uint32_t schema_version{1};
    LeaseTransport transport{LeaseTransport::cpu_reference};
    std::uint64_t lease_nonce_high{};
    std::uint64_t lease_nonce_low{};
    std::uint32_t owner_process_id{};
    std::uint32_t intended_consumer_process_id{};
    std::uint64_t native_handle_value{};
    std::uint32_t adapter_luid_low{};
    std::int32_t adapter_luid_high{};
    std::uint64_t keyed_mutex_acquire_key{};
    std::uint64_t keyed_mutex_release_key{};
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    PixelFormat format{PixelFormat::bgra8_unorm_premultiplied};
    Nanoseconds expires_at_ns{};
};

struct CpuFrame {
    TextureLeaseDescriptor lease;
    FrameIdentity identity;
    std::vector<std::uint8_t> bgra;
};

enum class Viseme : std::uint8_t {
    silence,
    bilabial,
    labiodental,
    dental,
    alveolar,
    postalveolar,
    palatal,
    velar,
    rounded,
    open_vowel,
    spread_vowel,
};

struct MouthCoefficients {
    double jaw_open{};
    double lip_close{};
    double funnel{};
    double pucker{};
    double smile_left{};
    double smile_right{};
    double upper_lip_raise{};
    double lower_lip_depress{};
};

struct AudioClockBinding {
    std::uint64_t stream_generation{};
    std::uint64_t segment_id{};
    std::uint64_t first_sample_index{};
    std::uint64_t sample_count{};
    std::uint64_t playback_sample_index{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    Nanoseconds playback_at_ns{};
};

enum class DriveKind : std::uint8_t {
    explicit_coefficients,
    timed_viseme,
    pcm_window,
};

struct MouthDrive {
    DriveKind kind{DriveKind::explicit_coefficients};
    AudioClockBinding clock;
    MouthCoefficients coefficients;
    Viseme viseme{Viseme::silence};
    double viseme_strength{1.0};
    std::vector<float> interleaved_pcm;
};

struct WorkItem {
    TrackBinding track;
    CpuFrame source;
    TrackingEvidence tracking;
    MouthDrive drive;
    Nanoseconds deadline_ns{};
};

struct ResidualPatch {
    TrackBinding track;
    FrameIdentity source_frame;
    AudioClockBinding audio_clock;
    NormalizedRect normalized_bounds;
    TextureLeaseDescriptor source_lease;
    TextureLeaseDescriptor residual_lease;
    MouthCoefficients coefficients;
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    Nanoseconds produced_at_ns{};
    std::vector<std::uint8_t> premultiplied_bgra;
};

enum class Disposition : std::uint8_t {
    residual_ready,
    bypass_no_work,
    bypass_cancelled,
    bypass_wrong_frame,
    bypass_stale_frame,
    bypass_expired_lease,
    bypass_deadline,
    bypass_audio_clock,
    bypass_invalid_frame,
    bypass_invalid_tracking,
    bypass_occluded,
    bypass_pose,
    bypass_low_confidence,
    bypass_unsafe_bounds,
};

[[nodiscard]] constexpr std::string_view to_string(const Disposition value) noexcept {
    switch (value) {
    case Disposition::residual_ready: return "residual_ready";
    case Disposition::bypass_no_work: return "bypass_no_work";
    case Disposition::bypass_cancelled: return "bypass_cancelled";
    case Disposition::bypass_wrong_frame: return "bypass_wrong_frame";
    case Disposition::bypass_stale_frame: return "bypass_stale_frame";
    case Disposition::bypass_expired_lease: return "bypass_expired_lease";
    case Disposition::bypass_deadline: return "bypass_deadline";
    case Disposition::bypass_audio_clock: return "bypass_audio_clock";
    case Disposition::bypass_invalid_frame: return "bypass_invalid_frame";
    case Disposition::bypass_invalid_tracking: return "bypass_invalid_tracking";
    case Disposition::bypass_occluded: return "bypass_occluded";
    case Disposition::bypass_pose: return "bypass_pose";
    case Disposition::bypass_low_confidence: return "bypass_low_confidence";
    case Disposition::bypass_unsafe_bounds: return "bypass_unsafe_bounds";
    }
    return "unknown";
}

struct ProcessResult {
    Disposition disposition{Disposition::bypass_no_work};
    ResidualPatch residual;

    [[nodiscard]] bool has_residual() const noexcept {
        return disposition == Disposition::residual_ready;
    }
};

struct WorkerStats {
    std::uint64_t submitted{};
    std::uint64_t replaced_before_processing{};
    std::uint64_t cancelled{};
    std::uint64_t processed{};
    std::uint64_t residuals{};
    std::uint64_t bypasses{};
};

struct WorkerPolicy {
    Nanoseconds maximum_source_age_ns{150'000'000};
    Nanoseconds maximum_tracking_age_ns{150'000'000};
    Nanoseconds maximum_audio_skew_ns{80'000'000};
    double minimum_face_confidence{0.70};
    double minimum_landmark_confidence{0.82};
    double minimum_semantic_landmark_confidence{0.55};
    double minimum_visibility_ratio{0.72};
    double maximum_absolute_yaw_degrees{35.0};
    double maximum_absolute_pitch_degrees{25.0};
    double maximum_absolute_roll_degrees{45.0};
    double maximum_mouth_width_fraction{0.45};
    double maximum_mouth_height_fraction{0.35};
    double maximum_mouth_area_fraction{0.12};
};

} // namespace npc::mouth
