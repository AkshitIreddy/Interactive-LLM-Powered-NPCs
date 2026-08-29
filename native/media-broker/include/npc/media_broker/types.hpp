#pragma once

#include <chrono>
#include <array>
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>

namespace npc::media {

using MonotonicTime = std::chrono::steady_clock::time_point;
using Duration = std::chrono::steady_clock::duration;

enum class BrokerState {
    stopped,
    starting,
    awaiting_target,
    capturing_primary,
    capturing_fallback,
    recovering_device,
    blocked_by_policy,
    degraded_audio_only,
    stopping,
    failed,
};

enum class CaptureBackend { none, windows_graphics_capture, desktop_duplication };
enum class OverlayBackend { none, d3d11_direct_composition };
enum class AudioState { stopped, initializing, ready, capturing, playing, recovering, failed };
enum class TargetState { none, selected, unavailable, minimized, occluded, protected_content, closed };
enum class InputMode { push_to_talk, voice_activity, typed_only };
enum class PttState { released, pressed };
enum class ColorSpace { sdr_srgb, sdr_sc_rgb, hdr10_pq, hdr_sc_rgb, unknown };
enum class DisplayRotation { identity, rotate_90, rotate_180, rotate_270 };

/// Immutable process boundary for every supported game. The broker can inspect
/// a selected HWND/process for policy evidence and capture it through public
/// Windows APIs; it has no in-process integration mode.
struct ExternalGameBoundary final {
    static constexpr std::array<CaptureBackend, 2> capture_backends{
        CaptureBackend::windows_graphics_capture,
        CaptureBackend::desktop_duplication,
    };
    static constexpr bool permits_native_rig_animation = false;
    static constexpr bool permits_executable_adapters = false;
    static constexpr bool permits_process_injection = false;
    static constexpr bool permits_game_hooks = false;
    static constexpr bool permits_game_module_loading = false;
    static constexpr bool permits_process_memory_writes = false;
};

static_assert(!ExternalGameBoundary::permits_native_rig_animation);
static_assert(!ExternalGameBoundary::permits_executable_adapters);
static_assert(!ExternalGameBoundary::permits_process_injection);
static_assert(!ExternalGameBoundary::permits_game_hooks);
static_assert(!ExternalGameBoundary::permits_game_module_loading);
static_assert(!ExternalGameBoundary::permits_process_memory_writes);
enum class FailureDomain { target, capture, overlay, audio_capture, audio_render, hotkey, device, policy };
enum class FailureCode {
    none,
    target_lost,
    target_minimized,
    access_denied,
    protected_content,
    backend_unavailable,
    device_removed,
    device_reset,
    timeout,
    invalid_geometry,
    hotkey_conflict,
    anti_cheat_detected,
    online_mode_detected,
    internal_error,
};

enum class RecoveryAction {
    none,
    retry_same_backend,
    switch_to_desktop_duplication,
    recreate_graphics_device,
    recreate_audio_client,
    await_target,
    degrade_to_audio_only,
    block_session,
};

struct SizeI {
    std::int32_t width{};
    std::int32_t height{};
    [[nodiscard]] constexpr bool valid() const noexcept { return width > 0 && height > 0; }
    friend constexpr bool operator==(const SizeI&, const SizeI&) = default;
};

struct PointI {
    std::int32_t x{};
    std::int32_t y{};
    friend constexpr bool operator==(const PointI&, const PointI&) = default;
};

struct RectI {
    std::int32_t left{};
    std::int32_t top{};
    std::int32_t right{};
    std::int32_t bottom{};

    [[nodiscard]] constexpr std::int32_t width() const noexcept { return right - left; }
    [[nodiscard]] constexpr std::int32_t height() const noexcept { return bottom - top; }
    [[nodiscard]] constexpr bool valid() const noexcept { return width() > 0 && height() > 0; }
    friend constexpr bool operator==(const RectI&, const RectI&) = default;
};

struct RectF {
    double left{};
    double top{};
    double right{};
    double bottom{};
    [[nodiscard]] constexpr double width() const noexcept { return right - left; }
    [[nodiscard]] constexpr double height() const noexcept { return bottom - top; }
};

// HWND is deliberately represented as an integer in the portable contract. The
// Windows adapter is solely responsible for validating and converting it.
struct GameTarget {
    std::uintptr_t native_window{};
    std::uint32_t process_id{};
    std::string executable_name;
    std::string display_name;

