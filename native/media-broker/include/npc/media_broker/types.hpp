#pragma once

#include <chrono>
#include <array>
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace npc::media {

using MonotonicTime = std::chrono::steady_clock::time_point;
using Duration = std::chrono::steady_clock::duration;

// Event-driven WGC delivery, CPU landmark inference, and authenticated D3D
// presentation are separately bounded. The review producer gives inference a
// 220 ms budget after binding; these broker bounds include at most 120 ms of
// source-frame age plus a short result handoff window.
inline constexpr std::uint64_t visual_capture_freshness_ms = 120U;
inline constexpr std::uint64_t visual_worker_result_deadline_ms = 340U;
inline constexpr std::uint64_t visual_presentation_deadline_ms = 440U;

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

// Authenticated capture evidence names both where pixels came from and what
// they cover.  A display-wide source must never be promoted to exact-window
// evidence merely because its pixels happen to resemble the selected game.
enum class CapturePixelSource : std::uint32_t {
    unavailable = 0,
    windows_graphics_capture_texture = 1,
    desktop_duplication_texture = 2,
};

enum class CapturePixelScope : std::uint32_t {
    unavailable = 0,
    exact_selected_window = 1,
    full_display_output = 2,
};
enum class OverlayBackend { none, d3d11_direct_composition };
enum class AudioState { stopped, initializing, ready, capturing, playing, recovering, failed };
enum class TargetState {
    none,
    selected,
    unavailable,
    minimized,
    occluded,
    protected_content,
    closed,
    exclusive_fullscreen,
    unsupported,
};
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
    exclusive_fullscreen,
    unsupported_path,
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
    // Monotonically advances whenever capture-to-desktop mapping changes. It
    // invalidates frames and asynchronous residual work from the old mapping.
    std::uint64_t geometry_epoch{};
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
    std::uint64_t geometry_epoch{};
    // Acquisition-time QPC and a bounded fingerprint of captured pixels are
    // independent evidence that the capture stream is genuinely advancing.
    std::uint64_t captured_qpc{};
    std::uint64_t content_hash{};
};

struct OcclusionEvidence {
    double face_confidence{};
    double landmark_confidence{};
    double visibility_ratio{};
    bool mouth_region_occluded{};
    MonotonicTime measured_at{};
    // Bind tracking evidence to one captured frame in one graphics epoch.
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_qpc{};
    // Actor identity and temporal track identity are independent typed values.
    // Reacquisition must increment the epoch even if a tracker reuses its ID.
    std::uint64_t actor_id{};
    std::uint64_t selected_track_id{};
    std::uint64_t track_epoch{};
};

struct MouthPatch {
    std::uint64_t source_frame_sequence{};
    std::uint64_t cancellation_generation{};
    RectF normalized_bounds;
    double confidence{};
    MonotonicTime produced_at{};
    std::uintptr_t native_texture{};
    // Sequence values can restart after capture recreation. The graphics epoch
    // and original capture time prevent stale output from matching by accident.
    std::uint64_t source_device_generation{};
    MonotonicTime source_frame_captured_at{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_qpc{};
    std::uint64_t actor_id{};
    std::uint64_t selected_track_id{};
    std::uint64_t track_epoch{};
};

// Validated control-plane metadata for one externally produced residual. The
// Windows adapter is the only component permitted to turn source_handle_value
// into a local D3D resource, using DuplicateHandle from worker_process_id.
struct SharedResidualLease {
    std::uint32_t schema_version{1};
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    std::uint64_t source_handle_value{};
    std::uint64_t lease_nonce_high{};
    std::uint64_t lease_nonce_low{};
    std::uint64_t adapter_luid{};
    std::uint64_t keyed_mutex_acquire_key{};
    std::uint64_t keyed_mutex_release_key{};
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    std::uint32_t dxgi_format{};
    std::uint32_t alpha_mode{};
    std::uint64_t expires_qpc{};
    std::uint64_t cancellation_generation{};
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_frame_qpc{};
    std::uint64_t produced_qpc{};
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};
    RectF normalized_bounds;
};

