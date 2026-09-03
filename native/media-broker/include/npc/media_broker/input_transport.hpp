#pragma once

#include "npc/media_broker/playback_transport.hpp"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <vector>

namespace npc::media::input {

inline constexpr std::uint32_t schema_version = 2;
inline constexpr std::uint32_t minimum_rehearsal_duration_ms = 250;
inline constexpr std::uint32_t maximum_rehearsal_duration_ms = 10'000;
inline constexpr std::uint32_t maximum_chunk_bytes = 64U * 1024U;

enum class ActivationSource : std::uint8_t {
    explicit_rehearsal = 1,
    push_to_talk = 2,
};

struct RehearsalLease {
    std::uint32_t schema_version{input::schema_version};
    std::string stream_id;
    std::string producer_endpoint;
    playback::AuthenticationToken one_time_token{};
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::uint32_t duration_ms{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{1};
    std::uint64_t max_frames{};
    std::uint32_t max_chunk_bytes{input::maximum_chunk_bytes};
    std::uint64_t expires_qpc{};
    std::uint64_t qpc_frequency{};
    playback::AudioOutputSelectionMode input_selection_mode{
        playback::AudioOutputSelectionMode::system_default};
    std::string input_endpoint_id;
    std::uint64_t input_endpoint_generation{};
    ActivationSource activation_source{ActivationSource::explicit_rehearsal};
    std::uint32_t ptt_virtual_key{};
    std::uint64_t ptt_press_transition_sequence{};
    std::uint64_t ptt_pressed_qpc{};
};

struct StreamHello {
    std::uint32_t schema_version{input::schema_version};
    std::string stream_id;
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t max_frames{};
    std::uint32_t max_chunk_bytes{input::maximum_chunk_bytes};
    std::uint64_t qpc_frequency{};
    ActivationSource activation_source{ActivationSource::explicit_rehearsal};
    std::uint32_t ptt_virtual_key{};
    std::uint64_t ptt_press_transition_sequence{};
    std::uint64_t ptt_pressed_qpc{};
};

struct PcmChunk {
    std::uint32_t schema_version{input::schema_version};
    std::uint64_t sequence{};
    std::uint64_t first_frame_qpc{};
    std::uint64_t first_frame_index{};
    std::uint32_t frame_count{};
    std::vector<std::byte> pcm_s16le;
};

enum class AckAction : std::uint8_t { continue_stream = 0, stop = 1, cancel = 2 };

struct ConsumerAck {
    std::uint32_t schema_version{input::schema_version};
    std::uint64_t sequence{};
    AckAction action{AckAction::continue_stream};
};

struct RehearsalReceipt {
    std::uint32_t schema_version{input::schema_version};
    std::string receipt_id;
    std::string stream_id;
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    playback::AudioOutputSelectionMode input_selection_mode{
        playback::AudioOutputSelectionMode::system_default};
    std::string input_endpoint_id;
    std::uint64_t input_endpoint_generation{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t captured_frames{};
    std::uint64_t captured_duration_micros{};
    std::int32_t peak_milli_dbfs{-120'000};
    std::int32_t rms_milli_dbfs{-120'000};
    std::uint64_t clipped_samples{};
    std::uint64_t silent_frames{};
    bool source_capture_complete{};
    bool silence_detected{};
    bool clipping_detected{};
    bool cancelled{};
    bool device_lost{};
    ActivationSource activation_source{ActivationSource::explicit_rehearsal};
    std::uint32_t ptt_virtual_key{};
    std::uint64_t ptt_press_transition_sequence{};
    std::uint64_t ptt_pressed_qpc{};
    std::uint64_t ptt_release_transition_sequence{};
    std::uint64_t ptt_released_qpc{};
};

[[nodiscard]] bool valid_lease(const RehearsalLease& lease) noexcept;
[[nodiscard]] bool valid_receipt(const RehearsalReceipt& receipt) noexcept;
[[nodiscard]] std::optional<std::vector<std::byte>> encode_receipt(
    const RehearsalReceipt& receipt);
[[nodiscard]] std::optional<RehearsalReceipt> decode_receipt(
    std::span<const std::byte> body);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_hello(const StreamHello& hello);
[[nodiscard]] std::optional<StreamHello> decode_hello(std::span<const std::byte> body);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_chunk(const PcmChunk& chunk,
                                                                 std::uint16_t channels);
[[nodiscard]] std::optional<PcmChunk> decode_chunk(std::span<const std::byte> body,
                                                  std::uint16_t channels);
[[nodiscard]] std::optional<std::vector<std::byte>> encode_ack(const ConsumerAck& ack);
[[nodiscard]] std::optional<ConsumerAck> decode_ack(std::span<const std::byte> body);

} // namespace npc::media::input
