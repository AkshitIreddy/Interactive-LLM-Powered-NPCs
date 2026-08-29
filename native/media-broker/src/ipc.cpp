#include "npc/media_broker/ipc.hpp"

#include <algorithm>
#include <bit>
#include <cmath>
#include <cstring>
#include <limits>
#include <type_traits>

namespace npc::media::ipc {

namespace {

[[nodiscard]] bool unit_value(const double value) noexcept {
    return std::isfinite(value) && value >= 0.0 && value <= 1.0;
}

enum class Wire : std::uint8_t { varint = 0, fixed64 = 1, bytes = 2, fixed32 = 5 };

class Writer {
public:
    void varint_field(const std::uint32_t field, const std::uint64_t value) {
        varint((static_cast<std::uint64_t>(field) << 3U) | static_cast<std::uint8_t>(Wire::varint));
        varint(value);
    }

    void fixed64_field(const std::uint32_t field, const std::uint64_t value) {
        varint((static_cast<std::uint64_t>(field) << 3U) | static_cast<std::uint8_t>(Wire::fixed64));
        for (unsigned shift = 0; shift < 64; shift += 8) {
            bytes_.push_back(static_cast<std::byte>((value >> shift) & 0xffU));
        }
    }

    void double_field(const std::uint32_t field, const double value) {
        fixed64_field(field, std::bit_cast<std::uint64_t>(value));
    }

    void bytes_field(const std::uint32_t field, const std::span<const std::byte> value) {
        varint((static_cast<std::uint64_t>(field) << 3U) | static_cast<std::uint8_t>(Wire::bytes));
        varint(value.size());
        bytes_.insert(bytes_.end(), value.begin(), value.end());
    }

    void string_field(const std::uint32_t field, const std::string_view value) {
        bytes_field(field, {reinterpret_cast<const std::byte*>(value.data()), value.size()});
    }

    [[nodiscard]] std::vector<std::byte> finish() && { return std::move(bytes_); }

private:
    void varint(std::uint64_t value) {
        while (value >= 0x80U) {
            bytes_.push_back(static_cast<std::byte>((value & 0x7fU) | 0x80U));
            value >>= 7U;
        }
        bytes_.push_back(static_cast<std::byte>(value));
    }

    std::vector<std::byte> bytes_;
};

class Reader {
public:
    explicit Reader(const std::span<const std::byte> bytes) : bytes_(bytes) {}

    struct Field { std::uint32_t number{}; Wire wire{}; };

    [[nodiscard]] std::optional<Field> next() {
        if (position_ == bytes_.size()) {
            return std::nullopt;
        }
        const auto tag = varint();
        if (!tag || (*tag >> 3U) == 0 || (*tag >> 3U) > std::numeric_limits<std::uint32_t>::max()) {
            failed_ = true;
            return std::nullopt;
        }
        const auto wire = static_cast<std::uint8_t>(*tag & 0x7U);
        if (wire != static_cast<std::uint8_t>(Wire::varint) &&
            wire != static_cast<std::uint8_t>(Wire::fixed64) &&
            wire != static_cast<std::uint8_t>(Wire::bytes) &&
            wire != static_cast<std::uint8_t>(Wire::fixed32)) {
            failed_ = true;
            return std::nullopt;
        }
        return Field{static_cast<std::uint32_t>(*tag >> 3U), static_cast<Wire>(wire)};
    }

    [[nodiscard]] std::optional<std::uint64_t> read_varint(const Field field) {
        if (field.wire != Wire::varint) {
            failed_ = true;
            return std::nullopt;
        }
        return varint();
    }

    [[nodiscard]] std::optional<std::uint64_t> read_fixed64(const Field field) {
        if (field.wire != Wire::fixed64 || position_ + 8 > bytes_.size()) {
            failed_ = true;
            return std::nullopt;
        }
        std::uint64_t value{};
        for (unsigned shift = 0; shift < 64; shift += 8) {
            value |= static_cast<std::uint64_t>(std::to_integer<unsigned char>(bytes_[position_++])) << shift;
        }
        return value;
    }

    [[nodiscard]] std::optional<double> read_double(const Field field) {
        const auto value = read_fixed64(field);
        return value ? std::optional<double>{std::bit_cast<double>(*value)} : std::nullopt;
    }

    [[nodiscard]] std::optional<std::span<const std::byte>> read_bytes(const Field field) {
        if (field.wire != Wire::bytes) {
            failed_ = true;
            return std::nullopt;
        }
        const auto size = varint();
        if (!size || *size > bytes_.size() - position_) {
            failed_ = true;
            return std::nullopt;
        }
        const auto value = bytes_.subspan(position_, static_cast<std::size_t>(*size));
        position_ += static_cast<std::size_t>(*size);
        return value;
    }

