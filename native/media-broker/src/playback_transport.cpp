#include "npc/media_broker/playback_transport.hpp"

#include <algorithm>
#include <limits>
#include <type_traits>

namespace npc::media::playback {

namespace {

constexpr std::array<std::byte, 4> envelope_magic{
    std::byte{'N'}, std::byte{'P'}, std::byte{'C'}, std::byte{'P'}};
constexpr std::array<std::byte, 4> response_magic{
    std::byte{'N'}, std::byte{'P'}, std::byte{'C'}, std::byte{'R'}};
constexpr std::size_t maximum_identifier_bytes = 128;

template <typename T>
void append_little(std::vector<std::byte>& output, T value) {
    static_assert(std::is_unsigned_v<T>);
    for (unsigned shift = 0; shift < sizeof(T) * 8; shift += 8) {
        output.push_back(static_cast<std::byte>((value >> shift) & 0xffU));
    }
}

template <typename T>
std::optional<T> read_little(std::span<const std::byte> bytes, std::size_t& position) {
    static_assert(std::is_unsigned_v<T>);
    if (position > bytes.size() || bytes.size() - position < sizeof(T)) return std::nullopt;
    T value{};
    for (unsigned shift = 0; shift < sizeof(T) * 8; shift += 8) {
        value |= static_cast<T>(std::to_integer<unsigned char>(bytes[position++])) << shift;
    }
    return value;
}

bool append_bounded_string(std::vector<std::byte>& output, const std::string_view value,
                           const std::size_t maximum) {
    if (value.empty() || value.size() > maximum || value.find('\0') != std::string_view::npos ||
        value.size() > std::numeric_limits<std::uint16_t>::max()) return false;
    append_little(output, static_cast<std::uint16_t>(value.size()));
    output.insert(output.end(), reinterpret_cast<const std::byte*>(value.data()),
                  reinterpret_cast<const std::byte*>(value.data() + value.size()));
    return true;
}

bool append_string(std::vector<std::byte>& output, const std::string_view value) {
    return append_bounded_string(output, value, maximum_identifier_bytes);
}

std::optional<std::string> read_bounded_string(std::span<const std::byte> bytes,
                                               std::size_t& position,
                                               const std::size_t maximum) {
    const auto size = read_little<std::uint16_t>(bytes, position);
    if (!size || *size == 0 || *size > maximum ||
        position > bytes.size() || bytes.size() - position < *size) return std::nullopt;
    const auto* data = reinterpret_cast<const char*>(bytes.data() + position);
    std::string value(data, data + *size);
    position += *size;
    if (value.find('\0') != std::string::npos) return std::nullopt;
    return value;
}

std::optional<std::string> read_string(std::span<const std::byte> bytes, std::size_t& position) {
    return read_bounded_string(bytes, position, maximum_identifier_bytes);
}

bool valid_identifier(const std::string_view value) noexcept {
    return !value.empty() && value.size() <= maximum_identifier_bytes &&
           std::all_of(value.begin(), value.end(), [](const unsigned char character) {
               return std::isalnum(character) || character == '-' || character == '_' ||
                      character == '.' || character == ':';
           }) && value.find("..") == std::string_view::npos;
}

void append_receipt(std::vector<std::byte>& output, const PlaybackReceipt& receipt) {
    append_little(output, receipt.schema_version);
    (void)append_string(output, receipt.receipt_id);
    (void)append_string(output, receipt.stream_id);
    (void)append_string(output, receipt.session_id);
    (void)append_string(output, receipt.turn_id);
    append_little(output, receipt.generation);
    append_little(output, receipt.source_frames);
    append_little(output, receipt.device_frames);
    append_little(output, receipt.source_duration_micros);
    output.push_back(static_cast<std::byte>(receipt.source_submission_complete));
    output.push_back(static_cast<std::byte>(receipt.endpoint_drain_complete));
    output.push_back(static_cast<std::byte>(receipt.cancelled));
    output.push_back(static_cast<std::byte>(receipt.output_selection_mode));
    (void)append_bounded_string(output, receipt.output_endpoint_id, maximum_endpoint_id_bytes);
    append_little(output, receipt.output_endpoint_generation);
}

std::optional<PlaybackReceipt> read_receipt(std::span<const std::byte> bytes,
                                            std::size_t& position) {
    PlaybackReceipt receipt;
    const auto version = read_little<std::uint32_t>(bytes, position);
    auto receipt_id = read_string(bytes, position);
    auto stream_id = read_string(bytes, position);
    auto session_id = read_string(bytes, position);
    auto turn_id = read_string(bytes, position);
    const auto generation = read_little<std::uint64_t>(bytes, position);
    const auto source_frames = read_little<std::uint64_t>(bytes, position);
    const auto device_frames = read_little<std::uint64_t>(bytes, position);
    const auto duration = read_little<std::uint64_t>(bytes, position);
    if (!version || *version != schema_version || !receipt_id || !stream_id || !session_id ||
        !turn_id || !generation || !source_frames || !device_frames || !duration ||
        bytes.size() - position < 4) return std::nullopt;
    const auto source_complete = std::to_integer<unsigned char>(bytes[position++]);
    const auto drain_complete = std::to_integer<unsigned char>(bytes[position++]);
    const auto cancelled = std::to_integer<unsigned char>(bytes[position++]);
    const auto selection_mode = std::to_integer<unsigned char>(bytes[position++]);
    auto endpoint_id = read_bounded_string(bytes, position, maximum_endpoint_id_bytes);
    const auto endpoint_generation = read_little<std::uint64_t>(bytes, position);
    if (source_complete > 1 || drain_complete > 1 || cancelled > 1 ||
        (selection_mode != static_cast<unsigned char>(AudioOutputSelectionMode::system_default) &&
         selection_mode != static_cast<unsigned char>(AudioOutputSelectionMode::endpoint_id)) ||
        !endpoint_id || !endpoint_generation || *endpoint_generation == 0) return std::nullopt;
    receipt = {*version, std::move(*receipt_id), std::move(*stream_id),
               std::move(*session_id), std::move(*turn_id), *generation, *source_frames,
               *device_frames, *duration, source_complete != 0, drain_complete != 0,
               cancelled != 0, static_cast<AudioOutputSelectionMode>(selection_mode),
               std::move(*endpoint_id), *endpoint_generation};
    return receipt;
}

} // namespace

bool valid_allocation(const AllocationRequest& request) noexcept {
    if (!valid_identifier(request.session_id) || !valid_identifier(request.turn_id) ||
        request.generation == 0 || request.expected_producer_process_id == 0 ||
        request.sample_rate < minimum_sample_rate || request.sample_rate > maximum_sample_rate ||
        request.channels == 0 || request.channels > maximum_channels || request.max_frames == 0 ||
        request.max_frames > maximum_source_frames) return false;
    const auto seconds = request.max_frames / request.sample_rate;
    return seconds <= 600;
}

bool constant_time_equal(const AuthenticationToken& left,
                         const AuthenticationToken& right) noexcept {
    unsigned difference{};
    for (std::size_t index = 0; index < left.size(); ++index) {
        difference |= std::to_integer<unsigned char>(left[index] ^ right[index]);
    }
    return difference == 0;
}

void clear_authentication_token(AuthenticationToken& token) noexcept {
    // Volatile stores prevent the compiler from eliding credential erasure.
    volatile std::byte* output = token.data();
    for (std::size_t index = 0; index < token.size(); ++index) output[index] = std::byte{};
}

std::string_view to_string(const ProducerStatus status) noexcept {
    switch (status) {
    case ProducerStatus::ok: return "ok";
    case ProducerStatus::invalid_frame: return "invalid_frame";
    case ProducerStatus::authentication_failed: return "authentication_failed";
    case ProducerStatus::producer_mismatch: return "producer_mismatch";
    case ProducerStatus::identity_mismatch: return "identity_mismatch";
    case ProducerStatus::sequence_replayed: return "sequence_replayed";
    case ProducerStatus::deadline_expired: return "deadline_expired";
    case ProducerStatus::deadline_too_far: return "deadline_too_far";
    case ProducerStatus::invalid_state: return "invalid_state";
    case ProducerStatus::chunk_too_large: return "chunk_too_large";
    case ProducerStatus::frame_budget_exceeded: return "frame_budget_exceeded";
    case ProducerStatus::backpressure: return "backpressure";
    case ProducerStatus::device_unavailable: return "device_unavailable";
    case ProducerStatus::cancelled: return "cancelled";
    case ProducerStatus::drain_timeout: return "drain_timeout";
    }
    return "unknown";
}

std::optional<std::vector<std::byte>> encode_envelope(const ProducerEnvelope& envelope) {
    if (envelope.schema_version != schema_version || envelope.sequence == 0 ||
        envelope.deadline_qpc == 0 || envelope.generation == 0 ||
        !valid_identifier(envelope.stream_id) || !valid_identifier(envelope.session_id) ||
        !valid_identifier(envelope.turn_id) || envelope.payload.size() > maximum_chunk_bytes ||
        (envelope.command != ProducerCommand::chunk && !envelope.payload.empty())) return std::nullopt;
    std::vector<std::byte> body;
    body.reserve(128 + envelope.payload.size());
    body.insert(body.end(), envelope_magic.begin(), envelope_magic.end());
    append_little(body, envelope.schema_version);
    append_little(body, static_cast<std::uint16_t>(envelope.command));
    append_little(body, std::uint16_t{});
    append_little(body, envelope.sequence);
    append_little(body, envelope.deadline_qpc);
    append_little(body, envelope.generation);
    if (!append_string(body, envelope.stream_id) || !append_string(body, envelope.session_id) ||
        !append_string(body, envelope.turn_id)) return std::nullopt;
    body.insert(body.end(), envelope.token.begin(), envelope.token.end());
    append_little(body, static_cast<std::uint32_t>(envelope.payload.size()));
    body.insert(body.end(), envelope.payload.begin(), envelope.payload.end());
    if (body.size() > maximum_wire_frame_bytes) return std::nullopt;
    std::vector<std::byte> framed;
    framed.reserve(body.size() + 4);
    append_little(framed, static_cast<std::uint32_t>(body.size()));
    framed.insert(framed.end(), body.begin(), body.end());
    return framed;
}

std::optional<ProducerEnvelope> decode_envelope(const std::span<const std::byte> body) {
    if (body.size() < 4 || body.size() > maximum_wire_frame_bytes ||
        !std::equal(envelope_magic.begin(), envelope_magic.end(), body.begin())) return std::nullopt;
    std::size_t position = 4;
    const auto version = read_little<std::uint32_t>(body, position);
    const auto command_raw = read_little<std::uint16_t>(body, position);
    const auto reserved = read_little<std::uint16_t>(body, position);
    const auto sequence = read_little<std::uint64_t>(body, position);
    const auto deadline = read_little<std::uint64_t>(body, position);
    const auto generation = read_little<std::uint64_t>(body, position);
    auto stream = read_string(body, position);
    auto session = read_string(body, position);
    auto turn = read_string(body, position);
    if (!version || *version != schema_version || !command_raw || !reserved || *reserved != 0 ||
        !sequence || *sequence == 0 || !deadline || *deadline == 0 || !generation ||
        *generation == 0 || !stream || !session || !turn ||
        body.size() - position < authentication_token_bytes + sizeof(std::uint32_t)) return std::nullopt;
    ProducerCommand command;
    switch (*command_raw) {
    case 1: command = ProducerCommand::begin; break;
    case 2: command = ProducerCommand::chunk; break;
    case 3: command = ProducerCommand::finish; break;
    case 4: command = ProducerCommand::cancel; break;
    default: return std::nullopt;
    }
    AuthenticationToken token{};
    std::copy_n(body.begin() + static_cast<std::ptrdiff_t>(position), token.size(), token.begin());
    position += token.size();
    const auto payload_size = read_little<std::uint32_t>(body, position);
    if (!payload_size || *payload_size > maximum_chunk_bytes || body.size() - position != *payload_size ||
        (command != ProducerCommand::chunk && *payload_size != 0)) return std::nullopt;
    std::vector<std::byte> payload(body.begin() + static_cast<std::ptrdiff_t>(position), body.end());
    return ProducerEnvelope{*version, command, *sequence, *deadline, *generation,
                            std::move(*stream), std::move(*session), std::move(*turn),
                            token, std::move(payload)};
}

std::optional<std::vector<std::byte>> encode_response(const ProducerResponse& response) {
    if (response.schema_version != schema_version || response.response_to_sequence == 0) return std::nullopt;
    if (response.receipt &&
        (response.receipt->output_endpoint_id.empty() ||
         response.receipt->output_endpoint_id.size() > maximum_endpoint_id_bytes ||
         response.receipt->output_endpoint_id.find('\0') != std::string::npos ||
         response.receipt->output_endpoint_generation == 0 ||
         (response.receipt->output_selection_mode != AudioOutputSelectionMode::system_default &&
          response.receipt->output_selection_mode != AudioOutputSelectionMode::endpoint_id))) {
        return std::nullopt;
    }
    std::vector<std::byte> body;
    body.reserve(256);
    body.insert(body.end(), response_magic.begin(), response_magic.end());
    append_little(body, response.schema_version);
    append_little(body, static_cast<std::uint16_t>(response.status));
    append_little(body, static_cast<std::uint16_t>(response.receipt.has_value()));
    append_little(body, response.response_to_sequence);
    append_little(body, response.accepted_source_frames);
    if (response.receipt) append_receipt(body, *response.receipt);
    if (body.size() > maximum_wire_frame_bytes) return std::nullopt;
    std::vector<std::byte> framed;
    framed.reserve(body.size() + 4);
    append_little(framed, static_cast<std::uint32_t>(body.size()));
    framed.insert(framed.end(), body.begin(), body.end());
    return framed;
}

std::optional<ProducerResponse> decode_response(const std::span<const std::byte> body) {
    if (body.size() < 28 || body.size() > maximum_wire_frame_bytes ||
        !std::equal(response_magic.begin(), response_magic.end(), body.begin())) return std::nullopt;
    std::size_t position = 4;
    const auto version = read_little<std::uint32_t>(body, position);
    const auto status_raw = read_little<std::uint16_t>(body, position);
    const auto has_receipt = read_little<std::uint16_t>(body, position);
    const auto sequence = read_little<std::uint64_t>(body, position);
    const auto accepted = read_little<std::uint64_t>(body, position);
    if (!version || *version != schema_version || !status_raw || *status_raw > 14 ||
        !has_receipt || *has_receipt > 1 || !sequence || *sequence == 0 || !accepted) return std::nullopt;
    std::optional<PlaybackReceipt> receipt;
    if (*has_receipt != 0) {
        receipt = read_receipt(body, position);
        if (!receipt) return std::nullopt;
    }
    if (position != body.size()) return std::nullopt;
    return ProducerResponse{*version, *sequence, static_cast<ProducerStatus>(*status_raw),
                            *accepted, std::move(receipt)};
}

std::optional<std::uint32_t> decode_frame_size(const std::span<const std::byte, 4> prefix) noexcept {
    std::size_t position{};
    const auto size = read_little<std::uint32_t>(prefix, position);
    if (!size || *size == 0 || *size > maximum_wire_frame_bytes) return std::nullopt;
    return size;
}

PlaybackSession::PlaybackSession(PlaybackLease lease,
                                 const std::uint32_t expected_producer_process_id,
                                 const std::uint32_t ring_capacity_frames,
                                 const ValidationPolicy policy)
    : lease_(std::move(lease)),
      expected_producer_process_id_(expected_producer_process_id),
      policy_(policy),
      ring_({lease_.sample_rate, lease_.channels, 16,
             static_cast<std::uint16_t>(lease_.channels * 2), PcmSampleKind::signed_integer},
            ring_capacity_frames) {}

PlaybackSession::~PlaybackSession() {
    clear_authentication_token(lease_.one_time_token);
}

ProducerStatus PlaybackSession::authenticate(const ProducerEnvelope& envelope,
                                             const std::uint32_t actual_producer_process_id,
                                             const std::uint64_t now_qpc) noexcept {
    if (envelope.schema_version != schema_version) return ProducerStatus::invalid_frame;
    if (actual_producer_process_id == 0 ||
        actual_producer_process_id != expected_producer_process_id_) return ProducerStatus::producer_mismatch;
    if (!constant_time_equal(envelope.token, lease_.one_time_token)) return ProducerStatus::authentication_failed;
    if (envelope.stream_id != lease_.stream_id || envelope.session_id != lease_.session_id ||
        envelope.turn_id != lease_.turn_id || envelope.generation != lease_.generation) {
        return ProducerStatus::identity_mismatch;
    }
    if (envelope.sequence <= last_sequence_) return ProducerStatus::sequence_replayed;
    if (now_qpc > envelope.deadline_qpc ||
        (state_ == SessionState::allocated && now_qpc > lease_.expires_qpc) ||
        (state_ != SessionState::allocated && stream_deadline_qpc_ != 0 &&
         now_qpc > stream_deadline_qpc_)) return ProducerStatus::deadline_expired;
    if (policy_.maximum_future_deadline_ticks == 0 || envelope.deadline_qpc - now_qpc >
        policy_.maximum_future_deadline_ticks) return ProducerStatus::deadline_too_far;
    last_sequence_ = envelope.sequence;
    return ProducerStatus::ok;
}

ProducerResponse PlaybackSession::respond(const std::uint64_t sequence,
                                          const ProducerStatus status,
                                          const std::uint64_t accepted) const {
    ProducerResponse response{schema_version, sequence, status, accepted, std::nullopt};
    if (state_ == SessionState::drained || state_ == SessionState::cancelled ||
        state_ == SessionState::failed) response.receipt = receipt();
    return response;
}

ProducerResponse PlaybackSession::process(const ProducerEnvelope& envelope,
                                          const std::uint32_t actual_producer_process_id,
                                          const std::uint64_t now_qpc) {
    const auto auth = authenticate(envelope, actual_producer_process_id, now_qpc);
    if (auth != ProducerStatus::ok) return respond(envelope.sequence, auth);
    switch (envelope.command) {
    case ProducerCommand::begin:
        if (state_ != SessionState::allocated || token_consumed_) return respond(envelope.sequence, ProducerStatus::invalid_state);
        token_consumed_ = true;
        // Bound a connected producer even if it keeps sending individually fresh
        // command deadlines. Twice the declared maximum source duration permits
        // real-time streaming jitter; the fixed 15-second allowance covers drain.
        {
            const auto source_ticks = static_cast<std::uint64_t>(
                (static_cast<long double>(lease_.max_frames) * policy_.qpc_frequency) /
                static_cast<long double>(lease_.sample_rate));
            const auto budget = source_ticks > (std::numeric_limits<std::uint64_t>::max() -
                                                 policy_.qpc_frequency * 15ULL) / 2ULL
                                    ? std::numeric_limits<std::uint64_t>::max()
                                    : source_ticks * 2ULL + policy_.qpc_frequency * 15ULL;
            stream_deadline_qpc_ = now_qpc > std::numeric_limits<std::uint64_t>::max() - budget
                                       ? std::numeric_limits<std::uint64_t>::max()
                                       : now_qpc + budget;
        }
        state_ = SessionState::streaming;
        return respond(envelope.sequence, ProducerStatus::ok);
    case ProducerCommand::chunk: {
        if (state_ != SessionState::streaming || !token_consumed_) return respond(envelope.sequence, ProducerStatus::invalid_state);
        if (envelope.payload.empty() || envelope.payload.size() > lease_.max_chunk_bytes ||
            envelope.payload.size() % ring_.format().block_align != 0) return respond(envelope.sequence, ProducerStatus::chunk_too_large);
        const auto frames = static_cast<std::uint64_t>(envelope.payload.size() / ring_.format().block_align);
        if (frames > lease_.max_frames - source_frames_) return respond(envelope.sequence, ProducerStatus::frame_budget_exceeded);
        if (frames > ring_.capacity_frames() - ring_.available_frames()) return respond(envelope.sequence, ProducerStatus::backpressure);
        const auto transfer = ring_.write(envelope.payload, static_cast<std::uint32_t>(frames));
        if (transfer.transferred_frames != frames || transfer.overflowed) return respond(envelope.sequence, ProducerStatus::backpressure);
        source_frames_ += frames;
        return respond(envelope.sequence, ProducerStatus::ok, frames);
    }
    case ProducerCommand::finish:
        if (state_ != SessionState::streaming || !token_consumed_) return respond(envelope.sequence, ProducerStatus::invalid_state);
        source_submission_complete_ = true;
        state_ = SessionState::source_finished;
        return respond(envelope.sequence, ProducerStatus::ok);
    case ProducerCommand::cancel:
        if (state_ == SessionState::drained || state_ == SessionState::failed) return respond(envelope.sequence, ProducerStatus::invalid_state);
        cancel();
        return respond(envelope.sequence, ProducerStatus::cancelled);
    }
    return respond(envelope.sequence, ProducerStatus::invalid_frame);
}

void PlaybackSession::record_device_frames(const std::uint64_t frames) noexcept {
    if (state_ != SessionState::allocated && state_ != SessionState::drained) {
        device_frames_ = std::min(source_frames_, device_frames_ + frames);
    }
}

void PlaybackSession::mark_device_drained() noexcept {
    if (state_ == SessionState::source_finished && ring_.available_frames() == 0) {
        endpoint_drain_complete_ = true;
        state_ = SessionState::drained;
    }
}

void PlaybackSession::mark_device_lost() noexcept {
    ring_.clear();
    endpoint_drain_complete_ = false;
    state_ = SessionState::failed;
}

void PlaybackSession::cancel() noexcept {
    ring_.clear();
    endpoint_drain_complete_ = false;
    state_ = SessionState::cancelled;
}

PlaybackReceipt PlaybackSession::receipt() const {
    const auto duration = lease_.sample_rate == 0 ? 0 :
        static_cast<std::uint64_t>((static_cast<long double>(source_frames_) * 1'000'000.0L) /
                                   static_cast<long double>(lease_.sample_rate));
    return {schema_version,
            "receipt-" + lease_.stream_id,
            lease_.stream_id,
            lease_.session_id,
            lease_.turn_id,
            lease_.generation,
            source_frames_,
            device_frames_,
            duration,
            source_submission_complete_,
            endpoint_drain_complete_,
            state_ == SessionState::cancelled || state_ == SessionState::failed,
            lease_.output_selection_mode,
            lease_.output_endpoint_id,
            lease_.output_endpoint_generation};
}

} // namespace npc::media::playback