// Broker-issued, single-frame GPU source lease. The broker creates a distinct
// immutable shared copy of the latest WGC texture and duplicates its NT handle
// only into the attested worker. It never exports Desktop Duplication frames.
struct VisualSourceLeaseRequest {
    std::uint64_t cancellation_generation{};
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};
    bool reserve_for_actor_picker{};
};

struct VisualSourceLease {
    std::uint32_t schema_version{1};
    std::uint32_t broker_process_id{};
    std::uint64_t broker_process_creation_time{};
    std::string broker_executable_name;
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    std::uint64_t worker_handle_value{};
    std::uint64_t lease_nonce_high{};
    std::uint64_t lease_nonce_low{};
    std::uint64_t adapter_luid{};
    std::uint64_t keyed_mutex_acquire_key{};
    std::uint64_t keyed_mutex_release_key{};
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    std::uint32_t dxgi_format{};
    std::uint32_t alpha_mode{};
    std::uint64_t expires_qpc{};
    std::uint64_t qpc_frequency{};
    std::uint64_t cancellation_generation{};
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_frame_qpc{};
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};
};

// Broker-owned, one-frame CPU crop for the local identity observation worker.
// Unlike VisualSourceLease this is a named read-only byte mapping, not a GPU
// handle. Exactly one may be live, and every invalidation destroys the mapping.
struct IdentityFrameLeaseRequest {
    std::string capture_session_id;
    std::uint64_t cancellation_generation{};
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    RectI crop_px;
};

struct IdentityFrameLease {
    std::uint32_t schema_version{1};
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    std::string lease_id;
    std::string shared_memory_name;
    std::string lease_nonce;
    std::uint64_t byte_length{};
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    std::string pixel_format;
    std::string content_sha256;
    std::uint64_t expires_qpc{};
    std::uint64_t qpc_frequency{};
    std::uint64_t cancellation_generation{};
    std::string capture_session_id;
    std::uint32_t selected_process_id{};
    std::uint64_t selected_window_handle{};
    std::string selected_executable_name;
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_frame_qpc{};
    std::uint64_t captured_at_unix_ms{};
    bool advancing_frame_verified{};
    bool overlay_capture_excluded{};
    bool protected_online_detected{};
    bool anti_cheat_detected{};
    RectI crop_px;
    SizeI source_size_px;
};

enum class IdentityReferenceSourceClass : std::uint32_t {
    user_private = 1,
    original_synthetic = 2,
};

struct IdentityReferenceImportRequest {
    std::string capture_session_id;
    std::uint64_t cancellation_generation{};
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    std::string picker_consent_token;
    std::string game_profile_id;
    std::string character_id;
    std::string subject_id;
    std::string reference_id;
    std::string subject_display_name;
    IdentityReferenceSourceClass source_class{IdentityReferenceSourceClass::user_private};
    std::string owner_user_id;
    std::string original_work_license;
    bool explicit_user_consent{};
    bool local_only{};
    std::uint64_t imported_at_unix_ms{};
};

struct IdentityReferenceImportLease {
    std::uint32_t schema_version{1};
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    std::string lease_id;
    std::string shared_memory_name;
    std::string lease_nonce;
    std::uint64_t byte_length{};
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    std::string pixel_format;
    std::string content_sha256;
    std::string source_asset_sha256;
    std::string source_media_type;
    std::uint64_t expires_qpc{};
    std::uint64_t qpc_frequency{};
    std::uint64_t cancellation_generation{};
    std::string capture_session_id;
    std::uint32_t selected_process_id{};
    std::uint64_t selected_window_handle{};
    std::string selected_executable_name;
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::string picker_consent_token;
    std::string game_profile_id;
    std::string character_id;
    std::string subject_id;
    std::string reference_id;
    std::string subject_display_name;
    IdentityReferenceSourceClass source_class{IdentityReferenceSourceClass::user_private};
    std::string owner_user_id;
    std::string original_work_license;
    bool explicit_user_consent{};
    bool local_only{};
    std::uint64_t imported_at_unix_ms{};
};