    [[nodiscard]] std::optional<std::string> read_string(const Field field, const std::size_t maximum) {
        const auto value = read_bytes(field);
        if (!value || value->size() > maximum) {
            failed_ = true;
            return std::nullopt;
        }
        return std::string(reinterpret_cast<const char*>(value->data()), value->size());
    }

    bool skip(const Field field) {
        switch (field.wire) {
        case Wire::varint: return varint().has_value();
        case Wire::fixed64:
            if (position_ + 8 > bytes_.size()) { failed_ = true; return false; }
            position_ += 8;
            return true;
        case Wire::bytes: return read_bytes(field).has_value();
        case Wire::fixed32:
            if (position_ + 4 > bytes_.size()) { failed_ = true; return false; }
            position_ += 4;
            return true;
        }
        failed_ = true;
        return false;
    }

    [[nodiscard]] bool good() const noexcept { return !failed_ && position_ == bytes_.size(); }

private:
    [[nodiscard]] std::optional<std::uint64_t> varint() {
        std::uint64_t value{};
        for (unsigned index = 0; index < 10 && position_ < bytes_.size(); ++index) {
            const auto byte = std::to_integer<std::uint8_t>(bytes_[position_++]);
            if (index == 9 && byte > 1) {
                failed_ = true;
                return std::nullopt;
            }
            value |= static_cast<std::uint64_t>(byte & 0x7fU) << (index * 7U);
            if ((byte & 0x80U) == 0) {
                return value;
            }
        }
        failed_ = true;
        return std::nullopt;
    }

    std::span<const std::byte> bytes_;
    std::size_t position_{};
    bool failed_{};
};

template <typename T>
[[nodiscard]] bool read_varint_as(Reader& reader, const Reader::Field field, T& output) {
    const auto value = reader.read_varint(field);
    if (!value || *value > static_cast<std::uint64_t>(std::numeric_limits<T>::max())) {
        return false;
    }
    output = static_cast<T>(*value);
    return true;
}

[[nodiscard]] std::optional<SharedTextureDescriptor> decode_texture(const std::span<const std::byte> bytes) {
    Reader reader(bytes);
    SharedTextureDescriptor result;
    while (const auto field = reader.next()) {
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, result.source_process_handle_value)) return std::nullopt; break;
        case 2: if (!read_varint_as(reader, *field, result.adapter_luid)) return std::nullopt; break;
        case 3: if (!read_varint_as(reader, *field, result.keyed_mutex_acquire_key)) return std::nullopt; break;
        case 4: if (!read_varint_as(reader, *field, result.keyed_mutex_release_key)) return std::nullopt; break;
        case 5: if (!read_varint_as(reader, *field, result.width)) return std::nullopt; break;
        case 6: if (!read_varint_as(reader, *field, result.height)) return std::nullopt; break;
        case 7: if (!read_varint_as(reader, *field, result.dxgi_format)) return std::nullopt; break;
        default: if (!reader.skip(*field)) return std::nullopt;
        }
    }
    if (!reader.good() || result.source_process_handle_value == 0 || result.width == 0 || result.height == 0 ||
        result.width > 16384 || result.height > 16384 || result.dxgi_format == 0) {
        return std::nullopt;
    }
    return result;
}

[[nodiscard]] std::vector<std::byte> encode_texture(const SharedTextureDescriptor& value) {
    Writer writer;
    writer.varint_field(1, value.source_process_handle_value);
    writer.varint_field(2, value.adapter_luid);
    writer.varint_field(3, value.keyed_mutex_acquire_key);
    writer.varint_field(4, value.keyed_mutex_release_key);
    writer.varint_field(5, value.width);
    writer.varint_field(6, value.height);
    writer.varint_field(7, value.dxgi_format);
    return std::move(writer).finish();
}

} // namespace

EnvelopeValidator::EnvelopeValidator(ValidationConfig config) : config_(std::move(config)) {}

