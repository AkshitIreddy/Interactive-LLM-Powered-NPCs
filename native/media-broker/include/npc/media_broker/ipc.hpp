#pragma once

#include "npc/media_broker/types.hpp"

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
};

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
    std::uint64_t source_process_handle_value{};
    std::uint64_t adapter_luid{};
    std::uint64_t keyed_mutex_acquire_key{};
    std::uint64_t keyed_mutex_release_key{};
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t dxgi_format{};
};

struct HealthCommand {};
struct ClearTargetCommand {};
struct AudioStatusCommand {};
struct DiagnosticsCommand {};
struct ShutdownCommand {};

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
};

using Command = std::variant<HealthCommand,
                             SelectTargetCommand,
                             ClearTargetCommand,
                             ConfigurePttCommand,
                             AudioStatusCommand,
                             SubmitOcclusionCommand,
                             SubmitPatchCommand,
                             CancelCommand,
                             DiagnosticsCommand,
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
[[nodiscard]] std::vector<std::byte> frame_message(std::span<const std::byte> message);
[[nodiscard]] std::optional<std::uint32_t> decode_frame_size(std::span<const std::byte, 4> prefix) noexcept;
[[nodiscard]] std::string_view to_string(StatusCode status) noexcept;

} // namespace npc::media::ipc
