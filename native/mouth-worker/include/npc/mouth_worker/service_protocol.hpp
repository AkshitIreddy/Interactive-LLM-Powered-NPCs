#pragma once

#include "npc/mouth_worker/landmark_provider.hpp"
#include "npc/mouth_worker/product_runtime.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace npc::mouth::service {

inline constexpr std::uint32_t protocol_magic = 0x3152574dU; // MWR framing magic, little endian
// Version 3 adds native detector candidates to worker responses. Keeping a
// distinct version makes old clients fail closed instead of interpreting the
// bounded candidate vector as response detail.
inline constexpr std::uint16_t protocol_version = 3U;
inline constexpr std::size_t session_nonce_bytes = 32U;
inline constexpr std::uint32_t maximum_atlas_bytes = 16U * 1024U * 1024U;
inline constexpr std::uint32_t maximum_message_bytes = maximum_atlas_bytes + 512U * 1024U;
inline constexpr std::uint32_t maximum_pcm_samples = 32U * 1024U;

enum class CommandKind : std::uint16_t {
    health = 1,
    render_current_frame = 2,
    cancel_generation = 3,
    acknowledge_residual = 4,
    shutdown = 5,
    // Uses the worker-owned, admitted native landmark provider. The caller
    // supplies only an identity-authoritative face ROI bound to this exact
    // leased frame; it cannot inject landmark coordinates.
    render_with_admitted_landmarks = 6,
    configure_admitted_landmark_provider = 7,
    install_character_mouth_atlas = 8,
    clear_character_mouth_atlas = 9,
    // Loads and immediately unloads an admitted provider for setup. This
    // command accepts no target process authority and can never render.
    self_test_admitted_landmark_provider = 10,
    // Runs the admitted detector against one broker-leased exact WGC frame.
    // The result is selection evidence only and can never produce a residual.
    discover_actor_candidates = 11,
};

enum class StatusCode : std::uint16_t {
    ok = 0,
    invalid_frame = 1,
    unsupported_version = 2,
    authentication_failed = 3,
    session_mismatch = 4,
    sequence_replayed = 5,
    deadline_expired = 6,
    cancellation_mismatch = 7,
    payload_invalid = 8,
    capability_unavailable = 9,
    internal_error = 10,
};

struct SessionBindingV1 {
    std::array<std::byte, session_nonce_bytes> nonce{};
    std::uint64_t session_id_high{};
    std::uint64_t session_id_low{};
};

struct SourceTextureLeaseV1 {
    std::uint32_t schema_version{1};
    SessionBindingV1 session;
    TextureLeaseDescriptor texture;
    std::uint32_t broker_process_id{};
    std::uint64_t broker_process_creation_time{};
    std::string broker_executable_name;
    std::uint64_t source_frame_qpc{};
    std::uint64_t qpc_frequency{};
};

struct RenderCurrentFrameCommandV1 {
    ProductRequestIdentity request;
    SourceTextureLeaseV1 source;
    TrackBinding track;
    FrameIdentity frame;
    OpenSeeFaceLandmarkPacketV1 landmarks;
    AppearanceGateEvidenceV1 appearance;
    VisualResourceStateV1 resources;
    MouthDrive drive;
    Nanoseconds deadline_ns{};
};

struct RenderWithAdmittedLandmarksCommandV1 {
    ProductRequestIdentity request;
    SourceTextureLeaseV1 source;
    TrackBinding track;
    FrameIdentity frame;
    NormalizedRect seed_face_bounds;
    AppearanceGateEvidenceV1 appearance;
    VisualResourceStateV1 resources;
    MouthDrive drive;
    Nanoseconds deadline_ns{};
    bool sealed_click_spatial_authority{};
};

struct CancelGenerationCommandV1 {
    std::uint64_t new_generation{};
};

struct ConfigureAdmittedLandmarkProviderCommandV1 {
    AdmittedLandmarkProviderLaunchV1 launch;
};

struct InstallCharacterMouthAtlasCommandV1 {
    CharacterMouthAtlas atlas;
};

struct DiscoverActorCandidatesCommandV1 {
    SourceTextureLeaseV1 source;
    TrackBinding discovery_track;
    FrameIdentity frame;
    Nanoseconds deadline_ns{};
};

struct DetectedActorCandidateV1 {
    std::uint64_t actor_id{};
    std::uint64_t track_id{};
    std::uint64_t track_epoch{};
    NormalizedRect bounds;
    double confidence{};
};

struct AcknowledgeResidualCommandV1 {
    std::uint64_t lease_nonce_high{};
    std::uint64_t lease_nonce_low{};
    bool presented{};
};

struct EnvelopeV1 {
    std::uint32_t magic{protocol_magic};
    std::uint16_t version{protocol_version};
    CommandKind command{CommandKind::health};
    SessionBindingV1 session;
    std::uint64_t sequence{};
    std::uint64_t cancellation_generation{};
    Nanoseconds deadline_ns{};
    std::vector<std::byte> payload;
};