StatusCode EnvelopeValidator::validate(const Envelope& envelope,
                                       const std::uint64_t now_qpc,
                                       const std::uint64_t cancellation_generation) noexcept {
    if (envelope.version != protocol_version) return StatusCode::unsupported_version;
    if (envelope.nonce != config_.nonce) return StatusCode::authentication_failed;
    if (envelope.session_id != config_.session_id) return StatusCode::session_mismatch;
    if (envelope.sequence == 0 || envelope.sequence <= last_sequence_) return StatusCode::sequence_replayed;
    if (envelope.deadline_qpc < now_qpc) return StatusCode::deadline_expired;
    if (config_.maximum_future_qpc_ticks > 0 &&
        envelope.deadline_qpc - now_qpc > config_.maximum_future_qpc_ticks) return StatusCode::deadline_too_far;
    if (envelope.command != CommandKind::cancel &&
        envelope.cancellation_generation != cancellation_generation) return StatusCode::cancellation_mismatch;
    if (envelope.payload.size() > maximum_frame_bytes) return StatusCode::payload_invalid;
    last_sequence_ = envelope.sequence;
    return StatusCode::ok;
}

std::optional<std::vector<std::byte>> encode_envelope(const Envelope& envelope) {
    if (envelope.session_id.empty() || envelope.session_id.size() > 64 || envelope.payload.size() > maximum_frame_bytes) {
        return std::nullopt;
    }
    Writer writer;
    writer.varint_field(1, envelope.version);
    writer.bytes_field(2, envelope.nonce);
    writer.string_field(3, envelope.session_id);
    writer.varint_field(4, envelope.sequence);
    writer.varint_field(5, envelope.deadline_qpc);
    writer.varint_field(6, envelope.cancellation_generation);
    writer.varint_field(7, static_cast<std::uint32_t>(envelope.command));
    writer.bytes_field(8, envelope.payload);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<Envelope> decode_envelope(const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    Envelope result;
    bool nonce_seen{}, session_seen{}, sequence_seen{}, deadline_seen{}, command_seen{};
    while (const auto field = reader.next()) {
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, result.version)) return std::nullopt; break;
        case 2: {
            const auto value = reader.read_bytes(*field);
            if (!value || value->size() != result.nonce.size()) return std::nullopt;
            std::copy(value->begin(), value->end(), result.nonce.begin());
            nonce_seen = true;
            break;
        }
        case 3: {
            auto value = reader.read_string(*field, 64);
            if (!value || value->empty()) return std::nullopt;
            result.session_id = std::move(*value);
            session_seen = true;
            break;
        }
        case 4: sequence_seen = read_varint_as(reader, *field, result.sequence); if (!sequence_seen) return std::nullopt; break;
        case 5: deadline_seen = read_varint_as(reader, *field, result.deadline_qpc); if (!deadline_seen) return std::nullopt; break;
        case 6: if (!read_varint_as(reader, *field, result.cancellation_generation)) return std::nullopt; break;
        case 7: {
            std::uint32_t command{};
            if (!read_varint_as(reader, *field, command) || command < 1 || command > 10) return std::nullopt;
            result.command = static_cast<CommandKind>(command);
            command_seen = true;
            break;
        }
        case 8: {
            const auto value = reader.read_bytes(*field);
            if (!value) return std::nullopt;
            result.payload.assign(value->begin(), value->end());
            break;
        }
        default: if (!reader.skip(*field)) return std::nullopt;
        }
    }
    return reader.good() && nonce_seen && session_seen && sequence_seen && deadline_seen && command_seen
               ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_response(const Response& response) {
    if (response.payload.size() > maximum_frame_bytes) return std::nullopt;
    Writer writer;
    writer.varint_field(1, response.version);
    writer.varint_field(2, response.response_to_sequence);
    writer.varint_field(3, static_cast<std::uint32_t>(response.status));
    writer.varint_field(4, response.cancellation_generation);
    writer.bytes_field(5, response.payload);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<Response> decode_response(const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    Response result;
    bool sequence_seen{}, status_seen{};
    while (const auto field = reader.next()) {
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, result.version)) return std::nullopt; break;
        case 2: sequence_seen = read_varint_as(reader, *field, result.response_to_sequence); if (!sequence_seen) return std::nullopt; break;
        case 3: { std::uint32_t value{}; if (!read_varint_as(reader, *field, value) || value > 12) return std::nullopt; result.status = static_cast<StatusCode>(value); status_seen = true; break; }
        case 4: if (!read_varint_as(reader, *field, result.cancellation_generation)) return std::nullopt; break;
        case 5: { const auto value = reader.read_bytes(*field); if (!value) return std::nullopt; result.payload.assign(value->begin(), value->end()); break; }
        default: if (!reader.skip(*field)) return std::nullopt;
        }
    }
    return reader.good() && sequence_seen && status_seen ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_command(const CommandKind kind, const Command& command) {
    Writer writer;
    switch (kind) {
    case CommandKind::health: if (!std::holds_alternative<HealthCommand>(command)) return std::nullopt; break;
    case CommandKind::clear_target: if (!std::holds_alternative<ClearTargetCommand>(command)) return std::nullopt; break;
    case CommandKind::audio_status: if (!std::holds_alternative<AudioStatusCommand>(command)) return std::nullopt; break;
    case CommandKind::diagnostics: if (!std::holds_alternative<DiagnosticsCommand>(command)) return std::nullopt; break;
    case CommandKind::shutdown: if (!std::holds_alternative<ShutdownCommand>(command)) return std::nullopt; break;
    case CommandKind::select_target: {
        const auto* value = std::get_if<SelectTargetCommand>(&command);
        if (!value || value->native_window == 0 || value->allowed_process_names.size() > 64) return std::nullopt;
        writer.varint_field(1, value->native_window);
        writer.varint_field(2, value->expected_process_id);
        for (const auto& name : value->allowed_process_names) {
            if (name.empty() || name.size() > 260) return std::nullopt;
            writer.string_field(3, name);
        }
        break;
    }
    case CommandKind::configure_ptt: {
        const auto* value = std::get_if<ConfigurePttCommand>(&command); if (!value) return std::nullopt;
        writer.varint_field(1, value->virtual_key); break;
    }
    case CommandKind::cancel: {
        const auto* value = std::get_if<CancelCommand>(&command); if (!value) return std::nullopt;
        writer.varint_field(1, value->new_generation); break;
    }
    case CommandKind::submit_occlusion: {
        const auto* value = std::get_if<SubmitOcclusionCommand>(&command); if (!value) return std::nullopt;
        writer.double_field(1, value->face_confidence); writer.double_field(2, value->landmark_confidence);
        writer.double_field(3, value->visibility_ratio); writer.varint_field(4, value->mouth_occluded ? 1 : 0);
        writer.varint_field(5, value->measured_qpc); break;
    }
    case CommandKind::submit_patch: {
        const auto* value = std::get_if<SubmitPatchCommand>(&command); if (!value) return std::nullopt;
        writer.varint_field(1, value->source_frame_sequence); writer.varint_field(2, value->cancellation_generation);
        writer.double_field(3, value->left); writer.double_field(4, value->top);
        writer.double_field(5, value->right); writer.double_field(6, value->bottom);
        writer.double_field(7, value->confidence); writer.varint_field(8, value->produced_qpc);
        if (value->shared_texture) { const auto nested = encode_texture(*value->shared_texture); writer.bytes_field(9, nested); }
        break;
    }
    }
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<Command> decode_command(const CommandKind kind, const std::span<const std::byte> bytes) {
    Reader reader(bytes);
    switch (kind) {
    case CommandKind::health: return bytes.empty() ? std::optional<Command>{HealthCommand{}} : std::nullopt;
    case CommandKind::clear_target: return bytes.empty() ? std::optional<Command>{ClearTargetCommand{}} : std::nullopt;
    case CommandKind::audio_status: return bytes.empty() ? std::optional<Command>{AudioStatusCommand{}} : std::nullopt;
    case CommandKind::diagnostics: return bytes.empty() ? std::optional<Command>{DiagnosticsCommand{}} : std::nullopt;
    case CommandKind::shutdown: return bytes.empty() ? std::optional<Command>{ShutdownCommand{}} : std::nullopt;
    case CommandKind::select_target: {
        SelectTargetCommand result;
        while (const auto field = reader.next()) {
            if (field->number == 1) { if (!read_varint_as(reader, *field, result.native_window)) return std::nullopt; }
            else if (field->number == 2) { if (!read_varint_as(reader, *field, result.expected_process_id)) return std::nullopt; }
            else if (field->number == 3) { auto name = reader.read_string(*field, 260); if (!name || result.allowed_process_names.size() >= 64) return std::nullopt; result.allowed_process_names.push_back(std::move(*name)); }
            else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && result.native_window != 0 && !result.allowed_process_names.empty()
                   ? std::optional<Command>{std::move(result)} : std::nullopt;
    }
    case CommandKind::configure_ptt: {
        ConfigurePttCommand result; while (const auto field = reader.next()) { if (field->number == 1) { if (!read_varint_as(reader, *field, result.virtual_key)) return std::nullopt; } else if (!reader.skip(*field)) return std::nullopt; }
        return reader.good() && result.virtual_key > 0 && result.virtual_key <= 0xff ? std::optional<Command>{result} : std::nullopt;
    }
    case CommandKind::cancel: {
        CancelCommand result; while (const auto field = reader.next()) { if (field->number == 1) { if (!read_varint_as(reader, *field, result.new_generation)) return std::nullopt; } else if (!reader.skip(*field)) return std::nullopt; }
        return reader.good() && result.new_generation > 0 ? std::optional<Command>{result} : std::nullopt;
    }
    case CommandKind::submit_occlusion: {
        SubmitOcclusionCommand result; while (const auto field = reader.next()) {
            if (field->number == 1) { auto v = reader.read_double(*field); if (!v) return std::nullopt; result.face_confidence = *v; }
            else if (field->number == 2) { auto v = reader.read_double(*field); if (!v) return std::nullopt; result.landmark_confidence = *v; }
            else if (field->number == 3) { auto v = reader.read_double(*field); if (!v) return std::nullopt; result.visibility_ratio = *v; }
            else if (field->number == 4) { std::uint32_t v{}; if (!read_varint_as(reader, *field, v) || v > 1) return std::nullopt; result.mouth_occluded = v != 0; }
            else if (field->number == 5) { if (!read_varint_as(reader, *field, result.measured_qpc)) return std::nullopt; }
            else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && unit_value(result.face_confidence) &&
                       unit_value(result.landmark_confidence) && unit_value(result.visibility_ratio) &&
                       result.measured_qpc > 0
                   ? std::optional<Command>{result} : std::nullopt;
    }
    case CommandKind::submit_patch: {
        SubmitPatchCommand result; while (const auto field = reader.next()) {
            if (field->number == 1) { if (!read_varint_as(reader, *field, result.source_frame_sequence)) return std::nullopt; }
            else if (field->number == 2) { if (!read_varint_as(reader, *field, result.cancellation_generation)) return std::nullopt; }
            else if (field->number >= 3 && field->number <= 7) { auto v = reader.read_double(*field); if (!v) return std::nullopt; double* slots[]{&result.left,&result.top,&result.right,&result.bottom,&result.confidence}; *slots[field->number - 3] = *v; }
            else if (field->number == 8) { if (!read_varint_as(reader, *field, result.produced_qpc)) return std::nullopt; }
            else if (field->number == 9) { auto nested = reader.read_bytes(*field); if (!nested) return std::nullopt; result.shared_texture = decode_texture(*nested); if (!result.shared_texture) return std::nullopt; }
            else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && result.source_frame_sequence > 0 && result.produced_qpc > 0 &&
                       unit_value(result.left) && unit_value(result.top) && unit_value(result.right) &&
                       unit_value(result.bottom) && result.right > result.left && result.bottom > result.top &&
                       unit_value(result.confidence)
                   ? std::optional<Command>{result} : std::nullopt;
    }
    }
    return std::nullopt;
}

std::vector<std::byte> frame_message(const std::span<const std::byte> message) {
    if (message.size() > maximum_frame_bytes) return {};
    std::vector<std::byte> result(4 + message.size());
    const auto size = static_cast<std::uint32_t>(message.size());
    for (unsigned shift = 0; shift < 32; shift += 8) result[shift / 8] = static_cast<std::byte>((size >> shift) & 0xffU);
    std::copy(message.begin(), message.end(), result.begin() + 4);
    return result;
}

std::optional<std::uint32_t> decode_frame_size(const std::span<const std::byte, 4> prefix) noexcept {
    std::uint32_t size{};
    for (unsigned shift = 0; shift < 32; shift += 8) size |= static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(prefix[shift / 8])) << shift;
    return size > 0 && size <= maximum_frame_bytes ? std::optional{size} : std::nullopt;
}

std::string_view to_string(const StatusCode status) noexcept {
    switch (status) {
    case StatusCode::ok: return "ok"; case StatusCode::invalid_frame: return "invalid_frame";
    case StatusCode::unsupported_version: return "unsupported_version"; case StatusCode::authentication_failed: return "authentication_failed";
    case StatusCode::session_mismatch: return "session_mismatch"; case StatusCode::sequence_replayed: return "sequence_replayed";
    case StatusCode::deadline_expired: return "deadline_expired"; case StatusCode::deadline_too_far: return "deadline_too_far";
    case StatusCode::cancellation_mismatch: return "cancellation_mismatch"; case StatusCode::payload_invalid: return "payload_invalid";
    case StatusCode::target_blocked: return "target_blocked"; case StatusCode::capability_unavailable: return "capability_unavailable";
    case StatusCode::internal_error: return "internal_error";
    }
    return "unknown";
}

} // namespace npc::media::ipc