// Manual actor selection is deliberately a native transaction. The WebView is
// never given captured pixels, candidate rectangles, or the pointer location.
// A qualified native visual authority supplies frame-bound detected ROIs and
// consumes the typed terminal receipt.
enum class ManualActorPickerStatus : std::uint32_t {
    pending = 1,
    selected = 2,
    cancelled = 3,
    timed_out = 4,
    target_lost = 5,
    target_resized = 6,
    dpi_changed = 7,
    device_changed = 8,
    capture_changed = 9,
    click_outside_detected_roi = 10,
    ambiguous_detected_roi = 11,
    untrusted_pointer_input = 12,
    overlay_unavailable = 13,
    internal_error = 14,
};

enum class ManualActorPointerKind : std::uint32_t {
    none = 0,
    mouse = 1,
    touch = 2,
    pen = 3,
};

struct ManualActorCandidate {
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};
    RectF normalized_bounds;
};

struct ManualActorPickerRequest {
    std::string request_id;
    std::string capture_session_id;
    std::uint64_t cancellation_generation{};
    std::uint32_t selected_process_id{};
    std::uint64_t selected_window_handle{};
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_frame_qpc{};
    std::uint32_t timeout_ms{};
    std::vector<ManualActorCandidate> candidates;
};

struct ManualActorPickerReceipt {
    std::uint32_t schema_version{1};
    std::string request_id;
    ManualActorPickerStatus status{ManualActorPickerStatus::pending};
    std::uint64_t receipt_nonce_high{};
    std::uint64_t receipt_nonce_low{};
    std::string capture_session_id;
    std::uint64_t cancellation_generation{};
    std::uint32_t selected_process_id{};
    std::uint64_t selected_window_handle{};
    std::string selected_executable_name;
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_frame_qpc{};
    std::uint64_t selected_actor_id{};
    std::uint64_t selected_track_id{};
    std::uint64_t selected_track_epoch{};
    std::uint32_t candidate_count{};
    std::string candidate_set_sha256;
    std::uint64_t began_qpc{};
    std::uint64_t clicked_qpc{};
    std::uint64_t attested_at_qpc{};
    std::uint64_t qpc_frequency{};
    ManualActorPointerKind pointer_kind{ManualActorPointerKind::none};
    bool frozen_wgc_frame_verified{};
    bool overlay_capture_excluded{};
    bool overlay_nonactivating{};
    bool single_hardware_pointer_click{};
    bool pixels_withheld_from_webview{true};
    bool coordinates_withheld_from_webview{true};
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
    std::chrono::milliseconds maximum_source_to_patch_latency{80};
    std::chrono::milliseconds source_timestamp_tolerance{2};
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

struct ResidualBoundsPolicy {
    // A residual is a local mouth edit, never a replacement face or frame.
    double maximum_width{0.45};
    double maximum_height{0.35};
    double maximum_area{0.12};
};

struct BrokerPolicy {
    TimingPolicy timing;
    OcclusionPolicy occlusion;
    ResidualBoundsPolicy residual_bounds;
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
    std::uint64_t patches_received{};
    std::uint64_t patches_presented{};
    std::uint64_t patches_rejected{};
    std::uint32_t consecutive_failures{};
    std::optional<Failure> last_failure;
    std::uint64_t geometry_epoch{};
    std::uint64_t geometry_changes{};
    std::uint64_t latest_frame_sequence{};
    std::uint64_t latest_frame_qpc{};
    std::uint64_t initial_content_hash{};
    std::uint64_t latest_content_hash{};
    std::uint64_t content_hash_changes{};
    std::uint64_t nonadvancing_frames{};
    std::uint32_t selected_process_id{};
    std::uintptr_t selected_window{};
    std::string selected_executable_name;
    SizeI latest_content_size_px;
    bool overlay_capture_excluded{};
    bool overlay_visuals_allowed{};
};

[[nodiscard]] std::string_view to_string(BrokerState state) noexcept;
[[nodiscard]] std::string_view to_string(CaptureBackend backend) noexcept;
[[nodiscard]] std::string_view to_string(RecoveryAction action) noexcept;

} // namespace npc::media
