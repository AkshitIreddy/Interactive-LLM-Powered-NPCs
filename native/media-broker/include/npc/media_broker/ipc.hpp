#pragma once

#include "npc/media_broker/types.hpp"
#include "npc/media_broker/playback_transport.hpp"
#include "npc/media_broker/input_transport.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <variant>
#include <vector>

namespace npc::media::ipc {

inline constexpr std::uint32_t protocol_version = 1;
inline constexpr std::uint32_t maximum_frame_bytes = 64U * 1024U;
inline constexpr std::uint32_t maximum_pending_requests = 16;
inline constexpr std::size_t launch_nonce_bytes = 32;

enum class CommandKind : std::uint32_t {
    health = 1,
    select_target = 2,
    clear_target = 3,
    configure_ptt = 4,
    audio_status = 5,
    submit_occlusion = 6,
    submit_patch = 7,
    cancel = 8,
    diagnostics = 9,
    shutdown = 10,
    capture_evidence = 11,
    allocate_playback_stream = 12,
    cancel_playback = 13,
    allocate_visual_source = 14,
    release_visual_source = 15,
    allocate_identity_frame = 16,
    release_identity_frame = 17,
    enumerate_audio_outputs = 18,
    select_audio_output = 19,
    selected_audio_output = 20,
    // Append-only central registry. Independent broker capabilities must not
    // silently reuse a wire command ID.
    allocate_identity_reference_import = 21,
    release_identity_reference_import = 22,
    trusted_subtitle_presentation_context = 23,
    enumerate_audio_inputs = 24,
    select_audio_input = 25,
    selected_audio_input = 26,
    allocate_audio_input_rehearsal = 27,
    cancel_audio_input_rehearsal = 28,
    query_visual_audio_envelope = 29,
    query_ptt_activation_state = 30,
    manual_actor_picker = 31,
};

inline constexpr std::array command_id_registry{
    CommandKind::health,
    CommandKind::select_target,
    CommandKind::clear_target,
    CommandKind::configure_ptt,
    CommandKind::audio_status,
    CommandKind::submit_occlusion,
    CommandKind::submit_patch,
    CommandKind::cancel,
    CommandKind::diagnostics,
    CommandKind::shutdown,
    CommandKind::capture_evidence,
    CommandKind::allocate_playback_stream,
    CommandKind::cancel_playback,
    CommandKind::allocate_visual_source,
    CommandKind::release_visual_source,
    CommandKind::allocate_identity_frame,
    CommandKind::release_identity_frame,
    CommandKind::enumerate_audio_outputs,
    CommandKind::select_audio_output,
    CommandKind::selected_audio_output,
    CommandKind::allocate_identity_reference_import,
    CommandKind::release_identity_reference_import,
    CommandKind::trusted_subtitle_presentation_context,
    CommandKind::enumerate_audio_inputs,
    CommandKind::select_audio_input,
    CommandKind::selected_audio_input,
    CommandKind::allocate_audio_input_rehearsal,
    CommandKind::cancel_audio_input_rehearsal,
    CommandKind::query_visual_audio_envelope,
    CommandKind::query_ptt_activation_state,
    CommandKind::manual_actor_picker,
};

[[nodiscard]] consteval bool command_ids_are_unique() noexcept {
    for (std::size_t left = 0; left < command_id_registry.size(); ++left) {
        for (std::size_t right = left + 1; right < command_id_registry.size(); ++right) {
            if (command_id_registry[left] == command_id_registry[right]) return false;
        }
    }
    return true;
}

static_assert(command_ids_are_unique(), "Media-broker command IDs must remain globally unique");

enum class TargetProcessEffect {
    none,
    read_only_inspection_and_external_capture,
};

/// Classifies the complete V1 command surface. There is intentionally no
/// process-modifying effect: no injection, hook installation, module loading,
/// memory writing, or executable game adapter can be represented by IPC.
[[nodiscard]] constexpr TargetProcessEffect target_process_effect(const CommandKind kind) noexcept {
    return kind == CommandKind::select_target
               ? TargetProcessEffect::read_only_inspection_and_external_capture
               : TargetProcessEffect::none;
}

enum class StatusCode : std::uint32_t {
    ok = 0,
    invalid_frame = 1,
    unsupported_version = 2,
    authentication_failed = 3,
    session_mismatch = 4,
    sequence_replayed = 5,
    deadline_expired = 6,
    deadline_too_far = 7,
    cancellation_mismatch = 8,
    payload_invalid = 9,
    target_blocked = 10,
    capability_unavailable = 11,
    internal_error = 12,
};

struct SharedTextureDescriptor {
    std::uint32_t schema_version{1};
    std::array<std::byte, launch_nonce_bytes> session_nonce{};
    std::string session_id;
    std::uint64_t lease_nonce_high{};
    std::uint64_t lease_nonce_low{};
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    std::uint64_t source_process_handle_value{};
    std::uint64_t adapter_luid{};
    std::uint64_t keyed_mutex_acquire_key{};
    std::uint64_t keyed_mutex_release_key{};
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    std::uint32_t dxgi_format{};
    std::uint32_t alpha_mode{};
    std::uint64_t expires_qpc{};
};

struct HealthCommand {};
struct ClearTargetCommand {};
struct AudioStatusCommand {};
struct DiagnosticsCommand {};
struct CaptureEvidenceCommand {};
struct ShutdownCommand {};
struct CancelPlaybackCommand {};
struct EnumerateAudioOutputsCommand {};
struct SelectedAudioOutputCommand {};
struct TrustedSubtitlePresentationContextCommand {};
struct EnumerateAudioInputsCommand {};
struct SelectedAudioInputCommand {};

struct SelectAudioOutputCommand {
    playback::AudioOutputSelectionMode mode{playback::AudioOutputSelectionMode::system_default};
    std::string endpoint_id;
};

struct SelectAudioInputCommand {
    playback::AudioOutputSelectionMode mode{playback::AudioOutputSelectionMode::system_default};
    std::string endpoint_id;
};

struct AllocateAudioInputRehearsalCommand {
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::uint32_t duration_ms{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t max_frames{};
    std::uint32_t expected_producer_process_id{};
    input::ActivationSource activation_source{input::ActivationSource::explicit_rehearsal};
};

struct CancelAudioInputRehearsalCommand {
    std::string stream_id;
    std::uint64_t generation{};
};

struct QueryVisualAudioEnvelopeCommand {
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::string stream_id;
    std::string segment_id;
};

struct QueryPttActivationStateCommand {};

enum class ManualActorPickerAction : std::uint32_t {
    begin = 1,
    poll = 2,
    cancel = 3,
};

struct ManualActorPickerCommand {
    ManualActorPickerAction action{ManualActorPickerAction::poll};
    std::string request_id;
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_frame_qpc{};
    std::uint32_t timeout_ms{};
    std::vector<ManualActorCandidate> candidates;
};

struct PttActivationState {
    std::uint32_t schema_version{1};
    std::uint32_t virtual_key{};
    PttState state{PttState::released};
    std::uint64_t transition_sequence{};
    std::uint64_t transition_qpc{};
    std::uint64_t release_transition_sequence{};
    std::uint64_t released_qpc{};
};

struct VisualAudioEnvelope {
    std::uint32_t schema_version{1};
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::string stream_id;
    std::string segment_id;
    std::uint64_t source_sample_start{};
    std::uint32_t source_sample_count{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t device_write_qpc{};
    std::uint64_t qpc_frequency{};
    std::uint64_t source_frames{};
    std::uint64_t device_frames{};
    std::array<std::uint16_t, 8> mono_rms_q15{};
    std::array<std::uint16_t, 8> mono_peak_q15{};
    std::vector<playback::VisualSpeechCue> visual_speech_cues;
    bool active{};
    bool draining{};
    bool cancelled{};
};

enum class AudioOutputState : std::uint32_t {
    active = 1,
    disabled = 2,
    not_present = 3,
    unplugged = 4,
};

struct AudioOutputEndpoint {
    std::string endpoint_id;
    std::string friendly_name;
    AudioOutputState state{AudioOutputState::not_present};
    bool system_default{};
    std::uint64_t generation{};
};

struct AudioOutputSnapshot {
    std::uint32_t schema_version{1};
    std::uint64_t catalog_generation{};
    std::vector<AudioOutputEndpoint> endpoints;
};

struct SelectedAudioOutput {
    std::uint32_t schema_version{1};
    playback::AudioOutputSelectionMode mode{playback::AudioOutputSelectionMode::system_default};
    std::string requested_endpoint_id;
    AudioOutputEndpoint resolved;
};

enum class AudioInputState : std::uint32_t {
    active = 1,
    disabled = 2,
    not_present = 3,
    unplugged = 4,
};

struct AudioInputEndpoint {
    std::string endpoint_id;
    std::string friendly_name;
    AudioInputState state{AudioInputState::not_present};
    bool system_default{};
    std::uint64_t generation{};
};

struct AudioInputSnapshot {
    std::uint32_t schema_version{1};
    std::uint64_t catalog_generation{};
    std::vector<AudioInputEndpoint> endpoints;
};

struct SelectedAudioInput {
    std::uint32_t schema_version{1};
    playback::AudioOutputSelectionMode mode{playback::AudioOutputSelectionMode::system_default};
    std::string requested_endpoint_id;
    AudioInputEndpoint resolved;
};

struct TrustedSubtitlePresentationContext {
    std::uint32_t schema_version{1};
    std::uint32_t selected_process_id{};
    std::uint64_t selected_window{};
    std::string selected_executable_name;
    std::uint64_t capture_device_generation{};
    std::uint64_t geometry_epoch{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_frame_qpc{};
    RectI window_bounds_px;
    RectI client_bounds_px;
    SizeI captured_content_px;
    std::string monitor_id;
    RectI monitor_bounds_px;
    RectI monitor_work_area_px;
    bool dpi_available{};
    std::uint32_t dpi_x{};
    std::uint32_t dpi_y{};
    bool hdr_evidence_available{};
    bool hdr_supported{};
    bool hdr_user_enabled{};
    bool hdr_active{};
    bool advanced_color_active{};
    std::uint32_t active_color_mode{};
    bool color_encoding_available{};
    std::uint32_t color_encoding{};
    std::uint32_t bits_per_color_channel{};
    bool sdr_white_level_available{};
    double sdr_white_level_nits{};
    CaptureBackend capture_backend{CaptureBackend::none};
    std::uint32_t capture_scope{};
    bool overlay_capture_excluded{};
    bool overlay_visuals_allowed{};
    bool target_color_space_available{};
    ColorSpace target_color_space{ColorSpace::unknown};
    std::uint64_t attested_at_qpc{};
    std::uint64_t qpc_frequency{};
    std::uint64_t attestation_id{};
};

struct AllocatePlaybackStreamCommand {
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t max_frames{};
    std::uint32_t expected_producer_process_id{};
};

struct AllocateVisualSourceCommand {
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};
    bool reserve_for_actor_picker{};
};

struct ReleaseVisualSourceCommand {
    std::uint32_t worker_process_id{};
    std::uint64_t lease_nonce_high{};
    std::uint64_t lease_nonce_low{};
};

struct AllocateIdentityFrameCommand {
    std::uint32_t worker_process_id{};
    std::uint64_t worker_process_creation_time{};
    std::string worker_executable_name;
    RectI crop_px;
};

struct ReleaseIdentityFrameCommand {
    std::uint32_t worker_process_id{};
    std::string lease_id;
    std::string lease_nonce;
};

struct AllocateIdentityReferenceImportCommand {
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

struct ReleaseIdentityReferenceImportCommand {
    std::uint32_t worker_process_id{};
    std::string lease_id;
    std::string lease_nonce;
};

struct SelectTargetCommand {
    std::uint64_t native_window{};
    std::uint32_t expected_process_id{};
    std::vector<std::string> allowed_process_names;
};

struct ConfigurePttCommand { std::uint32_t virtual_key{}; };
struct CancelCommand { std::uint64_t new_generation{}; };

struct SubmitOcclusionCommand {
    double face_confidence{};
    double landmark_confidence{};
    double visibility_ratio{};
    bool mouth_occluded{};
    std::uint64_t measured_qpc{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_device_generation{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t source_frame_qpc{};
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};
};

struct SubmitPatchCommand {
    std::uint64_t source_frame_sequence{};
    std::uint64_t cancellation_generation{};
    double left{};
    double top{};
    double right{};
    double bottom{};
    double confidence{};
    std::uint64_t produced_qpc{};
    std::optional<SharedTextureDescriptor> shared_texture;
    std::uint64_t source_device_generation{};
    std::uint64_t source_frame_qpc{};
    std::uint64_t source_geometry_epoch{};
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};
};

enum class ResidualContractStatus : std::uint32_t {
    accepted = 0,
    unavailable_capture_path,
    missing_texture,
    session_mismatch,
    invalid_worker,
    invalid_lease_nonce,
    invalid_handle,
    invalid_adapter,
    invalid_mutex_keys,
    invalid_format,
    invalid_extent,
    wrong_cancellation_generation,
    wrong_device_generation,
    wrong_geometry_epoch,
    wrong_frame,
    wrong_frame_qpc,
    invalid_track,
    invalid_bounds,
    invalid_confidence,
    invalid_timing,
};

struct ResidualValidationContext {
    std::array<std::byte, launch_nonce_bytes> nonce{};
    std::string session_id;
    std::uint32_t broker_process_id{};
    std::uint32_t selected_target_process_id{};
    std::uint64_t cancellation_generation{};
    std::uint64_t device_generation{};
    std::uint64_t geometry_epoch{};
    std::uint64_t latest_frame_sequence{};
    std::uint64_t latest_frame_qpc{};
    SizeI latest_frame_size_px;
    std::uint64_t now_qpc{};
    std::uint64_t qpc_frequency{};
    CaptureBackend capture_backend{CaptureBackend::none};
    bool overlay_visuals_allowed{};
};

[[nodiscard]] ResidualContractStatus validate_residual_contract(
    const SubmitPatchCommand& command,
    const ResidualValidationContext& context) noexcept;
[[nodiscard]] std::string_view to_string(ResidualContractStatus status) noexcept;

using Command = std::variant<HealthCommand,
                             SelectTargetCommand,
                             ClearTargetCommand,
                             ConfigurePttCommand,
                             AudioStatusCommand,
                             SubmitOcclusionCommand,
                             SubmitPatchCommand,
                             CancelCommand,
                             DiagnosticsCommand,
                             CaptureEvidenceCommand,
                             AllocatePlaybackStreamCommand,
                             CancelPlaybackCommand,
                             EnumerateAudioOutputsCommand,
                             SelectAudioOutputCommand,
                             SelectedAudioOutputCommand,
                             TrustedSubtitlePresentationContextCommand,
                             EnumerateAudioInputsCommand,
                             SelectAudioInputCommand,
                             SelectedAudioInputCommand,
                             AllocateAudioInputRehearsalCommand,
                             CancelAudioInputRehearsalCommand,
                             QueryVisualAudioEnvelopeCommand,
                             QueryPttActivationStateCommand,
                             ManualActorPickerCommand,
                             AllocateVisualSourceCommand,
                             ReleaseVisualSourceCommand,
                             AllocateIdentityFrameCommand,
                             ReleaseIdentityFrameCommand,
                             AllocateIdentityReferenceImportCommand,
                             ReleaseIdentityReferenceImportCommand,
                             ShutdownCommand>;

struct Envelope {
    std::uint32_t version{protocol_version};
    std::array<std::byte, launch_nonce_bytes> nonce{};
    std::string session_id;
    std::uint64_t sequence{};
    std::uint64_t deadline_qpc{};
    std::uint64_t cancellation_generation{};
    CommandKind command{CommandKind::health};
    std::vector<std::byte> payload;
};

struct Response {
    std::uint32_t version{protocol_version};
    std::uint64_t response_to_sequence{};
    StatusCode status{StatusCode::ok};
    std::uint64_t cancellation_generation{};
    std::vector<std::byte> payload;
};

struct ValidationConfig {
    std::array<std::byte, launch_nonce_bytes> nonce{};
    std::string session_id;
    std::uint64_t maximum_future_qpc_ticks{};
};

class EnvelopeValidator final {
public:
    explicit EnvelopeValidator(ValidationConfig config);
    [[nodiscard]] StatusCode validate(const Envelope& envelope,
                                      std::uint64_t now_qpc,
                                      std::uint64_t cancellation_generation) noexcept;
    void reset() noexcept { last_sequence_ = 0; }
    [[nodiscard]] std::uint64_t last_sequence() const noexcept { return last_sequence_; }

private:
    ValidationConfig config_;
    std::uint64_t last_sequence_{};
};

[[nodiscard]] std::optional<std::vector<std::byte>> encode_envelope(const Envelope& envelope);
[[nodiscard]] std::optional<Envelope> decode_envelope(std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_response(const Response& response);
[[nodiscard]] std::optional<Response> decode_response(std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_command(CommandKind kind, const Command& command);
[[nodiscard]] std::optional<Command> decode_command(CommandKind kind, std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_playback_lease(
    const playback::PlaybackLease& lease);
[[nodiscard]] std::optional<playback::PlaybackLease> decode_playback_lease(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_audio_output_snapshot(
    const AudioOutputSnapshot& snapshot);
[[nodiscard]] std::optional<AudioOutputSnapshot> decode_audio_output_snapshot(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_selected_audio_output(
    const SelectedAudioOutput& selection);
[[nodiscard]] std::optional<SelectedAudioOutput> decode_selected_audio_output(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_trusted_subtitle_presentation_context(
    const TrustedSubtitlePresentationContext& context);
[[nodiscard]] std::optional<TrustedSubtitlePresentationContext>
decode_trusted_subtitle_presentation_context(std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_audio_input_snapshot(
    const AudioInputSnapshot& snapshot);
[[nodiscard]] std::optional<AudioInputSnapshot> decode_audio_input_snapshot(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_selected_audio_input(
    const SelectedAudioInput& selection);
[[nodiscard]] std::optional<SelectedAudioInput> decode_selected_audio_input(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_input_rehearsal_lease(
    const input::RehearsalLease& lease);
[[nodiscard]] std::optional<input::RehearsalLease> decode_input_rehearsal_lease(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_visual_audio_envelope(
    const VisualAudioEnvelope& envelope);
[[nodiscard]] std::optional<VisualAudioEnvelope> decode_visual_audio_envelope(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_ptt_activation_state(
    const PttActivationState& state);
[[nodiscard]] std::optional<PttActivationState> decode_ptt_activation_state(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_manual_actor_picker_receipt(
    const ManualActorPickerReceipt& receipt);
[[nodiscard]] std::optional<ManualActorPickerReceipt> decode_manual_actor_picker_receipt(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_visual_source_lease(
    const VisualSourceLease& lease);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_identity_frame_lease(
    const IdentityFrameLease& lease);
[[nodiscard]] std::optional<IdentityFrameLease> decode_identity_frame_lease(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_identity_reference_import_lease(
    const IdentityReferenceImportLease& lease);
[[nodiscard]] std::optional<IdentityReferenceImportLease> decode_identity_reference_import_lease(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<VisualSourceLease> decode_visual_source_lease(
    std::span<const std::byte> bytes);
[[nodiscard]] std::vector<std::byte> frame_message(std::span<const std::byte> message);
[[nodiscard]] std::optional<std::uint32_t> decode_frame_size(std::span<const std::byte, 4> prefix) noexcept;
[[nodiscard]] std::string_view to_string(StatusCode status) noexcept;

} // namespace npc::media::ipc