    [[nodiscard]] bool valid() const noexcept {
        return native_window != 0 && process_id != 0 && !executable_name.empty();
    }
};

struct MonitorInfo {
    std::string stable_id;
    RectI desktop_bounds_px;
    RectI work_area_px;
    std::uint32_t dpi_x{96};
    std::uint32_t dpi_y{96};
    ColorSpace color_space{ColorSpace::unknown};
    DisplayRotation rotation{DisplayRotation::identity};
    double sdr_white_level_nits{80.0};
};

struct TargetGeometry {
    RectI window_bounds_px;
    RectI client_bounds_px;
    // Desktop-space extent represented by captured_content_px. For WGC this is
    // normally the client bounds; for Desktop Duplication it is the output.
    RectI captured_desktop_bounds_px;
    SizeI captured_content_px;
    MonitorInfo monitor;
    bool minimized{};
};

struct FrameDescriptor {
    std::uint64_t sequence{};
    std::uint64_t device_generation{};
    MonotonicTime captured_at{};
    SizeI content_size_px;
    ColorSpace color_space{ColorSpace::unknown};
    // Opaque platform resource. On Windows this identifies a same-process D3D11
    // texture or a broker-created shared handle; it is never an injected pointer.
    std::uintptr_t native_texture{};
    bool content_occluded{};
    bool protected_content{};
};

struct OcclusionEvidence {
    double face_confidence{};
    double landmark_confidence{};
    double visibility_ratio{};
    bool mouth_region_occluded{};
    MonotonicTime measured_at{};
};

struct MouthPatch {
    std::uint64_t source_frame_sequence{};
    std::uint64_t cancellation_generation{};
    RectF normalized_bounds;
    double confidence{};
    MonotonicTime produced_at{};
    std::uintptr_t native_texture{};
};

struct Failure {
    FailureDomain domain{FailureDomain::device};
    FailureCode code{FailureCode::internal_error};
    bool retryable{};
    std::string message;
};

struct TimingPolicy {
    std::chrono::milliseconds frame_stale_after{100};
    std::chrono::milliseconds patch_stale_after{80};
    std::chrono::milliseconds evidence_stale_after{120};
    std::chrono::milliseconds recovery_initial_backoff{50};
    std::chrono::milliseconds recovery_max_backoff{2000};
    std::uint32_t max_same_backend_retries{2};
};

struct OcclusionPolicy {
    double minimum_face_confidence{0.78};
    double minimum_landmark_confidence{0.82};
    double minimum_visibility_ratio{0.72};
    double minimum_patch_confidence{0.82};
};

struct BrokerPolicy {
    TimingPolicy timing;
    OcclusionPolicy occlusion;
    bool permit_desktop_duplication_fallback{true};
    bool permit_audio_only_fallback{true};
};

struct Diagnostics {
    BrokerState state{BrokerState::stopped};
    CaptureBackend capture_backend{CaptureBackend::none};
    OverlayBackend overlay_backend{OverlayBackend::none};
    AudioState capture_audio{AudioState::stopped};
    AudioState render_audio{AudioState::stopped};
    TargetState target_state{TargetState::none};
    RecoveryAction pending_recovery{RecoveryAction::none};
    std::uint64_t device_generation{};
    std::uint64_t audio_device_generation{};
    std::uint64_t cancellation_generation{};
    std::uint64_t frames_received{};
    std::uint64_t frames_presented{};
    std::uint64_t frames_dropped{};
    std::uint64_t overlays_suppressed{};
    std::uint32_t consecutive_failures{};
    std::optional<Failure> last_failure;
};

[[nodiscard]] std::string_view to_string(BrokerState state) noexcept;
[[nodiscard]] std::string_view to_string(CaptureBackend backend) noexcept;
[[nodiscard]] std::string_view to_string(RecoveryAction action) noexcept;

} // namespace npc::media