struct ResidualProposalV1 {
    std::uint32_t schema_version{1};
    ProductRequestIdentity request;
    TrackBinding track;
    FrameIdentity source_frame;
    std::uint64_t source_frame_qpc{};
    NormalizedRect normalized_bounds;
    TextureLeaseDescriptor residual;
    double confidence{};
    // Schema 2 binds broker occlusion evidence to the worker-produced native
    // landmark packet. Schema 1 omitted these fields and remains decodable for
    // the supplied-packet reference path.
    double detector_confidence{};
    double landmark_confidence{};
    double visibility_ratio{};
    bool mouth_occluded{};
    Nanoseconds landmarks_measured_at_ns{};
    // Schema 3 proves which bounded audio interval drove this exact source
    // frame. A presentation broker can reject a late/replayed residual without
    // trusting the worker's visual output.
    AudioClockBinding audio_clock;
    Nanoseconds produced_at_ns{};
};

struct WorkerResponseV1 {
    std::uint32_t magic{protocol_magic};
    std::uint16_t version{protocol_version};
    StatusCode status{StatusCode::ok};
    std::uint64_t response_to_sequence{};
    std::uint64_t cancellation_generation{};
    PresentationReceiptV1 receipt;
    std::optional<ResidualProposalV1> residual;
    std::vector<DetectedActorCandidateV1> actor_candidates;
    std::string detail;
};

struct ValidationConfig {
    SessionBindingV1 session;
    Nanoseconds maximum_future_deadline_ns{5'000'000'000};
};

class EnvelopeValidator final {
public:
    explicit EnvelopeValidator(ValidationConfig config);
    [[nodiscard]] StatusCode validate(const EnvelopeV1& envelope,
                                      Nanoseconds now_ns,
                                      std::uint64_t active_generation) noexcept;
    [[nodiscard]] std::uint64_t last_sequence() const noexcept;

private:
    ValidationConfig config_;
    std::uint64_t last_sequence_{};
};

[[nodiscard]] std::optional<std::vector<std::byte>> encode_envelope(
    const EnvelopeV1& envelope);
[[nodiscard]] std::optional<EnvelopeV1> decode_envelope(std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_response(
    const WorkerResponseV1& response);
[[nodiscard]] std::optional<WorkerResponseV1> decode_response(std::span<const std::byte> bytes);

[[nodiscard]] std::optional<std::vector<std::byte>> encode_render_command(
    const RenderCurrentFrameCommandV1& command);
[[nodiscard]] std::optional<RenderCurrentFrameCommandV1> decode_render_command(
    std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_admitted_render_command(
    const RenderWithAdmittedLandmarksCommandV1& command);
[[nodiscard]] std::optional<RenderWithAdmittedLandmarksCommandV1>
decode_admitted_render_command(std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_provider_configuration(
    const ConfigureAdmittedLandmarkProviderCommandV1& command);
[[nodiscard]] std::optional<ConfigureAdmittedLandmarkProviderCommandV1>
decode_provider_configuration(std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_character_mouth_atlas(
    const InstallCharacterMouthAtlasCommandV1& command);
[[nodiscard]] std::optional<InstallCharacterMouthAtlasCommandV1>
decode_character_mouth_atlas(std::span<const std::byte> bytes);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_actor_candidate_discovery(
    const DiscoverActorCandidatesCommandV1& command);
[[nodiscard]] std::optional<DiscoverActorCandidatesCommandV1>
decode_actor_candidate_discovery(std::span<const std::byte> bytes);
[[nodiscard]] std::vector<std::byte> encode_cancel_command(
    const CancelGenerationCommandV1& command);
[[nodiscard]] std::optional<CancelGenerationCommandV1> decode_cancel_command(
    std::span<const std::byte> bytes);
[[nodiscard]] std::vector<std::byte> encode_acknowledgement(
    const AcknowledgeResidualCommandV1& command);
[[nodiscard]] std::optional<AcknowledgeResidualCommandV1> decode_acknowledgement(
    std::span<const std::byte> bytes);

[[nodiscard]] std::vector<std::byte> frame_message(std::span<const std::byte> message);
[[nodiscard]] std::optional<std::uint32_t> decode_frame_size(
    std::span<const std::byte, 4> prefix) noexcept;

[[nodiscard]] constexpr std::string_view to_string(const StatusCode value) noexcept {
    switch (value) {
    case StatusCode::ok: return "ok";
    case StatusCode::invalid_frame: return "invalid_frame";
    case StatusCode::unsupported_version: return "unsupported_version";
    case StatusCode::authentication_failed: return "authentication_failed";
    case StatusCode::session_mismatch: return "session_mismatch";
    case StatusCode::sequence_replayed: return "sequence_replayed";
    case StatusCode::deadline_expired: return "deadline_expired";
    case StatusCode::cancellation_mismatch: return "cancellation_mismatch";
    case StatusCode::payload_invalid: return "payload_invalid";
    case StatusCode::capability_unavailable: return "capability_unavailable";
    case StatusCode::internal_error: return "internal_error";
    }
    return "unknown";
}

} // namespace npc::mouth::service
