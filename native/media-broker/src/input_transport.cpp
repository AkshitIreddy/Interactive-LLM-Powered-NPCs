#include "npc/media_broker/input_transport.hpp"

#include <algorithm>
#include <array>
#include <limits>
#include <type_traits>

namespace npc::media::input {

namespace {

constexpr std::array<std::byte, 4> receipt_magic{
    std::byte{'N'}, std::byte{'P'}, std::byte{'I'}, std::byte{'R'}};
constexpr std::array<std::byte, 4> hello_magic{
    std::byte{'N'}, std::byte{'P'}, std::byte{'I'}, std::byte{'H'}};
constexpr std::array<std::byte, 4> chunk_magic{
    std::byte{'N'}, std::byte{'P'}, std::byte{'I'}, std::byte{'C'}};
constexpr std::array<std::byte, 4> ack_magic{
    std::byte{'N'}, std::byte{'P'}, std::byte{'I'}, std::byte{'A'}};

template <typename T>
void append_little(std::vector<std::byte>& output, const T value) {
    static_assert(std::is_unsigned_v<T>);
    for (unsigned shift = 0; shift < sizeof(T) * 8U; shift += 8U) {
        output.push_back(static_cast<std::byte>((value >> shift) & 0xffU));
    }
}

template <typename T>
std::optional<T> read_little(const std::span<const std::byte> bytes,
                             std::size_t& position) {
    static_assert(std::is_unsigned_v<T>);
    if (position > bytes.size() || bytes.size() - position < sizeof(T)) return std::nullopt;
    T value{};
    for (unsigned shift = 0; shift < sizeof(T) * 8U; shift += 8U) {
        value |= static_cast<T>(std::to_integer<unsigned char>(bytes[position++])) << shift;
    }
    return value;
}

bool append_string(std::vector<std::byte>& output, const std::string_view value,
                   const std::size_t maximum) {
    if (value.empty() || value.size() > maximum ||
        value.size() > std::numeric_limits<std::uint16_t>::max() ||
        value.find('\0') != std::string_view::npos) return false;
    append_little(output, static_cast<std::uint16_t>(value.size()));
    output.insert(output.end(), reinterpret_cast<const std::byte*>(value.data()),
                  reinterpret_cast<const std::byte*>(value.data() + value.size()));
    return true;
}

std::optional<std::string> read_string(const std::span<const std::byte> bytes,
                                       std::size_t& position,
                                       const std::size_t maximum) {
    const auto size = read_little<std::uint16_t>(bytes, position);
    if (!size || *size == 0 || *size > maximum ||
        position > bytes.size() || bytes.size() - position < *size) return std::nullopt;
    const auto* begin = reinterpret_cast<const char*>(bytes.data() + position);
    std::string value(begin, begin + *size);
    position += *size;
    return value.find('\0') == std::string::npos ? std::optional{std::move(value)}
                                                 : std::nullopt;
}

bool valid_mode(const playback::AudioOutputSelectionMode mode) noexcept {
    return mode == playback::AudioOutputSelectionMode::system_default ||
           mode == playback::AudioOutputSelectionMode::endpoint_id;
}

bool valid_activation(const ActivationSource source, const std::uint32_t virtual_key,
                      const std::uint64_t press_sequence,
                      const std::uint64_t pressed_qpc) noexcept {
    return (source == ActivationSource::explicit_rehearsal && virtual_key == 0 &&
            press_sequence == 0 && pressed_qpc == 0) ||
           (source == ActivationSource::push_to_talk && virtual_key > 0 &&
            virtual_key <= 0xffU && press_sequence > 0 && pressed_qpc > 0);
}

} // namespace

bool valid_lease(const RehearsalLease& lease) noexcept {
    return lease.schema_version == schema_version && !lease.stream_id.empty() &&
           lease.stream_id.size() <= 128 && !lease.producer_endpoint.empty() &&
           lease.producer_endpoint.size() <= 256 && !lease.session_id.empty() &&
           lease.session_id.size() <= 128 && !lease.turn_id.empty() &&
           lease.turn_id.size() <= 128 && lease.generation > 0 &&
           lease.duration_ms >= minimum_rehearsal_duration_ms &&
           lease.duration_ms <= maximum_rehearsal_duration_ms &&
           lease.sample_rate >= playback::minimum_sample_rate &&
           lease.sample_rate <= playback::maximum_sample_rate && lease.channels > 0 &&
           lease.channels <= playback::maximum_channels && lease.max_frames > 0 &&
           lease.max_frames <= static_cast<std::uint64_t>(lease.sample_rate) *
                                   maximum_rehearsal_duration_ms / 1000U &&
           lease.max_chunk_bytes == maximum_chunk_bytes && lease.expires_qpc > 0 &&
           lease.qpc_frequency > 0 &&
           valid_mode(lease.input_selection_mode) && !lease.input_endpoint_id.empty() &&
           lease.input_endpoint_id.size() <= playback::maximum_endpoint_id_bytes &&
           lease.input_endpoint_id.find('\0') == std::string::npos &&
           lease.input_endpoint_generation > 0 &&
           valid_activation(lease.activation_source, lease.ptt_virtual_key,
                            lease.ptt_press_transition_sequence, lease.ptt_pressed_qpc);
}

bool valid_receipt(const RehearsalReceipt& receipt) noexcept {
    const bool metrics_valid = receipt.sample_rate >= playback::minimum_sample_rate &&
        receipt.sample_rate <= playback::maximum_sample_rate && receipt.channels > 0 &&
        receipt.channels <= 32 && receipt.peak_milli_dbfs >= -120'000 &&
        receipt.peak_milli_dbfs <= 0 && receipt.rms_milli_dbfs >= -120'000 &&
        receipt.rms_milli_dbfs <= 0 && receipt.captured_frames <=
            static_cast<std::uint64_t>(playback::maximum_sample_rate) *
                maximum_rehearsal_duration_ms / 1000U &&
        receipt.silent_frames <= receipt.captured_frames &&
        receipt.clipped_samples <= receipt.captured_frames * receipt.channels;
    const bool activation_valid = valid_activation(
        receipt.activation_source, receipt.ptt_virtual_key,
        receipt.ptt_press_transition_sequence, receipt.ptt_pressed_qpc) &&
        ((receipt.activation_source == ActivationSource::explicit_rehearsal &&
          receipt.ptt_release_transition_sequence == 0 && receipt.ptt_released_qpc == 0) ||
         (receipt.activation_source == ActivationSource::push_to_talk &&
          ((receipt.ptt_release_transition_sequence == 0 && receipt.ptt_released_qpc == 0) ||
           (receipt.ptt_release_transition_sequence > receipt.ptt_press_transition_sequence &&
            receipt.ptt_released_qpc >= receipt.ptt_pressed_qpc)))) &&
        (!receipt.source_capture_complete ||
         receipt.activation_source == ActivationSource::explicit_rehearsal ||
         receipt.ptt_release_transition_sequence > receipt.ptt_press_transition_sequence);
    return receipt.schema_version == schema_version && !receipt.receipt_id.empty() &&
           receipt.receipt_id.size() <= 128 && !receipt.stream_id.empty() &&
           receipt.stream_id.size() <= 128 && !receipt.session_id.empty() &&
           receipt.session_id.size() <= 128 && !receipt.turn_id.empty() &&
           receipt.turn_id.size() <= 128 && receipt.generation > 0 &&
           valid_mode(receipt.input_selection_mode) && !receipt.input_endpoint_id.empty() &&
           receipt.input_endpoint_id.size() <= playback::maximum_endpoint_id_bytes &&
           receipt.input_endpoint_generation > 0 && metrics_valid &&
           activation_valid &&
           !(receipt.source_capture_complete && (receipt.cancelled || receipt.device_lost));
}

std::optional<std::vector<std::byte>> encode_receipt(const RehearsalReceipt& receipt) {
    if (!valid_receipt(receipt)) return std::nullopt;
    std::vector<std::byte> body;
    body.insert(body.end(), receipt_magic.begin(), receipt_magic.end());
    append_little(body, receipt.schema_version);
    if (!append_string(body, receipt.receipt_id, 128) ||
        !append_string(body, receipt.stream_id, 128) ||
        !append_string(body, receipt.session_id, 128) ||
        !append_string(body, receipt.turn_id, 128)) return std::nullopt;
    append_little(body, receipt.generation);
    body.push_back(static_cast<std::byte>(receipt.input_selection_mode));
    if (!append_string(body, receipt.input_endpoint_id,
                       playback::maximum_endpoint_id_bytes)) return std::nullopt;
    append_little(body, receipt.input_endpoint_generation);
    append_little(body, receipt.sample_rate);
    append_little(body, receipt.channels);
    append_little(body, receipt.captured_frames);
    append_little(body, receipt.captured_duration_micros);
    append_little(body, static_cast<std::uint32_t>(receipt.peak_milli_dbfs));
    append_little(body, static_cast<std::uint32_t>(receipt.rms_milli_dbfs));
    append_little(body, receipt.clipped_samples);
    append_little(body, receipt.silent_frames);
    body.push_back(static_cast<std::byte>(receipt.source_capture_complete));
    body.push_back(static_cast<std::byte>(receipt.silence_detected));
    body.push_back(static_cast<std::byte>(receipt.clipping_detected));
    body.push_back(static_cast<std::byte>(receipt.cancelled));
    body.push_back(static_cast<std::byte>(receipt.device_lost));
    body.push_back(static_cast<std::byte>(receipt.activation_source));
    append_little(body, receipt.ptt_virtual_key);
    append_little(body, receipt.ptt_press_transition_sequence);
    append_little(body, receipt.ptt_pressed_qpc);
    append_little(body, receipt.ptt_release_transition_sequence);
    append_little(body, receipt.ptt_released_qpc);
    return body;
}

std::optional<RehearsalReceipt> decode_receipt(const std::span<const std::byte> body) {
    if (body.size() < 64 || body.size() > 4096 ||
        !std::equal(receipt_magic.begin(), receipt_magic.end(), body.begin())) return std::nullopt;
    std::size_t position = receipt_magic.size();
    RehearsalReceipt receipt;
    const auto version = read_little<std::uint32_t>(body, position);
    auto receipt_id = read_string(body, position, 128);
    auto stream_id = read_string(body, position, 128);
    auto session_id = read_string(body, position, 128);
    auto turn_id = read_string(body, position, 128);
    const auto generation = read_little<std::uint64_t>(body, position);
    if (!version || !receipt_id || !stream_id || !session_id || !turn_id || !generation ||
        position >= body.size()) return std::nullopt;
    const auto mode = std::to_integer<unsigned char>(body[position++]);
    auto endpoint_id = read_string(body, position, playback::maximum_endpoint_id_bytes);
    const auto endpoint_generation = read_little<std::uint64_t>(body, position);
    const auto sample_rate = read_little<std::uint32_t>(body, position);
    const auto channels = read_little<std::uint16_t>(body, position);
    const auto frames = read_little<std::uint64_t>(body, position);
    const auto duration = read_little<std::uint64_t>(body, position);
    const auto peak = read_little<std::uint32_t>(body, position);
    const auto rms = read_little<std::uint32_t>(body, position);
    const auto clipped = read_little<std::uint64_t>(body, position);
    const auto silent = read_little<std::uint64_t>(body, position);
    if (!endpoint_id || !endpoint_generation || !sample_rate || !channels || !frames ||
        !duration || !peak || !rms || !clipped || !silent || body.size() - position != 42 ||
        (mode != 1 && mode != 2)) return std::nullopt;
    std::array<unsigned char, 5> flags{};
    for (auto& flag : flags) {
        flag = std::to_integer<unsigned char>(body[position++]);
        if (flag > 1) return std::nullopt;
    }
    const auto activation = std::to_integer<unsigned char>(body[position++]);
    const auto virtual_key = read_little<std::uint32_t>(body, position);
    const auto press_sequence = read_little<std::uint64_t>(body, position);
    const auto pressed_qpc = read_little<std::uint64_t>(body, position);
    const auto release_sequence = read_little<std::uint64_t>(body, position);
    const auto released_qpc = read_little<std::uint64_t>(body, position);
    if (!virtual_key || !press_sequence || !pressed_qpc || !release_sequence ||
        !released_qpc || position != body.size() ||
        (activation != 1 && activation != 2)) return std::nullopt;
    receipt = {*version, std::move(*receipt_id), std::move(*stream_id), std::move(*session_id),
               std::move(*turn_id), *generation,
               static_cast<playback::AudioOutputSelectionMode>(mode),
               std::move(*endpoint_id), *endpoint_generation, *sample_rate, *channels,
               *frames, *duration, static_cast<std::int32_t>(*peak),
               static_cast<std::int32_t>(*rms), *clipped, *silent,
               flags[0] != 0, flags[1] != 0, flags[2] != 0, flags[3] != 0,
               flags[4] != 0, static_cast<ActivationSource>(activation), *virtual_key,
               *press_sequence, *pressed_qpc, *release_sequence, *released_qpc};
    return valid_receipt(receipt) ? std::optional{std::move(receipt)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_hello(const StreamHello& hello) {
    const RehearsalLease shape{schema_version, hello.stream_id, "pipe", {}, hello.session_id,
        hello.turn_id, hello.generation, minimum_rehearsal_duration_ms, hello.sample_rate,
        hello.channels, hello.max_frames, hello.max_chunk_bytes, 1, hello.qpc_frequency,
        playback::AudioOutputSelectionMode::system_default, "endpoint", 1,
        hello.activation_source, hello.ptt_virtual_key,
        hello.ptt_press_transition_sequence, hello.ptt_pressed_qpc};
    if (!valid_lease(shape)) return std::nullopt;
    std::vector<std::byte> body;
    body.insert(body.end(), hello_magic.begin(), hello_magic.end());
    append_little(body, hello.schema_version);
    if (!append_string(body, hello.stream_id, 128) ||
        !append_string(body, hello.session_id, 128) ||
        !append_string(body, hello.turn_id, 128)) return std::nullopt;
    append_little(body, hello.generation);
    append_little(body, hello.sample_rate);
    append_little(body, hello.channels);
    append_little(body, hello.max_frames);
    append_little(body, hello.max_chunk_bytes);
    append_little(body, hello.qpc_frequency);
    body.push_back(static_cast<std::byte>(hello.activation_source));
    append_little(body, hello.ptt_virtual_key);
    append_little(body, hello.ptt_press_transition_sequence);
    append_little(body, hello.ptt_pressed_qpc);
    return body;
}

std::optional<StreamHello> decode_hello(const std::span<const std::byte> body) {
    if (body.size() < 48 || body.size() > 1024 ||
        !std::equal(hello_magic.begin(), hello_magic.end(), body.begin())) return std::nullopt;
    std::size_t position = hello_magic.size();
    const auto version = read_little<std::uint32_t>(body, position);
    auto stream = read_string(body, position, 128);
    auto session = read_string(body, position, 128);
    auto turn = read_string(body, position, 128);
    const auto generation = read_little<std::uint64_t>(body, position);
    const auto rate = read_little<std::uint32_t>(body, position);
    const auto channels = read_little<std::uint16_t>(body, position);
    const auto frames = read_little<std::uint64_t>(body, position);
    const auto chunk = read_little<std::uint32_t>(body, position);
    const auto frequency = read_little<std::uint64_t>(body, position);
    if (!version || *version != schema_version || !stream || !session || !turn ||
        !generation || !rate || !channels || !frames || !chunk || !frequency ||
        position >= body.size()) return std::nullopt;
    const auto activation = std::to_integer<unsigned char>(body[position++]);
    const auto virtual_key = read_little<std::uint32_t>(body, position);
    const auto press_sequence = read_little<std::uint64_t>(body, position);
    const auto pressed_qpc = read_little<std::uint64_t>(body, position);
    if (!virtual_key || !press_sequence || !pressed_qpc || position != body.size() ||
        (activation != 1 && activation != 2)) return std::nullopt;
    StreamHello hello{*version, std::move(*stream), std::move(*session), std::move(*turn),
                      *generation, *rate, *channels, *frames, *chunk, *frequency,
                      static_cast<ActivationSource>(activation), *virtual_key,
                      *press_sequence, *pressed_qpc};
    return encode_hello(hello).has_value() ? std::optional{std::move(hello)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_chunk(const PcmChunk& chunk,
                                                    const std::uint16_t channels) {
    const auto bytes_per_frame = static_cast<std::uint32_t>(channels) * 2U;
    if (chunk.schema_version != schema_version || chunk.sequence == 0 ||
        chunk.first_frame_qpc == 0 || chunk.frame_count == 0 || channels == 0 ||
        channels > playback::maximum_channels || chunk.pcm_s16le.empty() ||
        chunk.pcm_s16le.size() > maximum_chunk_bytes ||
        chunk.pcm_s16le.size() != static_cast<std::size_t>(chunk.frame_count) * bytes_per_frame) {
        return std::nullopt;
    }
    std::vector<std::byte> body;
    body.insert(body.end(), chunk_magic.begin(), chunk_magic.end());
    append_little(body, chunk.schema_version);
    append_little(body, chunk.sequence);
    append_little(body, chunk.first_frame_qpc);
    append_little(body, chunk.first_frame_index);
    append_little(body, chunk.frame_count);
    append_little(body, static_cast<std::uint32_t>(chunk.pcm_s16le.size()));
    body.insert(body.end(), chunk.pcm_s16le.begin(), chunk.pcm_s16le.end());
    return body;
}

std::optional<PcmChunk> decode_chunk(const std::span<const std::byte> body,
                                     const std::uint16_t channels) {
    if (body.size() < 40 || body.size() > maximum_chunk_bytes + 40U ||
        !std::equal(chunk_magic.begin(), chunk_magic.end(), body.begin())) return std::nullopt;
    std::size_t position = chunk_magic.size();
    const auto version = read_little<std::uint32_t>(body, position);
    const auto sequence = read_little<std::uint64_t>(body, position);
    const auto qpc = read_little<std::uint64_t>(body, position);
    const auto index = read_little<std::uint64_t>(body, position);
    const auto frames = read_little<std::uint32_t>(body, position);
    const auto size = read_little<std::uint32_t>(body, position);
    if (!version || !sequence || !qpc || !index || !frames || !size ||
        body.size() - position != *size) return std::nullopt;
    PcmChunk chunk{*version, *sequence, *qpc, *index, *frames,
                   {body.begin() + static_cast<std::ptrdiff_t>(position), body.end()}};
    return encode_chunk(chunk, channels).has_value() ? std::optional{std::move(chunk)}
                                                     : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_ack(const ConsumerAck& ack) {
    if (ack.schema_version != schema_version || ack.sequence == 0 ||
        ack.action > AckAction::cancel) return std::nullopt;
    std::vector<std::byte> body;
    body.insert(body.end(), ack_magic.begin(), ack_magic.end());
    append_little(body, ack.schema_version);
    append_little(body, ack.sequence);
    body.push_back(static_cast<std::byte>(ack.action));
    return body;
}

std::optional<ConsumerAck> decode_ack(const std::span<const std::byte> body) {
    if (body.size() != 17 || !std::equal(ack_magic.begin(), ack_magic.end(), body.begin())) {
        return std::nullopt;
    }
    std::size_t position = ack_magic.size();
    const auto version = read_little<std::uint32_t>(body, position);
    const auto sequence = read_little<std::uint64_t>(body, position);
    const auto action = std::to_integer<unsigned char>(body[position]);
    if (!version || *version != schema_version || !sequence || *sequence == 0 || action > 2) {
        return std::nullopt;
    }
    return ConsumerAck{*version, *sequence, static_cast<AckAction>(action)};
}

} // namespace npc::media::input
