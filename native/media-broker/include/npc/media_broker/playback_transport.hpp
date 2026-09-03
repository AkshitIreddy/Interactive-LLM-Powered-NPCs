#pragma once

#include "npc/media_broker/audio_ring.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace npc::media::playback {

// V2 adds authoritative audio-output identity/generation to every lease and
// terminal receipt. V1 is intentionally rejected because it cannot prove which
// Windows render endpoint received the submitted frames.
inline constexpr std::uint32_t schema_version = 2;
inline constexpr std::uint32_t maximum_chunk_bytes = 64U * 1024U;
inline constexpr std::uint32_t maximum_wire_frame_bytes = maximum_chunk_bytes + 4096U;
inline constexpr std::size_t maximum_endpoint_id_bytes = 1024;
inline constexpr std::uint64_t maximum_source_frames = 192'000ULL * 60ULL * 10ULL;
inline constexpr std::uint32_t minimum_sample_rate = 8'000;
inline constexpr std::uint32_t maximum_sample_rate = 192'000;
inline constexpr std::uint16_t maximum_channels = 2;
inline constexpr std::size_t authentication_token_bytes = 32;

using AuthenticationToken = std::array<std::byte, authentication_token_bytes>;

enum class AudioOutputSelectionMode : std::uint8_t {
    system_default = 1,
    endpoint_id = 2,
};

enum class ProducerCommand : std::uint16_t {
    begin = 1,
    chunk = 2,
    finish = 3,
    cancel = 4,
};

enum class ProducerStatus : std::uint16_t {
    ok = 0,
    invalid_frame = 1,
    authentication_failed = 2,
    producer_mismatch = 3,
    identity_mismatch = 4,
    sequence_replayed = 5,
    deadline_expired = 6,
    deadline_too_far = 7,
    invalid_state = 8,
    chunk_too_large = 9,
    frame_budget_exceeded = 10,
    backpressure = 11,
    device_unavailable = 12,
    cancelled = 13,
    drain_timeout = 14,
};

enum class SessionState : std::uint8_t {
    allocated,
    streaming,
    source_finished,
    drained,
    cancelled,
    failed,
};

struct AllocationRequest {
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t max_frames{};
    std::uint32_t expected_producer_process_id{};
};

struct PlaybackLease {
    std::uint32_t schema_version{playback::schema_version};
    std::string stream_id;
    std::string producer_endpoint;
    AuthenticationToken one_time_token{};
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t max_frames{};
    std::uint32_t max_chunk_bytes{maximum_chunk_bytes};
    std::uint64_t expires_qpc{};
    AudioOutputSelectionMode output_selection_mode{AudioOutputSelectionMode::system_default};
    std::string output_endpoint_id;
    std::uint64_t output_endpoint_generation{};
};

struct ProducerEnvelope {
    std::uint32_t schema_version{playback::schema_version};
    ProducerCommand command{ProducerCommand::begin};
    std::uint64_t sequence{};
    std::uint64_t deadline_qpc{};
    std::uint64_t generation{};
    std::string stream_id;
    std::string session_id;
    std::string turn_id;
    AuthenticationToken token{};
    std::vector<std::byte> payload;
};

struct PlaybackReceipt {
    std::uint32_t schema_version{playback::schema_version};
    std::string receipt_id;
    std::string stream_id;
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::uint64_t source_frames{};
    std::uint64_t device_frames{};
    std::uint64_t source_duration_micros{};
    bool source_submission_complete{};
    bool endpoint_drain_complete{};
    bool cancelled{};
    AudioOutputSelectionMode output_selection_mode{AudioOutputSelectionMode::system_default};
    std::string output_endpoint_id;
    std::uint64_t output_endpoint_generation{};
};

struct ProducerResponse {
    std::uint32_t schema_version{playback::schema_version};
    std::uint64_t response_to_sequence{};
    ProducerStatus status{ProducerStatus::ok};
    std::uint64_t accepted_source_frames{};
    std::optional<PlaybackReceipt> receipt;
};

struct ValidationPolicy {
    std::uint64_t qpc_frequency{};
    std::uint64_t maximum_future_deadline_ticks{};
};

[[nodiscard]] bool valid_allocation(const AllocationRequest& request) noexcept;
[[nodiscard]] bool constant_time_equal(const AuthenticationToken& left,
                                       const AuthenticationToken& right) noexcept;
void clear_authentication_token(AuthenticationToken& token) noexcept;
[[nodiscard]] std::string_view to_string(ProducerStatus status) noexcept;

// Binary producer wire contract. Each encoded body is prefixed by a little-endian
// u32 body length. The decoder rejects unknown commands and any frame larger than
// maximum_wire_frame_bytes before allocating its payload.
[[nodiscard]] std::optional<std::vector<std::byte>> encode_envelope(const ProducerEnvelope& envelope);
[[nodiscard]] std::optional<ProducerEnvelope> decode_envelope(std::span<const std::byte> body);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_response(const ProducerResponse& response);
[[nodiscard]] std::optional<ProducerResponse> decode_response(std::span<const std::byte> body);
[[nodiscard]] std::optional<std::uint32_t> decode_frame_size(std::span<const std::byte, 4> prefix) noexcept;

// Portable, single-producer lifecycle and security policy. The Windows endpoint
// owns the pipe and WASAPI client; this class owns authentication, ordering,
// budgets, cancellation semantics, and the source PCM ring.
class PlaybackSession final {
public:
    PlaybackSession(PlaybackLease lease,
                    std::uint32_t expected_producer_process_id,
                    std::uint32_t ring_capacity_frames,
                    ValidationPolicy policy);
    ~PlaybackSession();
    PlaybackSession(const PlaybackSession&) = delete;
    PlaybackSession& operator=(const PlaybackSession&) = delete;

    [[nodiscard]] ProducerResponse process(const ProducerEnvelope& envelope,
                                           std::uint32_t actual_producer_process_id,
                                           std::uint64_t now_qpc);
    void record_device_frames(std::uint64_t frames) noexcept;
    void mark_device_drained() noexcept;
    void mark_device_lost() noexcept;
    void cancel() noexcept;

    [[nodiscard]] SessionState state() const noexcept { return state_; }
    [[nodiscard]] const PlaybackLease& lease() const noexcept { return lease_; }
    [[nodiscard]] SharedPcmRing& ring() noexcept { return ring_; }
    [[nodiscard]] const SharedPcmRing& ring() const noexcept { return ring_; }
    [[nodiscard]] PlaybackReceipt receipt() const;
    [[nodiscard]] std::uint64_t source_frames() const noexcept { return source_frames_; }
    [[nodiscard]] std::uint64_t device_frames() const noexcept { return device_frames_; }

private:
    [[nodiscard]] ProducerStatus authenticate(const ProducerEnvelope& envelope,
                                              std::uint32_t actual_producer_process_id,
                                              std::uint64_t now_qpc) noexcept;
    [[nodiscard]] ProducerResponse respond(std::uint64_t sequence,
                                           ProducerStatus status,
                                           std::uint64_t accepted = 0) const;

    PlaybackLease lease_;
    std::uint32_t expected_producer_process_id_{};
    ValidationPolicy policy_;
    SharedPcmRing ring_;
    SessionState state_{SessionState::allocated};
    std::uint64_t last_sequence_{};
    std::uint64_t source_frames_{};
    std::uint64_t device_frames_{};
    bool token_consumed_{};
    bool source_submission_complete_{};
    bool endpoint_drain_complete_{};
    std::uint64_t stream_deadline_qpc_{};
};

} // namespace npc::media::playback
