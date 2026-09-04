#include "npc/media_broker/ipc.hpp"
#include "npc/media_broker/geometry.hpp"

#include <algorithm>
#include <bit>
#include <cctype>
#include <cmath>
#include <cstring>
#include <limits>
#include <type_traits>

namespace npc::media::ipc {

namespace {

[[nodiscard]] bool unit_value(const double value) noexcept {
    return std::isfinite(value) && value >= 0.0 && value <= 1.0;
}

[[nodiscard]] bool valid_identity_identifier(const std::string_view value,
                                             const std::size_t maximum) noexcept {
    return !value.empty() && value.size() <= maximum &&
           std::all_of(value.begin(), value.end(), [](const unsigned char byte) {
               return std::isalnum(byte) != 0 || byte == '-' || byte == '_' || byte == '.';
           });
}

[[nodiscard]] bool valid_lower_sha256(const std::string_view value) noexcept {
    return value.size() == 64U &&
           std::all_of(value.begin(), value.end(), [](const unsigned char byte) {
               return std::isdigit(byte) != 0 || (byte >= 'a' && byte <= 'f');
           });
}

[[nodiscard]] bool valid_manual_actor_request_id(const std::string_view value) noexcept {
    return !value.empty() && value.size() <= 128U &&
           std::all_of(value.begin(), value.end(), [](const unsigned char byte) {
               return std::isalnum(byte) != 0 || byte == '-' || byte == '_' || byte == '.';
           });
}

[[nodiscard]] bool valid_manual_actor_candidate(const ManualActorCandidate& value) noexcept {
    return value.actor_id != 0U && value.track_id != 0U && value.track_epoch != 0U &&
           normalized_rect_valid(value.normalized_bounds) &&
           value.normalized_bounds.width() >= 0.005 && value.normalized_bounds.height() >= 0.005;
}

[[nodiscard]] bool valid_manual_actor_receipt(const ManualActorPickerReceipt& value) noexcept {
    const auto status = static_cast<std::uint32_t>(value.status);
    const auto pointer = static_cast<std::uint32_t>(value.pointer_kind);
    const bool selected = value.status == ManualActorPickerStatus::selected;
    const bool click_terminal = selected ||
        value.status == ManualActorPickerStatus::click_outside_detected_roi ||
        value.status == ManualActorPickerStatus::ambiguous_detected_roi ||
        value.status == ManualActorPickerStatus::untrusted_pointer_input;
    return value.schema_version == 1U && valid_manual_actor_request_id(value.request_id) &&
           status >= static_cast<std::uint32_t>(ManualActorPickerStatus::pending) &&
           status <= static_cast<std::uint32_t>(ManualActorPickerStatus::internal_error) &&
           (value.receipt_nonce_high != 0U || value.receipt_nonce_low != 0U) &&
           !value.capture_session_id.empty() && value.capture_session_id.size() <= 64U &&
           value.capture_session_id.find('\0') == std::string::npos &&
           value.cancellation_generation != 0U && value.selected_process_id != 0U &&
           value.selected_window_handle != 0U && !value.selected_executable_name.empty() &&
           value.selected_executable_name.size() <= 260U &&
           value.selected_executable_name.find('/') == std::string::npos &&
           value.selected_executable_name.find('\\') == std::string::npos &&
           value.source_device_generation != 0U && value.source_geometry_epoch != 0U &&
           value.source_frame_sequence != 0U && value.source_frame_qpc != 0U &&
           value.candidate_count > 0U && value.candidate_count <= 64U &&
           valid_lower_sha256(value.candidate_set_sha256) && value.began_qpc != 0U &&
           value.attested_at_qpc >= value.began_qpc && value.qpc_frequency != 0U &&
           pointer <= static_cast<std::uint32_t>(ManualActorPointerKind::pen) &&
           value.frozen_wgc_frame_verified && value.overlay_capture_excluded &&
           value.overlay_nonactivating && value.pixels_withheld_from_webview &&
           value.coordinates_withheld_from_webview &&
           (selected ? (value.selected_actor_id != 0U && value.selected_track_id != 0U &&
                        value.selected_track_epoch != 0U)
                     : (value.selected_actor_id == 0U && value.selected_track_id == 0U &&
                        value.selected_track_epoch == 0U)) &&
           (!selected || (value.single_hardware_pointer_click &&
                          value.pointer_kind != ManualActorPointerKind::none)) &&
           (click_terminal == (value.clicked_qpc != 0U)) &&
           (!click_terminal || value.clicked_qpc >= value.began_qpc) &&
           (!value.single_hardware_pointer_click || selected);
}

[[nodiscard]] bool valid_identity_reference_rights(
    const IdentityReferenceSourceClass source_class,
    const std::string_view owner_user_id,
    const std::string_view original_work_license,
    const bool explicit_user_consent,
    const bool local_only) noexcept {
    const bool private_rights = source_class == IdentityReferenceSourceClass::user_private &&
                                explicit_user_consent &&
                                valid_identity_identifier(owner_user_id, 128U) &&
                                original_work_license.empty();
    const bool original_rights =
        source_class == IdentityReferenceSourceClass::original_synthetic &&
        owner_user_id.empty() && !original_work_license.empty() &&
        original_work_license.size() <= 512U &&
        original_work_license.find('\0') == std::string_view::npos;
    return local_only && (private_rights || original_rights);
}

[[nodiscard]] bool valid_identity_reference_command(
    const AllocateIdentityReferenceImportCommand& value) noexcept {
    return value.worker_process_id != 0U && value.worker_process_creation_time != 0U &&
           !value.worker_executable_name.empty() && value.worker_executable_name.size() <= 260U &&
           value.worker_executable_name.find('/') == std::string::npos &&
           value.worker_executable_name.find('\\') == std::string::npos &&
           valid_identity_identifier(value.picker_consent_token, 128U) &&
           valid_identity_identifier(value.game_profile_id, 128U) &&
           valid_identity_identifier(value.character_id, 128U) &&
           value.subject_id == value.character_id &&
           valid_identity_identifier(value.reference_id, 128U) &&
           !value.subject_display_name.empty() && value.subject_display_name.size() <= 256U &&
           value.subject_display_name.find('\0') == std::string::npos &&
           valid_identity_reference_rights(value.source_class, value.owner_user_id,
                                           value.original_work_license,
                                           value.explicit_user_consent, value.local_only) &&
           value.imported_at_unix_ms != 0U;
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

    void sint32_field(const std::uint32_t field, const std::int32_t value) {
        const auto sign = static_cast<std::uint32_t>(-(value < 0));
        const auto zigzag = (static_cast<std::uint32_t>(value) << 1U) ^ sign;
        varint_field(field, zigzag);
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

    [[nodiscard]] std::optional<std::int32_t> read_sint32(const Field field) {
        const auto encoded = read_varint(field);
        if (!encoded || *encoded > std::numeric_limits<std::uint32_t>::max()) {
            failed_ = true;
            return std::nullopt;
        }
        const auto value = static_cast<std::uint32_t>(*encoded);
        return static_cast<std::int32_t>(value >> 1U) ^
               -static_cast<std::int32_t>(value & 1U);
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
    bool session_nonce_seen{};
    bool session_seen{};
    bool lease_high_seen{};
    bool lease_low_seen{};
    bool creation_seen{};
    while (const auto field = reader.next()) {
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, result.source_process_handle_value)) return std::nullopt; break;
        case 2: if (!read_varint_as(reader, *field, result.adapter_luid)) return std::nullopt; break;
        case 3: if (!read_varint_as(reader, *field, result.keyed_mutex_acquire_key)) return std::nullopt; break;
        case 4: if (!read_varint_as(reader, *field, result.keyed_mutex_release_key)) return std::nullopt; break;
        case 5: if (!read_varint_as(reader, *field, result.width)) return std::nullopt; break;
        case 6: if (!read_varint_as(reader, *field, result.height)) return std::nullopt; break;
        case 7: if (!read_varint_as(reader, *field, result.dxgi_format)) return std::nullopt; break;
        case 8: if (!read_varint_as(reader, *field, result.schema_version)) return std::nullopt; break;
        case 9: {
            const auto value = reader.read_bytes(*field);
            if (!value || value->size() != result.session_nonce.size()) return std::nullopt;
            std::copy(value->begin(), value->end(), result.session_nonce.begin());
            session_nonce_seen = true;
            break;
        }
        case 10: {
            auto value = reader.read_string(*field, 64);
            if (!value || value->empty()) return std::nullopt;
            result.session_id = std::move(*value);
            session_seen = true;
            break;
        }
        case 11: {
            const auto value = reader.read_fixed64(*field);
            if (!value) return std::nullopt;
            result.lease_nonce_high = *value;
            lease_high_seen = true;
            break;
        }
        case 12: {
            const auto value = reader.read_fixed64(*field);
            if (!value) return std::nullopt;
            result.lease_nonce_low = *value;
            lease_low_seen = true;
            break;
        }
        case 13: if (!read_varint_as(reader, *field, result.worker_process_id)) return std::nullopt; break;
        case 14: {
            const auto value = reader.read_fixed64(*field);
            if (!value) return std::nullopt;
            result.worker_process_creation_time = *value;
            creation_seen = true;
            break;
        }
        case 15: {
            auto value = reader.read_string(*field, 260);
            if (!value || value->empty()) return std::nullopt;
            result.worker_executable_name = std::move(*value);
            break;
        }
        case 16: if (!read_varint_as(reader, *field, result.stride_bytes)) return std::nullopt; break;
        case 17: if (!read_varint_as(reader, *field, result.alpha_mode)) return std::nullopt; break;
        case 18: if (!read_varint_as(reader, *field, result.expires_qpc)) return std::nullopt; break;
        default: if (!reader.skip(*field)) return std::nullopt;
        }
    }
    if (!reader.good() || !session_nonce_seen || !session_seen || !lease_high_seen ||
        !lease_low_seen || !creation_seen || result.schema_version != 1U ||
        result.source_process_handle_value == 0 || result.worker_process_id == 0 ||
        result.worker_executable_name.empty() || result.width == 0 || result.height == 0 ||
        result.width > 16384 || result.height > 16384 || result.dxgi_format == 0 ||
        result.stride_bytes == 0 || result.expires_qpc == 0) {
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
    writer.varint_field(8, value.schema_version);
    writer.bytes_field(9, value.session_nonce);
    writer.string_field(10, value.session_id);
    writer.fixed64_field(11, value.lease_nonce_high);
    writer.fixed64_field(12, value.lease_nonce_low);
    writer.varint_field(13, value.worker_process_id);
    writer.fixed64_field(14, value.worker_process_creation_time);
    writer.string_field(15, value.worker_executable_name);
    writer.varint_field(16, value.stride_bytes);
    writer.varint_field(17, value.alpha_mode);
    writer.varint_field(18, value.expires_qpc);
    return std::move(writer).finish();
}

[[nodiscard]] bool valid_audio_output_endpoint(const AudioOutputEndpoint& endpoint) {
    return !endpoint.endpoint_id.empty() &&
           endpoint.endpoint_id.size() <= playback::maximum_endpoint_id_bytes &&
           endpoint.endpoint_id.find('\0') == std::string::npos &&
           !endpoint.friendly_name.empty() && endpoint.friendly_name.size() <= 512 &&
           endpoint.friendly_name.find('\0') == std::string::npos &&
           endpoint.state >= AudioOutputState::active &&
           endpoint.state <= AudioOutputState::unplugged && endpoint.generation > 0;
}

[[nodiscard]] std::vector<std::byte> encode_audio_output_endpoint(
    const AudioOutputEndpoint& endpoint) {
    Writer writer;
    writer.string_field(1, endpoint.endpoint_id);
    writer.string_field(2, endpoint.friendly_name);
    writer.varint_field(3, static_cast<std::uint32_t>(endpoint.state));
    writer.varint_field(4, endpoint.system_default ? 1U : 0U);
    writer.varint_field(5, endpoint.generation);
    return std::move(writer).finish();
}

[[nodiscard]] std::optional<AudioOutputEndpoint> decode_audio_output_endpoint(
    const std::span<const std::byte> bytes) {
    Reader reader(bytes);
    AudioOutputEndpoint endpoint;
    std::uint32_t state{}, system_default{};
    bool id_seen{}, name_seen{}, state_seen{}, default_seen{}, generation_seen{};
    while (const auto field = reader.next()) {
        if (field->number == 1) {
            auto value = reader.read_string(*field, playback::maximum_endpoint_id_bytes);
            if (!value) return std::nullopt;
            endpoint.endpoint_id = std::move(*value);
            id_seen = true;
        } else if (field->number == 2) {
            auto value = reader.read_string(*field, 512);
            if (!value) return std::nullopt;
            endpoint.friendly_name = std::move(*value);
            name_seen = true;
        } else if (field->number == 3) {
            state_seen = read_varint_as(reader, *field, state);
            if (!state_seen) return std::nullopt;
        } else if (field->number == 4) {
            default_seen = read_varint_as(reader, *field, system_default);
            if (!default_seen) return std::nullopt;
        } else if (field->number == 5) {
            generation_seen = read_varint_as(reader, *field, endpoint.generation);
            if (!generation_seen) return std::nullopt;
        } else if (!reader.skip(*field)) return std::nullopt;
    }
    if (!reader.good() || !id_seen || !name_seen || !state_seen || !default_seen ||
        !generation_seen || state < 1 || state > 4 || system_default > 1) return std::nullopt;
    endpoint.state = static_cast<AudioOutputState>(state);
    endpoint.system_default = system_default != 0;
    return valid_audio_output_endpoint(endpoint) ? std::optional{std::move(endpoint)}
                                                 : std::nullopt;
}

[[nodiscard]] bool valid_audio_input_endpoint(const AudioInputEndpoint& endpoint) {
    return !endpoint.endpoint_id.empty() &&
           endpoint.endpoint_id.size() <= playback::maximum_endpoint_id_bytes &&
           endpoint.endpoint_id.find('\0') == std::string::npos &&
           !endpoint.friendly_name.empty() && endpoint.friendly_name.size() <= 512 &&
           endpoint.friendly_name.find('\0') == std::string::npos &&
           endpoint.state >= AudioInputState::active &&
           endpoint.state <= AudioInputState::unplugged && endpoint.generation > 0;
}

[[nodiscard]] std::vector<std::byte> encode_audio_input_endpoint(
    const AudioInputEndpoint& endpoint) {
    Writer writer;
    writer.string_field(1, endpoint.endpoint_id);
    writer.string_field(2, endpoint.friendly_name);
    writer.varint_field(3, static_cast<std::uint32_t>(endpoint.state));
    writer.varint_field(4, endpoint.system_default ? 1U : 0U);
    writer.varint_field(5, endpoint.generation);
    return std::move(writer).finish();
}

[[nodiscard]] std::optional<AudioInputEndpoint> decode_audio_input_endpoint(
    const std::span<const std::byte> bytes) {
    Reader reader(bytes);
    AudioInputEndpoint endpoint;
    std::uint32_t state{}, system_default{};
    bool id_seen{}, name_seen{}, state_seen{}, default_seen{}, generation_seen{};
    while (const auto field = reader.next()) {
        if (field->number == 1) {
            auto value = reader.read_string(*field, playback::maximum_endpoint_id_bytes);
            if (!value) return std::nullopt;
            endpoint.endpoint_id = std::move(*value);
            id_seen = true;
        } else if (field->number == 2) {
            auto value = reader.read_string(*field, 512);
            if (!value) return std::nullopt;
            endpoint.friendly_name = std::move(*value);
            name_seen = true;
        } else if (field->number == 3) {
            state_seen = read_varint_as(reader, *field, state);
            if (!state_seen) return std::nullopt;
        } else if (field->number == 4) {
            default_seen = read_varint_as(reader, *field, system_default);
            if (!default_seen) return std::nullopt;
        } else if (field->number == 5) {
            generation_seen = read_varint_as(reader, *field, endpoint.generation);
            if (!generation_seen) return std::nullopt;
        } else if (!reader.skip(*field)) return std::nullopt;
    }
    if (!reader.good() || !id_seen || !name_seen || !state_seen || !default_seen ||
        !generation_seen || state < 1 || state > 4 || system_default > 1) return std::nullopt;
    endpoint.state = static_cast<AudioInputState>(state);
    endpoint.system_default = system_default != 0;
    return valid_audio_input_endpoint(endpoint) ? std::optional{std::move(endpoint)}
                                                : std::nullopt;
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
            if (!read_varint_as(reader, *field, command) || command < 1 || command > 31) return std::nullopt;
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
    case CommandKind::capture_evidence: if (!std::holds_alternative<CaptureEvidenceCommand>(command)) return std::nullopt; break;
    case CommandKind::shutdown: if (!std::holds_alternative<ShutdownCommand>(command)) return std::nullopt; break;
    case CommandKind::cancel_playback: if (!std::holds_alternative<CancelPlaybackCommand>(command)) return std::nullopt; break;
    case CommandKind::enumerate_audio_outputs:
        if (!std::holds_alternative<EnumerateAudioOutputsCommand>(command)) return std::nullopt;
        break;
    case CommandKind::selected_audio_output:
        if (!std::holds_alternative<SelectedAudioOutputCommand>(command)) return std::nullopt;
        break;
    case CommandKind::trusted_subtitle_presentation_context:
        if (!std::holds_alternative<TrustedSubtitlePresentationContextCommand>(command)) {
            return std::nullopt;
        }
        break;
    case CommandKind::enumerate_audio_inputs:
        if (!std::holds_alternative<EnumerateAudioInputsCommand>(command)) return std::nullopt;
        break;
    case CommandKind::selected_audio_input:
        if (!std::holds_alternative<SelectedAudioInputCommand>(command)) return std::nullopt;
        break;
    case CommandKind::select_audio_input: {
        const auto* value = std::get_if<SelectAudioInputCommand>(&command);
        if (!value ||
            (value->mode != playback::AudioOutputSelectionMode::system_default &&
             value->mode != playback::AudioOutputSelectionMode::endpoint_id) ||
            (value->mode == playback::AudioOutputSelectionMode::system_default &&
             !value->endpoint_id.empty()) ||
            (value->mode == playback::AudioOutputSelectionMode::endpoint_id &&
             (value->endpoint_id.empty() ||
              value->endpoint_id.size() > playback::maximum_endpoint_id_bytes ||
              value->endpoint_id.find('\0') != std::string::npos))) return std::nullopt;
        writer.varint_field(1, static_cast<std::uint32_t>(value->mode));
        if (!value->endpoint_id.empty()) writer.string_field(2, value->endpoint_id);
        break;
    }
    case CommandKind::allocate_audio_input_rehearsal: {
        const auto* value = std::get_if<AllocateAudioInputRehearsalCommand>(&command);
        if (!value || value->session_id.empty() || value->session_id.size() > 128 ||
            value->session_id.find('\0') != std::string::npos || value->turn_id.empty() ||
            value->turn_id.size() > 128 || value->turn_id.find('\0') != std::string::npos ||
            value->generation == 0 ||
            value->duration_ms < input::minimum_rehearsal_duration_ms ||
            value->duration_ms > input::maximum_rehearsal_duration_ms ||
            value->sample_rate < playback::minimum_sample_rate ||
            value->sample_rate > playback::maximum_sample_rate || value->channels == 0 ||
            value->channels > playback::maximum_channels || value->max_frames == 0 ||
            value->max_frames > static_cast<std::uint64_t>(value->sample_rate) *
                                    input::maximum_rehearsal_duration_ms / 1000U ||
            value->expected_producer_process_id == 0 ||
            (value->activation_source != input::ActivationSource::explicit_rehearsal &&
             value->activation_source != input::ActivationSource::push_to_talk)) {
            return std::nullopt;
        }
        writer.string_field(1, value->session_id);
        writer.string_field(2, value->turn_id);
        writer.varint_field(3, value->generation);
        writer.varint_field(4, value->duration_ms);
        writer.varint_field(5, value->sample_rate);
        writer.varint_field(6, value->channels);
        writer.varint_field(7, value->max_frames);
        writer.varint_field(8, value->expected_producer_process_id);
        writer.varint_field(9, static_cast<std::uint32_t>(value->activation_source));
        break;
    }
    case CommandKind::cancel_audio_input_rehearsal: {
        const auto* value = std::get_if<CancelAudioInputRehearsalCommand>(&command);
        if (!value || value->stream_id.empty() || value->stream_id.size() > 128 ||
            value->generation == 0) return std::nullopt;
        writer.string_field(1, value->stream_id);
        writer.varint_field(2, value->generation);
        break;
    }
    case CommandKind::query_visual_audio_envelope: {
        const auto* value = std::get_if<QueryVisualAudioEnvelopeCommand>(&command);
        const auto valid = [](const std::string& text) {
            return !text.empty() && text.size() <= 128 &&
                   text.find('\0') == std::string::npos;
        };
        if (!value || !valid(value->session_id) || !valid(value->turn_id) ||
            value->generation == 0 || !valid(value->stream_id) ||
            !valid(value->segment_id)) return std::nullopt;
        writer.string_field(1, value->session_id);
        writer.string_field(2, value->turn_id);
        writer.varint_field(3, value->generation);
        writer.string_field(4, value->stream_id);
        writer.string_field(5, value->segment_id);
        break;
    }
    case CommandKind::query_ptt_activation_state:
        if (!std::holds_alternative<QueryPttActivationStateCommand>(command)) {
            return std::nullopt;
        }
        break;
    case CommandKind::manual_actor_picker: {
        const auto* value = std::get_if<ManualActorPickerCommand>(&command);
        if (!value || !valid_manual_actor_request_id(value->request_id) ||
            (value->action != ManualActorPickerAction::begin &&
             value->action != ManualActorPickerAction::poll &&
             value->action != ManualActorPickerAction::cancel)) {
            return std::nullopt;
        }
        writer.varint_field(1, static_cast<std::uint32_t>(value->action));
        writer.string_field(2, value->request_id);
        if (value->action != ManualActorPickerAction::begin) {
            if (value->source_device_generation != 0U || value->source_geometry_epoch != 0U ||
                value->source_frame_sequence != 0U || value->source_frame_qpc != 0U ||
                value->timeout_ms != 0U || !value->candidates.empty()) return std::nullopt;
            break;
        }
        if (value->source_device_generation == 0U || value->source_geometry_epoch == 0U ||
            value->source_frame_sequence == 0U || value->source_frame_qpc == 0U ||
            value->timeout_ms < 500U || value->timeout_ms > 15'000U ||
            value->candidates.empty() || value->candidates.size() > 64U) return std::nullopt;
        writer.varint_field(3, value->source_device_generation);
        writer.varint_field(4, value->source_geometry_epoch);
        writer.varint_field(5, value->source_frame_sequence);
        writer.varint_field(6, value->source_frame_qpc);
        writer.varint_field(7, value->timeout_ms);
        for (std::size_t index = 0; index < value->candidates.size(); ++index) {
            const auto& candidate = value->candidates[index];
            if (!valid_manual_actor_candidate(candidate)) return std::nullopt;
            for (std::size_t prior = 0; prior < index; ++prior) {
                const auto& other = value->candidates[prior];
                if (candidate.actor_id == other.actor_id ||
                    (candidate.track_id == other.track_id &&
                     candidate.track_epoch == other.track_epoch)) return std::nullopt;
            }
            Writer nested;
            nested.varint_field(1, candidate.actor_id);
            nested.varint_field(2, candidate.track_id);
            nested.varint_field(3, candidate.track_epoch);
            nested.double_field(4, candidate.normalized_bounds.left);
            nested.double_field(5, candidate.normalized_bounds.top);
            nested.double_field(6, candidate.normalized_bounds.right);
            nested.double_field(7, candidate.normalized_bounds.bottom);
            writer.bytes_field(8, std::move(nested).finish());
        }
        break;
    }
    case CommandKind::select_audio_output: {
        const auto* value = std::get_if<SelectAudioOutputCommand>(&command);
        if (!value ||
            (value->mode != playback::AudioOutputSelectionMode::system_default &&
             value->mode != playback::AudioOutputSelectionMode::endpoint_id) ||
            (value->mode == playback::AudioOutputSelectionMode::system_default &&
             !value->endpoint_id.empty()) ||
            (value->mode == playback::AudioOutputSelectionMode::endpoint_id &&
             (value->endpoint_id.empty() ||
              value->endpoint_id.size() > playback::maximum_endpoint_id_bytes))) {
            return std::nullopt;
        }
        writer.varint_field(1, static_cast<std::uint32_t>(value->mode));
        if (!value->endpoint_id.empty()) writer.string_field(2, value->endpoint_id);
        break;
    }
    case CommandKind::allocate_playback_stream: {
        const auto* value = std::get_if<AllocatePlaybackStreamCommand>(&command);
        if (!value) return std::nullopt;
        const playback::AllocationRequest request{value->session_id, value->turn_id,
            value->generation, value->sample_rate, value->channels, value->max_frames,
            value->expected_producer_process_id};
        if (!playback::valid_allocation(request)) return std::nullopt;
        writer.string_field(1, value->session_id);
        writer.string_field(2, value->turn_id);
        writer.varint_field(3, value->generation);
        writer.varint_field(4, value->sample_rate);
        writer.varint_field(5, value->channels);
        writer.varint_field(6, value->max_frames);
        writer.varint_field(7, value->expected_producer_process_id);
        break;
    }
    case CommandKind::allocate_visual_source: {
        const auto* value = std::get_if<AllocateVisualSourceCommand>(&command);
        if (!value || value->worker_process_id == 0U ||
            value->worker_process_creation_time == 0U ||
            value->worker_executable_name.empty() || value->worker_executable_name.size() > 260U ||
            value->worker_executable_name.find('/') != std::string::npos ||
            value->worker_executable_name.find('\\') != std::string::npos ||
            value->actor_id == 0U || value->track_id == 0U || value->track_epoch == 0U) {
            return std::nullopt;
        }
        writer.varint_field(1, value->worker_process_id);
        writer.fixed64_field(2, value->worker_process_creation_time);
        writer.string_field(3, value->worker_executable_name);
        writer.varint_field(4, value->actor_id);
        writer.varint_field(5, value->track_id);
        writer.varint_field(6, value->track_epoch);
        break;
    }
    case CommandKind::release_visual_source: {
        const auto* value = std::get_if<ReleaseVisualSourceCommand>(&command);
        if (!value || value->worker_process_id == 0U ||
            (value->lease_nonce_high == 0U && value->lease_nonce_low == 0U)) {
            return std::nullopt;
        }
        writer.varint_field(1, value->worker_process_id);
        writer.fixed64_field(2, value->lease_nonce_high);
        writer.fixed64_field(3, value->lease_nonce_low);
        break;
    }
    case CommandKind::allocate_identity_frame: {
        const auto* value = std::get_if<AllocateIdentityFrameCommand>(&command);
        const auto width = value ? value->crop_px.width() : 0;
        const auto height = value ? value->crop_px.height() : 0;
        if (!value || value->worker_process_id == 0U ||
            value->worker_process_creation_time == 0U ||
            value->worker_executable_name.empty() || value->worker_executable_name.size() > 260U ||
            value->worker_executable_name.find('/') != std::string::npos ||
            value->worker_executable_name.find('\\') != std::string::npos ||
            value->crop_px.left < 0 || value->crop_px.top < 0 || width <= 0 || height <= 0 ||
            width > 8192 || height > 8192 ||
            static_cast<std::uint64_t>(width) * static_cast<std::uint64_t>(height) * 4U >
                64U * 1024U * 1024U) {
            return std::nullopt;
        }
        writer.varint_field(1, value->worker_process_id);
        writer.fixed64_field(2, value->worker_process_creation_time);
        writer.string_field(3, value->worker_executable_name);
        writer.varint_field(4, static_cast<std::uint32_t>(value->crop_px.left));
        writer.varint_field(5, static_cast<std::uint32_t>(value->crop_px.top));
        writer.varint_field(6, static_cast<std::uint32_t>(value->crop_px.right));
        writer.varint_field(7, static_cast<std::uint32_t>(value->crop_px.bottom));
        break;
    }
    case CommandKind::release_identity_frame: {
        const auto* value = std::get_if<ReleaseIdentityFrameCommand>(&command);
        if (!value || value->worker_process_id == 0U || value->lease_id.empty() ||
            value->lease_id.size() > 256U || value->lease_nonce.empty() ||
            value->lease_nonce.size() > 256U) {
            return std::nullopt;
        }
        writer.varint_field(1, value->worker_process_id);
        writer.string_field(2, value->lease_id);
        writer.string_field(3, value->lease_nonce);
        break;
    }
    case CommandKind::allocate_identity_reference_import: {
        const auto* value = std::get_if<AllocateIdentityReferenceImportCommand>(&command);
        if (!value || !valid_identity_reference_command(*value)) return std::nullopt;
        writer.varint_field(1, value->worker_process_id);
        writer.fixed64_field(2, value->worker_process_creation_time);
        writer.string_field(3, value->worker_executable_name);
        writer.string_field(4, value->picker_consent_token);
        writer.string_field(5, value->game_profile_id);
        writer.string_field(6, value->character_id);
        writer.string_field(7, value->subject_id);
        writer.string_field(8, value->reference_id);
        writer.string_field(9, value->subject_display_name);
        writer.varint_field(10, static_cast<std::uint32_t>(value->source_class));
        if (!value->owner_user_id.empty()) writer.string_field(11, value->owner_user_id);
        if (!value->original_work_license.empty()) {
            writer.string_field(12, value->original_work_license);
        }
        writer.varint_field(13, value->explicit_user_consent ? 1U : 0U);
        writer.varint_field(14, value->local_only ? 1U : 0U);
        writer.varint_field(15, value->imported_at_unix_ms);
        break;
    }
    case CommandKind::release_identity_reference_import: {
        const auto* value = std::get_if<ReleaseIdentityReferenceImportCommand>(&command);
        if (!value || value->worker_process_id == 0U || value->lease_id.empty() ||
            value->lease_id.size() > 256U || value->lease_id.find('\0') != std::string::npos ||
            value->lease_nonce.empty() || value->lease_nonce.size() > 256U ||
            value->lease_nonce.find('\0') != std::string::npos) {
            return std::nullopt;
        }
        writer.varint_field(1, value->worker_process_id);
        writer.string_field(2, value->lease_id);
        writer.string_field(3, value->lease_nonce);
        break;
    }
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
        writer.varint_field(5, value->measured_qpc);
        writer.varint_field(6, value->source_frame_sequence);
        writer.varint_field(7, value->source_device_generation);
        if (value->actor_id == 0 || value->track_id == 0 || value->track_epoch == 0 ||
            value->source_geometry_epoch == 0 || value->source_frame_qpc == 0) return std::nullopt;
        writer.varint_field(9, value->track_epoch);
        writer.varint_field(10, value->actor_id);
        writer.varint_field(11, value->track_id);
        writer.varint_field(12, value->source_geometry_epoch);
        writer.varint_field(13, value->source_frame_qpc);
        break;
    }
    case CommandKind::submit_patch: {
        const auto* value = std::get_if<SubmitPatchCommand>(&command); if (!value) return std::nullopt;
        writer.varint_field(1, value->source_frame_sequence); writer.varint_field(2, value->cancellation_generation);
        writer.double_field(3, value->left); writer.double_field(4, value->top);
        writer.double_field(5, value->right); writer.double_field(6, value->bottom);
        writer.double_field(7, value->confidence); writer.varint_field(8, value->produced_qpc);
        if (!value->shared_texture || value->shared_texture->schema_version != 1U ||
            value->shared_texture->session_id.empty() || value->shared_texture->session_id.size() > 64 ||
            value->shared_texture->worker_executable_name.empty() ||
            value->shared_texture->worker_executable_name.size() > 260) return std::nullopt;
        const auto nested = encode_texture(*value->shared_texture);
        writer.bytes_field(9, nested);
        writer.varint_field(10, value->source_device_generation);
        writer.varint_field(11, value->source_frame_qpc);
        if (value->actor_id == 0 || value->track_id == 0 || value->track_epoch == 0 ||
            value->source_geometry_epoch == 0) return std::nullopt;
        writer.varint_field(13, value->track_epoch);
        writer.varint_field(14, value->actor_id);
        writer.varint_field(15, value->track_id);
        writer.varint_field(16, value->source_geometry_epoch);
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
    case CommandKind::capture_evidence: return bytes.empty() ? std::optional<Command>{CaptureEvidenceCommand{}} : std::nullopt;
    case CommandKind::shutdown: return bytes.empty() ? std::optional<Command>{ShutdownCommand{}} : std::nullopt;
    case CommandKind::cancel_playback: return bytes.empty() ? std::optional<Command>{CancelPlaybackCommand{}} : std::nullopt;
    case CommandKind::enumerate_audio_outputs:
        return bytes.empty() ? std::optional<Command>{EnumerateAudioOutputsCommand{}} : std::nullopt;
    case CommandKind::selected_audio_output:
        return bytes.empty() ? std::optional<Command>{SelectedAudioOutputCommand{}} : std::nullopt;
    case CommandKind::trusted_subtitle_presentation_context:
        return bytes.empty()
                   ? std::optional<Command>{TrustedSubtitlePresentationContextCommand{}}
                   : std::nullopt;
    case CommandKind::enumerate_audio_inputs:
        return bytes.empty() ? std::optional<Command>{EnumerateAudioInputsCommand{}}
                             : std::nullopt;
    case CommandKind::selected_audio_input:
        return bytes.empty() ? std::optional<Command>{SelectedAudioInputCommand{}}
                             : std::nullopt;
    case CommandKind::select_audio_input: {
        SelectAudioInputCommand result;
        std::uint32_t mode{};
        bool mode_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) {
                mode_seen = read_varint_as(reader, *field, mode);
                if (!mode_seen) return std::nullopt;
            } else if (field->number == 2) {
                auto value = reader.read_string(*field, playback::maximum_endpoint_id_bytes);
                if (!value) return std::nullopt;
                result.endpoint_id = std::move(*value);
            } else if (!reader.skip(*field)) return std::nullopt;
        }
        if (!reader.good() || !mode_seen || (mode != 1 && mode != 2)) return std::nullopt;
        result.mode = static_cast<playback::AudioOutputSelectionMode>(mode);
        if ((result.mode == playback::AudioOutputSelectionMode::system_default &&
             !result.endpoint_id.empty()) ||
            (result.mode == playback::AudioOutputSelectionMode::endpoint_id &&
             result.endpoint_id.empty())) return std::nullopt;
        return Command{std::move(result)};
    }
    case CommandKind::allocate_audio_input_rehearsal: {
        AllocateAudioInputRehearsalCommand result;
        bool session_seen{}, turn_seen{}, generation_seen{}, duration_seen{}, rate_seen{},
             channels_seen{}, frames_seen{}, producer_seen{}, activation_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.session_id = std::move(*value);
                session_seen = true;
            } else if (field->number == 2) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.turn_id = std::move(*value);
                turn_seen = true;
            } else if (field->number == 3) {
                generation_seen = read_varint_as(reader, *field, result.generation);
                if (!generation_seen) return std::nullopt;
            } else if (field->number == 4) {
                duration_seen = read_varint_as(reader, *field, result.duration_ms);
                if (!duration_seen) return std::nullopt;
            } else if (field->number == 5) {
                rate_seen = read_varint_as(reader, *field, result.sample_rate);
                if (!rate_seen) return std::nullopt;
            } else if (field->number == 6) {
                channels_seen = read_varint_as(reader, *field, result.channels);
                if (!channels_seen) return std::nullopt;
            } else if (field->number == 7) {
                frames_seen = read_varint_as(reader, *field, result.max_frames);
                if (!frames_seen) return std::nullopt;
            } else if (field->number == 8) {
                producer_seen = read_varint_as(reader, *field,
                                               result.expected_producer_process_id);
                if (!producer_seen) return std::nullopt;
            } else if (field->number == 9) {
                std::uint32_t value{};
                activation_seen = read_varint_as(reader, *field, value);
                if (!activation_seen || (value != 1 && value != 2)) return std::nullopt;
                result.activation_source = static_cast<input::ActivationSource>(value);
            } else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && session_seen && turn_seen && generation_seen && duration_seen &&
                       rate_seen && channels_seen && frames_seen && producer_seen &&
                       activation_seen &&
                       !result.session_id.empty() && !result.turn_id.empty() &&
                       result.generation > 0 &&
                       result.duration_ms >= input::minimum_rehearsal_duration_ms &&
                       result.duration_ms <= input::maximum_rehearsal_duration_ms &&
                       result.sample_rate >= playback::minimum_sample_rate &&
                       result.sample_rate <= playback::maximum_sample_rate &&
                       result.channels > 0 && result.channels <= playback::maximum_channels &&
                       result.max_frames > 0 &&
                       result.max_frames <= static_cast<std::uint64_t>(result.sample_rate) *
                                                input::maximum_rehearsal_duration_ms / 1000U &&
                       result.expected_producer_process_id > 0
                   ? std::optional<Command>{std::move(result)} : std::nullopt;
    }
    case CommandKind::cancel_audio_input_rehearsal: {
        CancelAudioInputRehearsalCommand result;
        bool stream_seen{}, generation_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.stream_id = std::move(*value);
                stream_seen = true;
            } else if (field->number == 2) {
                generation_seen = read_varint_as(reader, *field, result.generation);
                if (!generation_seen) return std::nullopt;
            } else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && stream_seen && generation_seen &&
                       !result.stream_id.empty() && result.generation > 0
                   ? std::optional<Command>{std::move(result)} : std::nullopt;
    }
    case CommandKind::query_visual_audio_envelope: {
        QueryVisualAudioEnvelopeCommand result;
        bool session_seen{}, turn_seen{}, generation_seen{}, stream_seen{}, segment_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.session_id = std::move(*value);
                session_seen = true;
            } else if (field->number == 2) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.turn_id = std::move(*value);
                turn_seen = true;
            } else if (field->number == 3) {
                generation_seen = read_varint_as(reader, *field, result.generation);
                if (!generation_seen) return std::nullopt;
            } else if (field->number == 4) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.stream_id = std::move(*value);
                stream_seen = true;
            } else if (field->number == 5) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.segment_id = std::move(*value);
                segment_seen = true;
            } else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && session_seen && turn_seen && generation_seen &&
                       stream_seen && segment_seen && !result.session_id.empty() &&
                       !result.turn_id.empty() && result.generation > 0 &&
                       !result.stream_id.empty() && !result.segment_id.empty()
                   ? std::optional<Command>{std::move(result)} : std::nullopt;
    }
    case CommandKind::query_ptt_activation_state:
        return bytes.empty()
                   ? std::optional<Command>{QueryPttActivationStateCommand{}}
                   : std::nullopt;
    case CommandKind::manual_actor_picker: {
        ManualActorPickerCommand result;
        std::array<bool, 8> seen{};
        std::uint32_t action{};
        while (const auto field = reader.next()) {
            if (field->number >= 1U && field->number <= 7U) {
                if (seen[field->number]) return std::nullopt;
                seen[field->number] = true;
            }
            switch (field->number) {
            case 1:
                if (!read_varint_as(reader, *field, action) || action < 1U || action > 3U) {
                    return std::nullopt;
                }
                result.action = static_cast<ManualActorPickerAction>(action);
                break;
            case 2: {
                auto value = reader.read_string(*field, 128U);
                if (!value) return std::nullopt;
                result.request_id = std::move(*value);
                break;
            }
            case 3:
                if (!read_varint_as(reader, *field, result.source_device_generation)) {
                    return std::nullopt;
                }
                break;
            case 4:
                if (!read_varint_as(reader, *field, result.source_geometry_epoch)) {
                    return std::nullopt;
                }
                break;
            case 5:
                if (!read_varint_as(reader, *field, result.source_frame_sequence)) {
                    return std::nullopt;
                }
                break;
            case 6:
                if (!read_varint_as(reader, *field, result.source_frame_qpc)) {
                    return std::nullopt;
                }
                break;
            case 7:
                if (!read_varint_as(reader, *field, result.timeout_ms)) return std::nullopt;
                break;
            case 8: {
                if (result.candidates.size() >= 64U) return std::nullopt;
                const auto nested_bytes = reader.read_bytes(*field);
                if (!nested_bytes) return std::nullopt;
                Reader nested(*nested_bytes);
                ManualActorCandidate candidate;
                std::array<bool, 8> candidate_seen{};
                while (const auto candidate_field = nested.next()) {
                    if (candidate_field->number == 0U || candidate_field->number >= candidate_seen.size() ||
                        candidate_seen[candidate_field->number]) return std::nullopt;
                    candidate_seen[candidate_field->number] = true;
                    switch (candidate_field->number) {
                    case 1:
                        if (!read_varint_as(nested, *candidate_field, candidate.actor_id)) {
                            return std::nullopt;
                        }
                        break;
                    case 2:
                        if (!read_varint_as(nested, *candidate_field, candidate.track_id)) {
                            return std::nullopt;
                        }
                        break;
                    case 3:
                        if (!read_varint_as(nested, *candidate_field, candidate.track_epoch)) {
                            return std::nullopt;
                        }
                        break;
                    case 4: {
                        const auto value = nested.read_double(*candidate_field);
                        if (!value) return std::nullopt;
                        candidate.normalized_bounds.left = *value;
                        break;
                    }
                    case 5: {
                        const auto value = nested.read_double(*candidate_field);
                        if (!value) return std::nullopt;
                        candidate.normalized_bounds.top = *value;
                        break;
                    }
                    case 6: {
                        const auto value = nested.read_double(*candidate_field);
                        if (!value) return std::nullopt;
                        candidate.normalized_bounds.right = *value;
                        break;
                    }
                    case 7: {
                        const auto value = nested.read_double(*candidate_field);
                        if (!value) return std::nullopt;
                        candidate.normalized_bounds.bottom = *value;
                        break;
                    }
                    default: return std::nullopt;
                    }
                }
                if (!nested.good() ||
                    !std::all_of(candidate_seen.begin() + 1, candidate_seen.end(),
                                 [](const bool value) { return value; }) ||
                    !valid_manual_actor_candidate(candidate)) return std::nullopt;
                result.candidates.push_back(candidate);
                break;
            }
            default:
                if (!reader.skip(*field)) return std::nullopt;
            }
        }
        if (!reader.good() || !seen[1] || !seen[2] ||
            !valid_manual_actor_request_id(result.request_id)) return std::nullopt;
        if (result.action == ManualActorPickerAction::begin) {
            if (!seen[3] || !seen[4] || !seen[5] || !seen[6] || !seen[7] ||
                result.source_device_generation == 0U || result.source_geometry_epoch == 0U ||
                result.source_frame_sequence == 0U || result.source_frame_qpc == 0U ||
                result.timeout_ms < 500U || result.timeout_ms > 15'000U ||
                result.candidates.empty()) return std::nullopt;
            for (std::size_t index = 0; index < result.candidates.size(); ++index) {
                for (std::size_t prior = 0; prior < index; ++prior) {
                    if (result.candidates[index].actor_id == result.candidates[prior].actor_id ||
                        (result.candidates[index].track_id == result.candidates[prior].track_id &&
                         result.candidates[index].track_epoch ==
                             result.candidates[prior].track_epoch)) return std::nullopt;
                }
            }
        } else if (seen[3] || seen[4] || seen[5] || seen[6] || seen[7] ||
                   !result.candidates.empty()) {
            return std::nullopt;
        }
        return Command{std::move(result)};
    }
    case CommandKind::select_audio_output: {
        SelectAudioOutputCommand result;
        std::uint32_t mode{};
        bool mode_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) {
                mode_seen = read_varint_as(reader, *field, mode);
                if (!mode_seen) return std::nullopt;
            } else if (field->number == 2) {
                auto endpoint = reader.read_string(*field, playback::maximum_endpoint_id_bytes);
                if (!endpoint) return std::nullopt;
                result.endpoint_id = std::move(*endpoint);
            } else if (!reader.skip(*field)) return std::nullopt;
        }
        if (!reader.good() || !mode_seen || (mode != 1 && mode != 2)) return std::nullopt;
        result.mode = static_cast<playback::AudioOutputSelectionMode>(mode);
        if ((result.mode == playback::AudioOutputSelectionMode::system_default &&
             !result.endpoint_id.empty()) ||
            (result.mode == playback::AudioOutputSelectionMode::endpoint_id &&
             result.endpoint_id.empty())) return std::nullopt;
        return Command{std::move(result)};
    }
    case CommandKind::allocate_playback_stream: {
        AllocatePlaybackStreamCommand result;
        bool session_seen{}, turn_seen{}, generation_seen{}, rate_seen{}, channels_seen{},
             frames_seen{}, producer_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.session_id = std::move(*value); session_seen = true;
            } else if (field->number == 2) {
                auto value = reader.read_string(*field, 128);
                if (!value) return std::nullopt;
                result.turn_id = std::move(*value); turn_seen = true;
            } else if (field->number == 3) {
                generation_seen = read_varint_as(reader, *field, result.generation);
                if (!generation_seen) return std::nullopt;
            } else if (field->number == 4) {
                rate_seen = read_varint_as(reader, *field, result.sample_rate);
                if (!rate_seen) return std::nullopt;
            } else if (field->number == 5) {
                channels_seen = read_varint_as(reader, *field, result.channels);
                if (!channels_seen) return std::nullopt;
            } else if (field->number == 6) {
                frames_seen = read_varint_as(reader, *field, result.max_frames);
                if (!frames_seen) return std::nullopt;
            } else if (field->number == 7) {
                producer_seen = read_varint_as(reader, *field, result.expected_producer_process_id);
                if (!producer_seen) return std::nullopt;
            } else if (!reader.skip(*field)) return std::nullopt;
        }
        const playback::AllocationRequest request{result.session_id, result.turn_id,
            result.generation, result.sample_rate, result.channels, result.max_frames,
            result.expected_producer_process_id};
        return reader.good() && session_seen && turn_seen && generation_seen && rate_seen &&
                       channels_seen && frames_seen && producer_seen && playback::valid_allocation(request)
                   ? std::optional<Command>{std::move(result)} : std::nullopt;
    }
    case CommandKind::allocate_visual_source: {
        AllocateVisualSourceCommand result;
        bool worker_seen{}, creation_seen{}, executable_seen{}, actor_seen{}, track_seen{}, epoch_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) {
                worker_seen = read_varint_as(reader, *field, result.worker_process_id);
                if (!worker_seen) return std::nullopt;
            } else if (field->number == 2) {
                const auto value = reader.read_fixed64(*field);
                if (!value) return std::nullopt;
                result.worker_process_creation_time = *value;
                creation_seen = true;
            } else if (field->number == 3) {
                auto value = reader.read_string(*field, 260U);
                if (!value) return std::nullopt;
                result.worker_executable_name = std::move(*value);
                executable_seen = true;
            } else if (field->number == 4) {
                actor_seen = read_varint_as(reader, *field, result.actor_id);
                if (!actor_seen) return std::nullopt;
            } else if (field->number == 5) {
                track_seen = read_varint_as(reader, *field, result.track_id);
                if (!track_seen) return std::nullopt;
            } else if (field->number == 6) {
                epoch_seen = read_varint_as(reader, *field, result.track_epoch);
                if (!epoch_seen) return std::nullopt;
            } else if (!reader.skip(*field)) {
                return std::nullopt;
            }
        }
        return reader.good() && worker_seen && creation_seen && executable_seen && actor_seen &&
                       track_seen && epoch_seen && result.worker_process_id > 0U &&
                       result.worker_process_creation_time > 0U &&
                       !result.worker_executable_name.empty() &&
                       result.worker_executable_name.find('/') == std::string::npos &&
                       result.worker_executable_name.find('\\') == std::string::npos &&
                       result.actor_id > 0U && result.track_id > 0U && result.track_epoch > 0U
                   ? std::optional<Command>{std::move(result)} : std::nullopt;
    }
    case CommandKind::release_visual_source: {
        ReleaseVisualSourceCommand result;
        bool worker_seen{}, high_seen{}, low_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1U) {
                worker_seen = read_varint_as(reader, *field, result.worker_process_id);
                if (!worker_seen) return std::nullopt;
            } else if (field->number == 2U) {
                const auto value = reader.read_fixed64(*field);
                if (!value) return std::nullopt;
                result.lease_nonce_high = *value;
                high_seen = true;
            } else if (field->number == 3U) {
                const auto value = reader.read_fixed64(*field);
                if (!value) return std::nullopt;
                result.lease_nonce_low = *value;
                low_seen = true;
            } else if (!reader.skip(*field)) {
                return std::nullopt;
            }
        }
        return reader.good() && worker_seen && high_seen && low_seen &&
                       result.worker_process_id > 0U &&
                       (result.lease_nonce_high != 0U || result.lease_nonce_low != 0U)
                   ? std::optional<Command>{result} : std::nullopt;
    }
    case CommandKind::allocate_identity_frame: {
        AllocateIdentityFrameCommand result;
        std::uint32_t left{}, top{}, right{}, bottom{};
        bool worker_seen{}, creation_seen{}, executable_seen{}, left_seen{}, top_seen{},
             right_seen{}, bottom_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1U) {
                worker_seen = read_varint_as(reader, *field, result.worker_process_id);
                if (!worker_seen) return std::nullopt;
            } else if (field->number == 2U) {
                const auto value = reader.read_fixed64(*field);
                if (!value) return std::nullopt;
                result.worker_process_creation_time = *value;
                creation_seen = true;
            } else if (field->number == 3U) {
                auto value = reader.read_string(*field, 260U);
                if (!value) return std::nullopt;
                result.worker_executable_name = std::move(*value);
                executable_seen = true;
            } else if (field->number == 4U) {
                left_seen = read_varint_as(reader, *field, left);
                if (!left_seen) return std::nullopt;
            } else if (field->number == 5U) {
                top_seen = read_varint_as(reader, *field, top);
                if (!top_seen) return std::nullopt;
            } else if (field->number == 6U) {
                right_seen = read_varint_as(reader, *field, right);
                if (!right_seen) return std::nullopt;
            } else if (field->number == 7U) {
                bottom_seen = read_varint_as(reader, *field, bottom);
                if (!bottom_seen) return std::nullopt;
            } else if (!reader.skip(*field)) return std::nullopt;
        }
        if (right > static_cast<std::uint32_t>(std::numeric_limits<std::int32_t>::max()) ||
            bottom > static_cast<std::uint32_t>(std::numeric_limits<std::int32_t>::max())) {
            return std::nullopt;
        }
        result.crop_px = {static_cast<std::int32_t>(left), static_cast<std::int32_t>(top),
                          static_cast<std::int32_t>(right), static_cast<std::int32_t>(bottom)};
        const auto width = result.crop_px.width();
        const auto height = result.crop_px.height();
        return reader.good() && worker_seen && creation_seen && executable_seen && left_seen &&
                       top_seen && right_seen && bottom_seen && result.worker_process_id > 0U &&
                       result.worker_process_creation_time > 0U &&
                       !result.worker_executable_name.empty() &&
                       result.worker_executable_name.find('/') == std::string::npos &&
                       result.worker_executable_name.find('\\') == std::string::npos &&
                       result.crop_px.left >= 0 && result.crop_px.top >= 0 &&
                       width > 0 && height > 0 && width <= 8192 && height <= 8192 &&
                       static_cast<std::uint64_t>(width) * static_cast<std::uint64_t>(height) * 4U <=
                           64U * 1024U * 1024U
                   ? std::optional<Command>{std::move(result)} : std::nullopt;
    }
    case CommandKind::release_identity_frame: {
        ReleaseIdentityFrameCommand result;
        bool worker_seen{}, id_seen{}, nonce_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1U) {
                worker_seen = read_varint_as(reader, *field, result.worker_process_id);
                if (!worker_seen) return std::nullopt;
            } else if (field->number == 2U) {
                auto value = reader.read_string(*field, 256U);
                if (!value) return std::nullopt;
                result.lease_id = std::move(*value);
                id_seen = true;
            } else if (field->number == 3U) {
                auto value = reader.read_string(*field, 256U);
                if (!value) return std::nullopt;
                result.lease_nonce = std::move(*value);
                nonce_seen = true;
            } else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && worker_seen && id_seen && nonce_seen &&
                       result.worker_process_id > 0U && !result.lease_id.empty() &&
                       !result.lease_nonce.empty()
                   ? std::optional<Command>{std::move(result)} : std::nullopt;
    }
    case CommandKind::allocate_identity_reference_import: {
        AllocateIdentityReferenceImportCommand result;
        std::array<bool, 16> seen{};
        std::uint32_t source_class{};
        std::uint32_t explicit_user_consent{};
        std::uint32_t local_only{};
        while (const auto field = reader.next()) {
            if (field->number > 0U && field->number < seen.size()) {
                if (seen[field->number]) return std::nullopt;
                seen[field->number] = true;
            }
            switch (field->number) {
            case 1:
                if (!read_varint_as(reader, *field, result.worker_process_id)) return std::nullopt;
                break;
            case 2: {
                const auto value = reader.read_fixed64(*field);
                if (!value) return std::nullopt;
                result.worker_process_creation_time = *value;
                break;
            }
            case 3: {
                auto value = reader.read_string(*field, 260U);
                if (!value) return std::nullopt;
                result.worker_executable_name = std::move(*value);
                break;
            }
            case 4: {
                auto value = reader.read_string(*field, 128U);
                if (!value) return std::nullopt;
                result.picker_consent_token = std::move(*value);
                break;
            }
            case 5: {
                auto value = reader.read_string(*field, 128U);
                if (!value) return std::nullopt;
                result.game_profile_id = std::move(*value);
                break;
            }
            case 6: {
                auto value = reader.read_string(*field, 128U);
                if (!value) return std::nullopt;
                result.character_id = std::move(*value);
                break;
            }
            case 7: {
                auto value = reader.read_string(*field, 128U);
                if (!value) return std::nullopt;
                result.subject_id = std::move(*value);
                break;
            }
            case 8: {
                auto value = reader.read_string(*field, 128U);
                if (!value) return std::nullopt;
                result.reference_id = std::move(*value);
                break;
            }
            case 9: {
                auto value = reader.read_string(*field, 256U);
                if (!value) return std::nullopt;
                result.subject_display_name = std::move(*value);
                break;
            }
            case 10:
                if (!read_varint_as(reader, *field, source_class) || source_class < 1U ||
                    source_class > 2U) return std::nullopt;
                result.source_class = static_cast<IdentityReferenceSourceClass>(source_class);
                break;
            case 11: {
                auto value = reader.read_string(*field, 128U);
                if (!value) return std::nullopt;
                result.owner_user_id = std::move(*value);
                break;
            }
            case 12: {
                auto value = reader.read_string(*field, 512U);
                if (!value) return std::nullopt;
                result.original_work_license = std::move(*value);
                break;
            }
            case 13:
                if (!read_varint_as(reader, *field, explicit_user_consent) ||
                    explicit_user_consent > 1U) return std::nullopt;
                result.explicit_user_consent = explicit_user_consent != 0U;
                break;
            case 14:
                if (!read_varint_as(reader, *field, local_only) || local_only > 1U) {
                    return std::nullopt;
                }
                result.local_only = local_only != 0U;
                break;
            case 15:
                if (!read_varint_as(reader, *field, result.imported_at_unix_ms)) {
                    return std::nullopt;
                }
                break;
            default:
                if (!reader.skip(*field)) return std::nullopt;
            }
        }
        const bool required_seen = seen[1] && seen[2] && seen[3] && seen[4] && seen[5] &&
                                   seen[6] && seen[7] && seen[8] && seen[9] && seen[10] &&
                                   seen[14] && seen[15];
        return reader.good() && required_seen && valid_identity_reference_command(result)
                   ? std::optional<Command>{std::move(result)}
                   : std::nullopt;
    }
    case CommandKind::release_identity_reference_import: {
        ReleaseIdentityReferenceImportCommand result;
        bool worker_seen{}, id_seen{}, nonce_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1U) {
                if (worker_seen || !read_varint_as(reader, *field, result.worker_process_id)) {
                    return std::nullopt;
                }
                worker_seen = true;
            } else if (field->number == 2U) {
                if (id_seen) return std::nullopt;
                auto value = reader.read_string(*field, 256U);
                if (!value) return std::nullopt;
                result.lease_id = std::move(*value);
                id_seen = true;
            } else if (field->number == 3U) {
                if (nonce_seen) return std::nullopt;
                auto value = reader.read_string(*field, 256U);
                if (!value) return std::nullopt;
                result.lease_nonce = std::move(*value);
                nonce_seen = true;
            } else if (!reader.skip(*field)) {
                return std::nullopt;
            }
        }
        return reader.good() && worker_seen && id_seen && nonce_seen &&
                       result.worker_process_id > 0U && !result.lease_id.empty() &&
                       !result.lease_nonce.empty()
                   ? std::optional<Command>{std::move(result)}
                   : std::nullopt;
    }
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
        SubmitOcclusionCommand result;
        bool source_epoch_seen{};
        bool geometry_epoch_seen{};
        bool source_qpc_seen{};
        bool actor_seen{};
        bool track_seen{};
        bool track_epoch_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) { auto v = reader.read_double(*field); if (!v) return std::nullopt; result.face_confidence = *v; }
            else if (field->number == 2) { auto v = reader.read_double(*field); if (!v) return std::nullopt; result.landmark_confidence = *v; }
            else if (field->number == 3) { auto v = reader.read_double(*field); if (!v) return std::nullopt; result.visibility_ratio = *v; }
            else if (field->number == 4) { std::uint32_t v{}; if (!read_varint_as(reader, *field, v) || v > 1) return std::nullopt; result.mouth_occluded = v != 0; }
            else if (field->number == 5) { if (!read_varint_as(reader, *field, result.measured_qpc)) return std::nullopt; }
            else if (field->number == 6) { if (!read_varint_as(reader, *field, result.source_frame_sequence)) return std::nullopt; }
            else if (field->number == 7) { if (!read_varint_as(reader, *field, result.source_device_generation)) return std::nullopt; source_epoch_seen = true; }
            else if (field->number == 8) { return std::nullopt; } // reject legacy string IDs
            else if (field->number == 9) { if (!read_varint_as(reader, *field, result.track_epoch)) return std::nullopt; track_epoch_seen = true; }
            else if (field->number == 10) { if (!read_varint_as(reader, *field, result.actor_id)) return std::nullopt; actor_seen = true; }
            else if (field->number == 11) { if (!read_varint_as(reader, *field, result.track_id)) return std::nullopt; track_seen = true; }
            else if (field->number == 12) { if (!read_varint_as(reader, *field, result.source_geometry_epoch)) return std::nullopt; geometry_epoch_seen = true; }
            else if (field->number == 13) { if (!read_varint_as(reader, *field, result.source_frame_qpc)) return std::nullopt; source_qpc_seen = true; }
            else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && unit_value(result.face_confidence) &&
                       unit_value(result.landmark_confidence) && unit_value(result.visibility_ratio) &&
                       result.measured_qpc > 0 && result.source_frame_sequence > 0 && source_epoch_seen &&
                       geometry_epoch_seen && source_qpc_seen && actor_seen && track_seen &&
                       track_epoch_seen && result.source_geometry_epoch > 0 &&
                       result.source_frame_qpc > 0 && result.actor_id > 0 && result.track_id > 0 &&
                       result.track_epoch > 0
                   ? std::optional<Command>{result} : std::nullopt;
    }
    case CommandKind::submit_patch: {
        SubmitPatchCommand result;
        bool source_epoch_seen{};
        bool geometry_epoch_seen{};
        bool actor_seen{};
        bool track_seen{};
        bool track_epoch_seen{};
        while (const auto field = reader.next()) {
            if (field->number == 1) { if (!read_varint_as(reader, *field, result.source_frame_sequence)) return std::nullopt; }
            else if (field->number == 2) { if (!read_varint_as(reader, *field, result.cancellation_generation)) return std::nullopt; }
            else if (field->number >= 3 && field->number <= 7) { auto v = reader.read_double(*field); if (!v) return std::nullopt; double* slots[]{&result.left,&result.top,&result.right,&result.bottom,&result.confidence}; *slots[field->number - 3] = *v; }
            else if (field->number == 8) { if (!read_varint_as(reader, *field, result.produced_qpc)) return std::nullopt; }
            else if (field->number == 9) { auto nested = reader.read_bytes(*field); if (!nested) return std::nullopt; result.shared_texture = decode_texture(*nested); if (!result.shared_texture) return std::nullopt; }
            else if (field->number == 10) { if (!read_varint_as(reader, *field, result.source_device_generation)) return std::nullopt; source_epoch_seen = true; }
            else if (field->number == 11) { if (!read_varint_as(reader, *field, result.source_frame_qpc)) return std::nullopt; }
            else if (field->number == 12) { return std::nullopt; } // reject legacy string IDs
            else if (field->number == 13) { if (!read_varint_as(reader, *field, result.track_epoch)) return std::nullopt; track_epoch_seen = true; }
            else if (field->number == 14) { if (!read_varint_as(reader, *field, result.actor_id)) return std::nullopt; actor_seen = true; }
            else if (field->number == 15) { if (!read_varint_as(reader, *field, result.track_id)) return std::nullopt; track_seen = true; }
            else if (field->number == 16) { if (!read_varint_as(reader, *field, result.source_geometry_epoch)) return std::nullopt; geometry_epoch_seen = true; }
            else if (!reader.skip(*field)) return std::nullopt;
        }
        return reader.good() && source_epoch_seen && geometry_epoch_seen && actor_seen && track_seen &&
                       track_epoch_seen && result.shared_texture && result.actor_id > 0 &&
                       result.track_id > 0 && result.track_epoch > 0 &&
                       result.source_geometry_epoch > 0 && result.source_frame_sequence > 0 &&
                       result.produced_qpc > 0 &&
                       result.source_frame_qpc > 0 && result.produced_qpc >= result.source_frame_qpc &&
                       unit_value(result.left) && unit_value(result.top) && unit_value(result.right) &&
                       unit_value(result.bottom) && result.right > result.left && result.bottom > result.top &&
                       unit_value(result.confidence)
                   ? std::optional<Command>{result} : std::nullopt;
    }
    }
    return std::nullopt;
}

std::optional<std::vector<std::byte>> encode_playback_lease(
    const playback::PlaybackLease& lease) {
    if (lease.schema_version != playback::schema_version || lease.stream_id.empty() ||
        lease.stream_id.size() > 128 || lease.producer_endpoint.empty() ||
        lease.producer_endpoint.size() > 256 || lease.session_id.empty() ||
        lease.session_id.size() > 128 || lease.turn_id.empty() || lease.turn_id.size() > 128 ||
        lease.generation == 0 || lease.sample_rate < playback::minimum_sample_rate ||
        lease.sample_rate > playback::maximum_sample_rate || lease.channels == 0 ||
        lease.channels > playback::maximum_channels || lease.max_frames == 0 ||
        lease.max_chunk_bytes != playback::maximum_chunk_bytes || lease.expires_qpc == 0 ||
        (lease.output_selection_mode != playback::AudioOutputSelectionMode::system_default &&
         lease.output_selection_mode != playback::AudioOutputSelectionMode::endpoint_id) ||
        lease.output_endpoint_id.empty() ||
        lease.output_endpoint_id.size() > playback::maximum_endpoint_id_bytes ||
        lease.output_endpoint_generation == 0) {
        return std::nullopt;
    }
    Writer writer;
    writer.varint_field(1, lease.schema_version);
    writer.string_field(2, lease.stream_id);
    writer.string_field(3, lease.producer_endpoint);
    writer.bytes_field(4, lease.one_time_token);
    writer.string_field(5, lease.session_id);
    writer.string_field(6, lease.turn_id);
    writer.varint_field(7, lease.generation);
    writer.varint_field(8, lease.sample_rate);
    writer.varint_field(9, lease.channels);
    writer.varint_field(10, lease.max_frames);
    writer.varint_field(11, lease.max_chunk_bytes);
    writer.varint_field(12, lease.expires_qpc);
    writer.varint_field(13, static_cast<std::uint32_t>(lease.output_selection_mode));
    writer.string_field(14, lease.output_endpoint_id);
    writer.varint_field(15, lease.output_endpoint_generation);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<playback::PlaybackLease> decode_playback_lease(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    playback::PlaybackLease lease;
    bool schema_seen{}, stream_seen{}, endpoint_seen{}, token_seen{}, session_seen{}, turn_seen{};
    bool generation_seen{}, rate_seen{}, channels_seen{}, frames_seen{}, chunk_seen{}, expiry_seen{};
    bool output_mode_seen{}, output_id_seen{}, output_generation_seen{};
    while (const auto field = reader.next()) {
        switch (field->number) {
        case 1:
            schema_seen = read_varint_as(reader, *field, lease.schema_version);
            if (!schema_seen) return std::nullopt;
            break;
        case 2: {
            auto value = reader.read_string(*field, 128);
            if (!value) return std::nullopt;
            lease.stream_id = std::move(*value);
            stream_seen = true;
            break;
        }
        case 3: {
            auto value = reader.read_string(*field, 256);
            if (!value) return std::nullopt;
            lease.producer_endpoint = std::move(*value);
            endpoint_seen = true;
            break;
        }
        case 4: {
            const auto value = reader.read_bytes(*field);
            if (!value || value->size() != playback::authentication_token_bytes) return std::nullopt;
            std::copy(value->begin(), value->end(), lease.one_time_token.begin());
            token_seen = true;
            break;
        }
        case 5: {
            auto value = reader.read_string(*field, 128);
            if (!value) return std::nullopt;
            lease.session_id = std::move(*value);
            session_seen = true;
            break;
        }
        case 6: {
            auto value = reader.read_string(*field, 128);
            if (!value) return std::nullopt;
            lease.turn_id = std::move(*value);
            turn_seen = true;
            break;
        }
        case 7:
            generation_seen = read_varint_as(reader, *field, lease.generation);
            if (!generation_seen) return std::nullopt;
            break;
        case 8:
            rate_seen = read_varint_as(reader, *field, lease.sample_rate);
            if (!rate_seen) return std::nullopt;
            break;
        case 9:
            channels_seen = read_varint_as(reader, *field, lease.channels);
            if (!channels_seen) return std::nullopt;
            break;
        case 10:
            frames_seen = read_varint_as(reader, *field, lease.max_frames);
            if (!frames_seen) return std::nullopt;
            break;
        case 11:
            chunk_seen = read_varint_as(reader, *field, lease.max_chunk_bytes);
            if (!chunk_seen) return std::nullopt;
            break;
        case 12:
            expiry_seen = read_varint_as(reader, *field, lease.expires_qpc);
            if (!expiry_seen) return std::nullopt;
            break;
        case 13: {
            std::uint32_t mode{};
            output_mode_seen = read_varint_as(reader, *field, mode);
            if (!output_mode_seen || (mode != 1 && mode != 2)) return std::nullopt;
            lease.output_selection_mode = static_cast<playback::AudioOutputSelectionMode>(mode);
            break;
        }
        case 14: {
            auto value = reader.read_string(*field, playback::maximum_endpoint_id_bytes);
            if (!value) return std::nullopt;
            lease.output_endpoint_id = std::move(*value);
            output_id_seen = true;
            break;
        }
        case 15:
            output_generation_seen = read_varint_as(
                reader, *field, lease.output_endpoint_generation);
            if (!output_generation_seen) return std::nullopt;
            break;
        default:
            if (!reader.skip(*field)) return std::nullopt;
        }
    }
    const playback::AllocationRequest allocation{
        lease.session_id, lease.turn_id, lease.generation, lease.sample_rate, lease.channels,
        lease.max_frames, 1};
    return reader.good() && schema_seen && stream_seen && endpoint_seen && token_seen &&
                   session_seen && turn_seen && generation_seen && rate_seen && channels_seen &&
                   frames_seen && chunk_seen && expiry_seen && output_mode_seen && output_id_seen &&
                   output_generation_seen &&
                   lease.schema_version == playback::schema_version &&
                   !lease.stream_id.empty() && !lease.producer_endpoint.empty() &&
                   playback::valid_allocation(allocation) &&
                   lease.max_chunk_bytes == playback::maximum_chunk_bytes && lease.expires_qpc > 0 &&
                   lease.output_endpoint_generation > 0
               ? std::optional<playback::PlaybackLease>{std::move(lease)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_audio_output_snapshot(
    const AudioOutputSnapshot& snapshot) {
    if (snapshot.schema_version != 1 || snapshot.catalog_generation == 0 ||
        snapshot.endpoints.size() > 128) return std::nullopt;
    Writer writer;
    writer.varint_field(1, snapshot.schema_version);
    writer.varint_field(2, snapshot.catalog_generation);
    for (const auto& endpoint : snapshot.endpoints) {
        if (!valid_audio_output_endpoint(endpoint)) return std::nullopt;
        const auto nested = encode_audio_output_endpoint(endpoint);
        writer.bytes_field(3, nested);
    }
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<AudioOutputSnapshot> decode_audio_output_snapshot(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    AudioOutputSnapshot snapshot;
    bool schema_seen{}, generation_seen{};
    while (const auto field = reader.next()) {
        if (field->number == 1) {
            schema_seen = read_varint_as(reader, *field, snapshot.schema_version);
            if (!schema_seen) return std::nullopt;
        } else if (field->number == 2) {
            generation_seen = read_varint_as(reader, *field, snapshot.catalog_generation);
            if (!generation_seen) return std::nullopt;
        } else if (field->number == 3) {
            const auto nested = reader.read_bytes(*field);
            if (!nested || snapshot.endpoints.size() >= 128) return std::nullopt;
            auto endpoint = decode_audio_output_endpoint(*nested);
            if (!endpoint) return std::nullopt;
            snapshot.endpoints.push_back(std::move(*endpoint));
        } else if (!reader.skip(*field)) return std::nullopt;
    }
    return reader.good() && schema_seen && generation_seen && snapshot.schema_version == 1 &&
                   snapshot.catalog_generation > 0
               ? std::optional{std::move(snapshot)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_selected_audio_output(
    const SelectedAudioOutput& selection) {
    if (selection.schema_version != 1 ||
        (selection.mode != playback::AudioOutputSelectionMode::system_default &&
         selection.mode != playback::AudioOutputSelectionMode::endpoint_id) ||
        (selection.mode == playback::AudioOutputSelectionMode::system_default &&
         !selection.requested_endpoint_id.empty()) ||
        (selection.mode == playback::AudioOutputSelectionMode::endpoint_id &&
         selection.requested_endpoint_id != selection.resolved.endpoint_id) ||
        !valid_audio_output_endpoint(selection.resolved)) return std::nullopt;
    Writer writer;
    writer.varint_field(1, selection.schema_version);
    writer.varint_field(2, static_cast<std::uint32_t>(selection.mode));
    if (!selection.requested_endpoint_id.empty()) {
        writer.string_field(3, selection.requested_endpoint_id);
    }
    const auto endpoint = encode_audio_output_endpoint(selection.resolved);
    writer.bytes_field(4, endpoint);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<SelectedAudioOutput> decode_selected_audio_output(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    SelectedAudioOutput selection;
    std::uint32_t mode{};
    bool schema_seen{}, mode_seen{}, resolved_seen{};
    while (const auto field = reader.next()) {
        if (field->number == 1) {
            schema_seen = read_varint_as(reader, *field, selection.schema_version);
            if (!schema_seen) return std::nullopt;
        } else if (field->number == 2) {
            mode_seen = read_varint_as(reader, *field, mode);
            if (!mode_seen) return std::nullopt;
        } else if (field->number == 3) {
            auto endpoint = reader.read_string(*field, playback::maximum_endpoint_id_bytes);
            if (!endpoint) return std::nullopt;
            selection.requested_endpoint_id = std::move(*endpoint);
        } else if (field->number == 4) {
            const auto nested = reader.read_bytes(*field);
            if (!nested) return std::nullopt;
            auto endpoint = decode_audio_output_endpoint(*nested);
            if (!endpoint) return std::nullopt;
            selection.resolved = std::move(*endpoint);
            resolved_seen = true;
        } else if (!reader.skip(*field)) return std::nullopt;
    }
    if (!reader.good() || !schema_seen || !mode_seen || !resolved_seen ||
        selection.schema_version != 1 || (mode != 1 && mode != 2)) return std::nullopt;
    selection.mode = static_cast<playback::AudioOutputSelectionMode>(mode);
    if ((selection.mode == playback::AudioOutputSelectionMode::system_default &&
         !selection.requested_endpoint_id.empty()) ||
        (selection.mode == playback::AudioOutputSelectionMode::endpoint_id &&
         selection.requested_endpoint_id != selection.resolved.endpoint_id)) return std::nullopt;
    return selection;
}

std::optional<std::vector<std::byte>> encode_trusted_subtitle_presentation_context(
    const TrustedSubtitlePresentationContext& context) {
    constexpr std::string_view executable_suffix = ".exe";
    const auto executable_has_suffix = context.selected_executable_name.size() >=
                                           executable_suffix.size() &&
        std::equal(executable_suffix.rbegin(), executable_suffix.rend(),
                   context.selected_executable_name.rbegin(),
                   [](const char expected, const char actual) {
                       return expected == static_cast<char>(std::tolower(
                           static_cast<unsigned char>(actual)));
                   });
    const auto executable_is_basename = context.selected_executable_name.size() >= 5 &&
        context.selected_executable_name.size() <= 260 &&
        context.selected_executable_name.find('/') == std::string::npos &&
        context.selected_executable_name.find('\\') == std::string::npos &&
        context.selected_executable_name.find('\0') == std::string::npos &&
        executable_has_suffix;
    const auto boolean_contract_valid =
        (!context.dpi_available || (context.dpi_x > 0 && context.dpi_y > 0)) &&
        (context.dpi_available || (context.dpi_x == 0 && context.dpi_y == 0)) &&
        (!context.hdr_active || (context.hdr_evidence_available && context.hdr_supported)) &&
        (!context.color_encoding_available || context.bits_per_color_channel > 0) &&
        (context.color_encoding_available ||
         (context.color_encoding == 0 && context.bits_per_color_channel == 0)) &&
        (!context.sdr_white_level_available ||
         (std::isfinite(context.sdr_white_level_nits) &&
          context.sdr_white_level_nits > 0.0 && context.sdr_white_level_nits <= 10'000.0)) &&
        (context.sdr_white_level_available || context.sdr_white_level_nits == 0.0) &&
        ((context.capture_backend == CaptureBackend::windows_graphics_capture &&
          context.capture_scope == 1U) ||
         (context.capture_backend == CaptureBackend::desktop_duplication &&
          context.capture_scope == 2U)) &&
        ((context.target_color_space_available &&
          context.target_color_space != ColorSpace::unknown) ||
         (!context.target_color_space_available &&
          context.target_color_space == ColorSpace::unknown));
    if (context.schema_version != 1 || context.selected_process_id == 0 ||
        context.selected_window == 0 || !executable_is_basename ||
        context.capture_device_generation == 0 ||
        context.geometry_epoch == 0 || context.source_frame_sequence == 0 ||
        context.source_frame_qpc == 0 || !context.window_bounds_px.valid() ||
        !context.client_bounds_px.valid() || !context.captured_content_px.valid() ||
        context.monitor_id.empty() || context.monitor_id.size() > 128 ||
        context.monitor_id.find('\0') != std::string::npos ||
        !context.monitor_bounds_px.valid() || !context.monitor_work_area_px.valid() ||
        !boolean_contract_valid || context.attested_at_qpc == 0 || context.qpc_frequency == 0 ||
        context.attestation_id == 0 || context.attested_at_qpc < context.source_frame_qpc) {
        return std::nullopt;
    }
    Writer writer;
    writer.varint_field(1, context.schema_version);
    writer.varint_field(2, context.selected_process_id);
    writer.fixed64_field(3, context.selected_window);
    writer.varint_field(4, context.capture_device_generation);
    writer.varint_field(5, context.geometry_epoch);
    writer.varint_field(6, context.source_frame_sequence);
    writer.varint_field(7, context.source_frame_qpc);
    writer.sint32_field(8, context.window_bounds_px.left);
    writer.sint32_field(9, context.window_bounds_px.top);
    writer.sint32_field(10, context.window_bounds_px.right);
    writer.sint32_field(11, context.window_bounds_px.bottom);
    writer.sint32_field(12, context.client_bounds_px.left);
    writer.sint32_field(13, context.client_bounds_px.top);
    writer.sint32_field(14, context.client_bounds_px.right);
    writer.sint32_field(15, context.client_bounds_px.bottom);
    writer.varint_field(16, static_cast<std::uint32_t>(context.captured_content_px.width));
    writer.varint_field(17, static_cast<std::uint32_t>(context.captured_content_px.height));
    writer.string_field(18, context.monitor_id);
    writer.sint32_field(19, context.monitor_bounds_px.left);
    writer.sint32_field(20, context.monitor_bounds_px.top);
    writer.sint32_field(21, context.monitor_bounds_px.right);
    writer.sint32_field(22, context.monitor_bounds_px.bottom);
    writer.sint32_field(23, context.monitor_work_area_px.left);
    writer.sint32_field(24, context.monitor_work_area_px.top);
    writer.sint32_field(25, context.monitor_work_area_px.right);
    writer.sint32_field(26, context.monitor_work_area_px.bottom);
    writer.varint_field(27, context.dpi_available ? 1U : 0U);
    writer.varint_field(28, context.dpi_x);
    writer.varint_field(29, context.dpi_y);
    writer.varint_field(30, context.hdr_evidence_available ? 1U : 0U);
    writer.varint_field(31, context.hdr_supported ? 1U : 0U);
    writer.varint_field(32, context.hdr_user_enabled ? 1U : 0U);
    writer.varint_field(33, context.hdr_active ? 1U : 0U);
    writer.varint_field(34, context.advanced_color_active ? 1U : 0U);
    writer.varint_field(35, context.active_color_mode);
    writer.varint_field(36, context.color_encoding_available ? 1U : 0U);
    writer.varint_field(37, context.color_encoding);
    writer.varint_field(38, context.bits_per_color_channel);
    writer.varint_field(39, context.sdr_white_level_available ? 1U : 0U);
    writer.double_field(40, context.sdr_white_level_nits);
    writer.varint_field(41, static_cast<std::uint32_t>(context.capture_backend));
    writer.varint_field(42, context.capture_scope);
    writer.varint_field(43, context.overlay_capture_excluded ? 1U : 0U);
    writer.varint_field(44, context.overlay_visuals_allowed ? 1U : 0U);
    writer.string_field(45, context.selected_executable_name);
    writer.varint_field(46, context.target_color_space_available ? 1U : 0U);
    writer.varint_field(47, static_cast<std::uint32_t>(context.target_color_space));
    writer.varint_field(48, context.attested_at_qpc);
    writer.varint_field(49, context.qpc_frequency);
    writer.varint_field(50, context.attestation_id);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<TrustedSubtitlePresentationContext>
decode_trusted_subtitle_presentation_context(const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    TrustedSubtitlePresentationContext context;
    std::array<bool, 50> seen{};
    auto read_bool = [&](const Reader::Field field, bool& output) {
        std::uint32_t value{};
        if (!read_varint_as(reader, field, value) || value > 1) return false;
        output = value != 0;
        return true;
    };
    while (const auto field = reader.next()) {
        if (field->number == 0 || field->number > seen.size() || seen[field->number - 1]) {
            return std::nullopt;
        }
        seen[field->number - 1] = true;
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, context.schema_version)) return std::nullopt; break;
        case 2: if (!read_varint_as(reader, *field, context.selected_process_id)) return std::nullopt; break;
        case 3: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; context.selected_window = *value; break; }
        case 4: if (!read_varint_as(reader, *field, context.capture_device_generation)) return std::nullopt; break;
        case 5: if (!read_varint_as(reader, *field, context.geometry_epoch)) return std::nullopt; break;
        case 6: if (!read_varint_as(reader, *field, context.source_frame_sequence)) return std::nullopt; break;
        case 7: if (!read_varint_as(reader, *field, context.source_frame_qpc)) return std::nullopt; break;
        case 8: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.window_bounds_px.left = *value; break; }
        case 9: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.window_bounds_px.top = *value; break; }
        case 10: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.window_bounds_px.right = *value; break; }
        case 11: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.window_bounds_px.bottom = *value; break; }
        case 12: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.client_bounds_px.left = *value; break; }
        case 13: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.client_bounds_px.top = *value; break; }
        case 14: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.client_bounds_px.right = *value; break; }
        case 15: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.client_bounds_px.bottom = *value; break; }
        case 16: { std::uint32_t value{}; if (!read_varint_as(reader, *field, value) || value > 32'768) return std::nullopt; context.captured_content_px.width = static_cast<std::int32_t>(value); break; }
        case 17: { std::uint32_t value{}; if (!read_varint_as(reader, *field, value) || value > 32'768) return std::nullopt; context.captured_content_px.height = static_cast<std::int32_t>(value); break; }
        case 18: { auto value = reader.read_string(*field, 128); if (!value) return std::nullopt; context.monitor_id = std::move(*value); break; }
        case 19: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.monitor_bounds_px.left = *value; break; }
        case 20: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.monitor_bounds_px.top = *value; break; }
        case 21: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.monitor_bounds_px.right = *value; break; }
        case 22: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.monitor_bounds_px.bottom = *value; break; }
        case 23: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.monitor_work_area_px.left = *value; break; }
        case 24: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.monitor_work_area_px.top = *value; break; }
        case 25: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.monitor_work_area_px.right = *value; break; }
        case 26: { const auto value = reader.read_sint32(*field); if (!value) return std::nullopt; context.monitor_work_area_px.bottom = *value; break; }
        case 27: if (!read_bool(*field, context.dpi_available)) return std::nullopt; break;
        case 28: if (!read_varint_as(reader, *field, context.dpi_x)) return std::nullopt; break;
        case 29: if (!read_varint_as(reader, *field, context.dpi_y)) return std::nullopt; break;
        case 30: if (!read_bool(*field, context.hdr_evidence_available)) return std::nullopt; break;
        case 31: if (!read_bool(*field, context.hdr_supported)) return std::nullopt; break;
        case 32: if (!read_bool(*field, context.hdr_user_enabled)) return std::nullopt; break;
        case 33: if (!read_bool(*field, context.hdr_active)) return std::nullopt; break;
        case 34: if (!read_bool(*field, context.advanced_color_active)) return std::nullopt; break;
        case 35: if (!read_varint_as(reader, *field, context.active_color_mode)) return std::nullopt; break;
        case 36: if (!read_bool(*field, context.color_encoding_available)) return std::nullopt; break;
        case 37: if (!read_varint_as(reader, *field, context.color_encoding)) return std::nullopt; break;
        case 38: if (!read_varint_as(reader, *field, context.bits_per_color_channel)) return std::nullopt; break;
        case 39: if (!read_bool(*field, context.sdr_white_level_available)) return std::nullopt; break;
        case 40: { const auto value = reader.read_double(*field); if (!value) return std::nullopt; context.sdr_white_level_nits = *value; break; }
        case 41: { std::uint32_t value{}; if (!read_varint_as(reader, *field, value) || value < 1 || value > 2) return std::nullopt; context.capture_backend = static_cast<CaptureBackend>(value); break; }
        case 42: if (!read_varint_as(reader, *field, context.capture_scope)) return std::nullopt; break;
        case 43: if (!read_bool(*field, context.overlay_capture_excluded)) return std::nullopt; break;
        case 44: if (!read_bool(*field, context.overlay_visuals_allowed)) return std::nullopt; break;
        case 45: { auto value = reader.read_string(*field, 260); if (!value) return std::nullopt; context.selected_executable_name = std::move(*value); break; }
        case 46: if (!read_bool(*field, context.target_color_space_available)) return std::nullopt; break;
        case 47: { std::uint32_t value{}; if (!read_varint_as(reader, *field, value) || value > static_cast<std::uint32_t>(ColorSpace::unknown)) return std::nullopt; context.target_color_space = static_cast<ColorSpace>(value); break; }
        case 48: if (!read_varint_as(reader, *field, context.attested_at_qpc)) return std::nullopt; break;
        case 49: if (!read_varint_as(reader, *field, context.qpc_frequency)) return std::nullopt; break;
        case 50: if (!read_varint_as(reader, *field, context.attestation_id)) return std::nullopt; break;
        default: return std::nullopt;
        }
    }
    if (!reader.good() || !std::all_of(seen.begin(), seen.end(), [](const bool value) { return value; })) {
        return std::nullopt;
    }
    return encode_trusted_subtitle_presentation_context(context).has_value()
               ? std::optional{std::move(context)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_audio_input_snapshot(
    const AudioInputSnapshot& snapshot) {
    if (snapshot.schema_version != 1 || snapshot.catalog_generation == 0 ||
        snapshot.endpoints.size() > 128) return std::nullopt;
    Writer writer;
    writer.varint_field(1, snapshot.schema_version);
    writer.varint_field(2, snapshot.catalog_generation);
    for (const auto& endpoint : snapshot.endpoints) {
        if (!valid_audio_input_endpoint(endpoint)) return std::nullopt;
        writer.bytes_field(3, encode_audio_input_endpoint(endpoint));
    }
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<AudioInputSnapshot> decode_audio_input_snapshot(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    AudioInputSnapshot snapshot;
    bool schema_seen{}, generation_seen{};
    while (const auto field = reader.next()) {
        if (field->number == 1) {
            schema_seen = read_varint_as(reader, *field, snapshot.schema_version);
            if (!schema_seen) return std::nullopt;
        } else if (field->number == 2) {
            generation_seen = read_varint_as(reader, *field, snapshot.catalog_generation);
            if (!generation_seen) return std::nullopt;
        } else if (field->number == 3) {
            const auto nested = reader.read_bytes(*field);
            if (!nested || snapshot.endpoints.size() >= 128) return std::nullopt;
            auto endpoint = decode_audio_input_endpoint(*nested);
            if (!endpoint) return std::nullopt;
            snapshot.endpoints.push_back(std::move(*endpoint));
        } else if (!reader.skip(*field)) return std::nullopt;
    }
    return reader.good() && schema_seen && generation_seen && snapshot.schema_version == 1 &&
                   snapshot.catalog_generation > 0
               ? std::optional{std::move(snapshot)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_selected_audio_input(
    const SelectedAudioInput& selection) {
    if (selection.schema_version != 1 ||
        (selection.mode != playback::AudioOutputSelectionMode::system_default &&
         selection.mode != playback::AudioOutputSelectionMode::endpoint_id) ||
        (selection.mode == playback::AudioOutputSelectionMode::system_default &&
         !selection.requested_endpoint_id.empty()) ||
        (selection.mode == playback::AudioOutputSelectionMode::endpoint_id &&
         selection.requested_endpoint_id != selection.resolved.endpoint_id) ||
        !valid_audio_input_endpoint(selection.resolved)) return std::nullopt;
    Writer writer;
    writer.varint_field(1, selection.schema_version);
    writer.varint_field(2, static_cast<std::uint32_t>(selection.mode));
    if (!selection.requested_endpoint_id.empty()) {
        writer.string_field(3, selection.requested_endpoint_id);
    }
    writer.bytes_field(4, encode_audio_input_endpoint(selection.resolved));
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<SelectedAudioInput> decode_selected_audio_input(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    SelectedAudioInput selection;
    std::uint32_t mode{};
    bool schema_seen{}, mode_seen{}, resolved_seen{};
    while (const auto field = reader.next()) {
        if (field->number == 1) {
            schema_seen = read_varint_as(reader, *field, selection.schema_version);
            if (!schema_seen) return std::nullopt;
        } else if (field->number == 2) {
            mode_seen = read_varint_as(reader, *field, mode);
            if (!mode_seen) return std::nullopt;
        } else if (field->number == 3) {
            auto value = reader.read_string(*field, playback::maximum_endpoint_id_bytes);
            if (!value) return std::nullopt;
            selection.requested_endpoint_id = std::move(*value);
        } else if (field->number == 4) {
            const auto nested = reader.read_bytes(*field);
            if (!nested) return std::nullopt;
            auto endpoint = decode_audio_input_endpoint(*nested);
            if (!endpoint) return std::nullopt;
            selection.resolved = std::move(*endpoint);
            resolved_seen = true;
        } else if (!reader.skip(*field)) return std::nullopt;
    }
    if (!reader.good() || !schema_seen || !mode_seen || !resolved_seen ||
        selection.schema_version != 1 || (mode != 1 && mode != 2)) return std::nullopt;
    selection.mode = static_cast<playback::AudioOutputSelectionMode>(mode);
    if ((selection.mode == playback::AudioOutputSelectionMode::system_default &&
         !selection.requested_endpoint_id.empty()) ||
        (selection.mode == playback::AudioOutputSelectionMode::endpoint_id &&
         selection.requested_endpoint_id != selection.resolved.endpoint_id)) return std::nullopt;
    return selection;
}

std::optional<std::vector<std::byte>> encode_input_rehearsal_lease(
    const input::RehearsalLease& lease) {
    if (!input::valid_lease(lease)) return std::nullopt;
    Writer writer;
    writer.varint_field(1, lease.schema_version);
    writer.string_field(2, lease.stream_id);
    writer.string_field(3, lease.producer_endpoint);
    writer.bytes_field(4, lease.one_time_token);
    writer.string_field(5, lease.session_id);
    writer.string_field(6, lease.turn_id);
    writer.varint_field(7, lease.generation);
    writer.varint_field(8, lease.duration_ms);
    writer.varint_field(9, lease.sample_rate);
    writer.varint_field(10, lease.channels);
    writer.varint_field(11, lease.max_frames);
    writer.varint_field(12, lease.max_chunk_bytes);
    writer.varint_field(13, lease.expires_qpc);
    writer.varint_field(14, lease.qpc_frequency);
    writer.varint_field(15, static_cast<std::uint32_t>(lease.input_selection_mode));
    writer.string_field(16, lease.input_endpoint_id);
    writer.varint_field(17, lease.input_endpoint_generation);
    writer.varint_field(18, static_cast<std::uint32_t>(lease.activation_source));
    writer.varint_field(19, lease.ptt_virtual_key);
    writer.varint_field(20, lease.ptt_press_transition_sequence);
    writer.varint_field(21, lease.ptt_pressed_qpc);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<input::RehearsalLease> decode_input_rehearsal_lease(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    input::RehearsalLease lease;
    std::array<bool, 21> seen{};
    while (const auto field = reader.next()) {
        if (field->number == 0 || field->number > seen.size() || seen[field->number - 1]) {
            return std::nullopt;
        }
        seen[field->number - 1] = true;
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, lease.schema_version)) return std::nullopt; break;
        case 2: { auto value = reader.read_string(*field, 128); if (!value) return std::nullopt; lease.stream_id = std::move(*value); break; }
        case 3: { auto value = reader.read_string(*field, 256); if (!value) return std::nullopt; lease.producer_endpoint = std::move(*value); break; }
        case 4: { const auto value = reader.read_bytes(*field); if (!value || value->size() != playback::authentication_token_bytes) return std::nullopt; std::copy(value->begin(), value->end(), lease.one_time_token.begin()); break; }
        case 5: { auto value = reader.read_string(*field, 128); if (!value) return std::nullopt; lease.session_id = std::move(*value); break; }
        case 6: { auto value = reader.read_string(*field, 128); if (!value) return std::nullopt; lease.turn_id = std::move(*value); break; }
        case 7: if (!read_varint_as(reader, *field, lease.generation)) return std::nullopt; break;
        case 8: if (!read_varint_as(reader, *field, lease.duration_ms)) return std::nullopt; break;
        case 9: if (!read_varint_as(reader, *field, lease.sample_rate)) return std::nullopt; break;
        case 10: if (!read_varint_as(reader, *field, lease.channels)) return std::nullopt; break;
        case 11: if (!read_varint_as(reader, *field, lease.max_frames)) return std::nullopt; break;
        case 12: if (!read_varint_as(reader, *field, lease.max_chunk_bytes)) return std::nullopt; break;
        case 13: if (!read_varint_as(reader, *field, lease.expires_qpc)) return std::nullopt; break;
        case 14: if (!read_varint_as(reader, *field, lease.qpc_frequency)) return std::nullopt; break;
        case 15: { std::uint32_t mode{}; if (!read_varint_as(reader, *field, mode) || (mode != 1 && mode != 2)) return std::nullopt; lease.input_selection_mode = static_cast<playback::AudioOutputSelectionMode>(mode); break; }
        case 16: { auto value = reader.read_string(*field, playback::maximum_endpoint_id_bytes); if (!value) return std::nullopt; lease.input_endpoint_id = std::move(*value); break; }
        case 17: if (!read_varint_as(reader, *field, lease.input_endpoint_generation)) return std::nullopt; break;
        case 18: { std::uint32_t source{}; if (!read_varint_as(reader, *field, source) || (source != 1 && source != 2)) return std::nullopt; lease.activation_source = static_cast<input::ActivationSource>(source); break; }
        case 19: if (!read_varint_as(reader, *field, lease.ptt_virtual_key)) return std::nullopt; break;
        case 20: if (!read_varint_as(reader, *field, lease.ptt_press_transition_sequence)) return std::nullopt; break;
        case 21: if (!read_varint_as(reader, *field, lease.ptt_pressed_qpc)) return std::nullopt; break;
        default: return std::nullopt;
        }
    }
    return reader.good() && std::all_of(seen.begin(), seen.end(), [](const bool value) { return value; }) &&
                   input::valid_lease(lease)
               ? std::optional{std::move(lease)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_visual_source_lease(
    const VisualSourceLease& lease) {
    if (lease.schema_version != 1U || lease.broker_process_id == 0U ||
        lease.broker_process_creation_time == 0U || lease.broker_executable_name.empty() ||
        lease.broker_executable_name.size() > 260U || lease.worker_process_id == 0U ||
        lease.worker_process_creation_time == 0U || lease.worker_executable_name.empty() ||
        lease.worker_executable_name.size() > 260U || lease.worker_handle_value == 0U ||
        (lease.lease_nonce_high == 0U && lease.lease_nonce_low == 0U) ||
        lease.adapter_luid == 0U || lease.keyed_mutex_acquire_key == 0U ||
        lease.keyed_mutex_release_key == 0U ||
        lease.keyed_mutex_acquire_key == lease.keyed_mutex_release_key ||
        lease.width == 0U || lease.height == 0U || lease.stride_bytes < lease.width * 4U ||
        lease.dxgi_format == 0U || lease.alpha_mode != 1U || lease.expires_qpc == 0U ||
        lease.qpc_frequency == 0U || lease.cancellation_generation == 0U ||
        lease.source_geometry_epoch == 0U ||
        lease.source_frame_sequence == 0U || lease.source_frame_qpc == 0U ||
        lease.actor_id == 0U || lease.track_id == 0U || lease.track_epoch == 0U) {
        return std::nullopt;
    }
    Writer writer;
    writer.varint_field(1, lease.schema_version);
    writer.varint_field(2, lease.broker_process_id);
    writer.fixed64_field(3, lease.broker_process_creation_time);
    writer.string_field(4, lease.broker_executable_name);
    writer.varint_field(5, lease.worker_process_id);
    writer.fixed64_field(6, lease.worker_process_creation_time);
    writer.string_field(7, lease.worker_executable_name);
    writer.fixed64_field(8, lease.worker_handle_value);
    writer.fixed64_field(9, lease.lease_nonce_high);
    writer.fixed64_field(10, lease.lease_nonce_low);
    writer.fixed64_field(11, lease.adapter_luid);
    writer.fixed64_field(12, lease.keyed_mutex_acquire_key);
    writer.fixed64_field(13, lease.keyed_mutex_release_key);
    writer.varint_field(14, lease.width);
    writer.varint_field(15, lease.height);
    writer.varint_field(16, lease.stride_bytes);
    writer.varint_field(17, lease.dxgi_format);
    writer.varint_field(18, lease.alpha_mode);
    writer.varint_field(19, lease.expires_qpc);
    writer.varint_field(20, lease.qpc_frequency);
    writer.varint_field(21, lease.cancellation_generation);
    writer.varint_field(22, lease.source_device_generation);
    writer.varint_field(23, lease.source_geometry_epoch);
    writer.varint_field(24, lease.source_frame_sequence);
    writer.varint_field(25, lease.source_frame_qpc);
    writer.varint_field(26, lease.actor_id);
    writer.varint_field(27, lease.track_id);
    writer.varint_field(28, lease.track_epoch);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_identity_frame_lease(
    const IdentityFrameLease& lease) {
    const auto width = lease.crop_px.width();
    const auto height = lease.crop_px.height();
    const auto expected_name_prefix = std::string_view{"Local\\npc.identity."};
    if (lease.schema_version != 1U || lease.worker_process_id == 0U ||
        lease.worker_process_creation_time == 0U || lease.worker_executable_name.empty() ||
        lease.worker_executable_name.size() > 260U || lease.lease_id.empty() ||
        lease.lease_id.size() > 256U || lease.shared_memory_name.size() != 83U ||
        !lease.shared_memory_name.starts_with(expected_name_prefix) || lease.lease_nonce.empty() ||
        lease.lease_nonce.size() > 256U || width <= 0 || height <= 0 ||
        lease.width != static_cast<std::uint32_t>(width) ||
        lease.height != static_cast<std::uint32_t>(height) ||
        lease.stride_bytes != lease.width * 4U ||
        lease.byte_length != static_cast<std::uint64_t>(lease.stride_bytes) * lease.height ||
        lease.byte_length > 64U * 1024U * 1024U || lease.pixel_format != "b8g8r8a8_unorm" ||
        lease.content_sha256.size() != 64U || lease.expires_qpc == 0U ||
        lease.qpc_frequency == 0U || lease.cancellation_generation == 0U ||
        lease.capture_session_id.empty() || lease.capture_session_id.size() > 64U ||
        lease.selected_process_id == 0U || lease.selected_window_handle == 0U ||
        lease.selected_executable_name.empty() || lease.source_device_generation == 0U ||
        lease.source_geometry_epoch == 0U || lease.source_frame_sequence == 0U ||
        lease.source_frame_qpc == 0U || lease.captured_at_unix_ms == 0U ||
        !lease.advancing_frame_verified || !lease.overlay_capture_excluded ||
        lease.protected_online_detected || lease.anti_cheat_detected ||
        lease.source_size_px.width < lease.crop_px.right ||
        lease.source_size_px.height < lease.crop_px.bottom) {
        return std::nullopt;
    }
    Writer writer;
    writer.varint_field(1, lease.schema_version);
    writer.varint_field(2, lease.worker_process_id);
    writer.fixed64_field(3, lease.worker_process_creation_time);
    writer.string_field(4, lease.worker_executable_name);
    writer.string_field(5, lease.lease_id);
    writer.string_field(6, lease.shared_memory_name);
    writer.string_field(7, lease.lease_nonce);
    writer.varint_field(8, lease.byte_length);
    writer.varint_field(9, lease.width);
    writer.varint_field(10, lease.height);
    writer.varint_field(11, lease.stride_bytes);
    writer.string_field(12, lease.pixel_format);
    writer.string_field(13, lease.content_sha256);
    writer.varint_field(14, lease.expires_qpc);
    writer.varint_field(15, lease.qpc_frequency);
    writer.varint_field(16, lease.cancellation_generation);
    writer.string_field(17, lease.capture_session_id);
    writer.varint_field(18, lease.selected_process_id);
    writer.fixed64_field(19, lease.selected_window_handle);
    writer.string_field(20, lease.selected_executable_name);
    writer.varint_field(21, lease.source_device_generation);
    writer.varint_field(22, lease.source_geometry_epoch);
    writer.varint_field(23, lease.source_frame_sequence);
    writer.varint_field(24, lease.source_frame_qpc);
    writer.varint_field(25, lease.captured_at_unix_ms);
    writer.varint_field(26, lease.advancing_frame_verified ? 1U : 0U);
    writer.varint_field(27, lease.overlay_capture_excluded ? 1U : 0U);
    writer.varint_field(28, lease.protected_online_detected ? 1U : 0U);
    writer.varint_field(29, lease.anti_cheat_detected ? 1U : 0U);
    writer.varint_field(30, static_cast<std::uint32_t>(lease.crop_px.left));
    writer.varint_field(31, static_cast<std::uint32_t>(lease.crop_px.top));
    writer.varint_field(32, static_cast<std::uint32_t>(lease.crop_px.right));
    writer.varint_field(33, static_cast<std::uint32_t>(lease.crop_px.bottom));
    writer.varint_field(34, static_cast<std::uint32_t>(lease.source_size_px.width));
    writer.varint_field(35, static_cast<std::uint32_t>(lease.source_size_px.height));
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<IdentityFrameLease> decode_identity_frame_lease(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    IdentityFrameLease lease{};
    std::array<bool, 36> seen{};
    std::uint32_t advancing_frame_verified{};
    std::uint32_t overlay_capture_excluded{};
    std::uint32_t protected_online_detected{};
    std::uint32_t anti_cheat_detected{};
    while (const auto field = reader.next()) {
        if (field->number == 0U || field->number >= seen.size() || seen[field->number]) {
            return std::nullopt;
        }
        seen[field->number] = true;
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, lease.schema_version)) return std::nullopt; break;
        case 2: if (!read_varint_as(reader, *field, lease.worker_process_id)) return std::nullopt; break;
        case 3: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.worker_process_creation_time = *value; break; }
        case 4: { auto value = reader.read_string(*field, 260U); if (!value) return std::nullopt; lease.worker_executable_name = std::move(*value); break; }
        case 5: { auto value = reader.read_string(*field, 256U); if (!value) return std::nullopt; lease.lease_id = std::move(*value); break; }
        case 6: { auto value = reader.read_string(*field, 83U); if (!value) return std::nullopt; lease.shared_memory_name = std::move(*value); break; }
        case 7: { auto value = reader.read_string(*field, 256U); if (!value) return std::nullopt; lease.lease_nonce = std::move(*value); break; }
        case 8: if (!read_varint_as(reader, *field, lease.byte_length)) return std::nullopt; break;
        case 9: if (!read_varint_as(reader, *field, lease.width)) return std::nullopt; break;
        case 10: if (!read_varint_as(reader, *field, lease.height)) return std::nullopt; break;
        case 11: if (!read_varint_as(reader, *field, lease.stride_bytes)) return std::nullopt; break;
        case 12: { auto value = reader.read_string(*field, 32U); if (!value) return std::nullopt; lease.pixel_format = std::move(*value); break; }
        case 13: { auto value = reader.read_string(*field, 64U); if (!value) return std::nullopt; lease.content_sha256 = std::move(*value); break; }
        case 14: if (!read_varint_as(reader, *field, lease.expires_qpc)) return std::nullopt; break;
        case 15: if (!read_varint_as(reader, *field, lease.qpc_frequency)) return std::nullopt; break;
        case 16: if (!read_varint_as(reader, *field, lease.cancellation_generation)) return std::nullopt; break;
        case 17: { auto value = reader.read_string(*field, 64U); if (!value) return std::nullopt; lease.capture_session_id = std::move(*value); break; }
        case 18: if (!read_varint_as(reader, *field, lease.selected_process_id)) return std::nullopt; break;
        case 19: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.selected_window_handle = *value; break; }
        case 20: { auto value = reader.read_string(*field, 260U); if (!value) return std::nullopt; lease.selected_executable_name = std::move(*value); break; }
        case 21: if (!read_varint_as(reader, *field, lease.source_device_generation)) return std::nullopt; break;
        case 22: if (!read_varint_as(reader, *field, lease.source_geometry_epoch)) return std::nullopt; break;
        case 23: if (!read_varint_as(reader, *field, lease.source_frame_sequence)) return std::nullopt; break;
        case 24: if (!read_varint_as(reader, *field, lease.source_frame_qpc)) return std::nullopt; break;
        case 25: if (!read_varint_as(reader, *field, lease.captured_at_unix_ms)) return std::nullopt; break;
        case 26: if (!read_varint_as(reader, *field, advancing_frame_verified)) return std::nullopt; break;
        case 27: if (!read_varint_as(reader, *field, overlay_capture_excluded)) return std::nullopt; break;
        case 28: if (!read_varint_as(reader, *field, protected_online_detected)) return std::nullopt; break;
        case 29: if (!read_varint_as(reader, *field, anti_cheat_detected)) return std::nullopt; break;
        case 30: if (!read_varint_as(reader, *field, lease.crop_px.left)) return std::nullopt; break;
        case 31: if (!read_varint_as(reader, *field, lease.crop_px.top)) return std::nullopt; break;
        case 32: if (!read_varint_as(reader, *field, lease.crop_px.right)) return std::nullopt; break;
        case 33: if (!read_varint_as(reader, *field, lease.crop_px.bottom)) return std::nullopt; break;
        case 34: if (!read_varint_as(reader, *field, lease.source_size_px.width)) return std::nullopt; break;
        case 35: if (!read_varint_as(reader, *field, lease.source_size_px.height)) return std::nullopt; break;
        default: return std::nullopt;
        }
    }
    if (!reader.good() || advancing_frame_verified > 1U || overlay_capture_excluded > 1U ||
        protected_online_detected > 1U || anti_cheat_detected > 1U) {
        return std::nullopt;
    }
    for (std::size_t field = 1U; field < seen.size(); ++field) {
        if (!seen[field]) return std::nullopt;
    }
    lease.advancing_frame_verified = advancing_frame_verified != 0U;
    lease.overlay_capture_excluded = overlay_capture_excluded != 0U;
    lease.protected_online_detected = protected_online_detected != 0U;
    lease.anti_cheat_detected = anti_cheat_detected != 0U;
    return encode_identity_frame_lease(lease).has_value()
        ? std::optional<IdentityFrameLease>{std::move(lease)}
        : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_identity_reference_import_lease(
    const IdentityReferenceImportLease& lease) {
    const auto expected_name_prefix = std::string_view{"Local\\npc.identity."};
    const auto expected_bytes = static_cast<std::uint64_t>(lease.stride_bytes) * lease.height;
    const AllocateIdentityReferenceImportCommand binding{
        lease.worker_process_id,
        lease.worker_process_creation_time,
        lease.worker_executable_name,
        lease.picker_consent_token,
        lease.game_profile_id,
        lease.character_id,
        lease.subject_id,
        lease.reference_id,
        lease.subject_display_name,
        lease.source_class,
        lease.owner_user_id,
        lease.original_work_license,
        lease.explicit_user_consent,
        lease.local_only,
        lease.imported_at_unix_ms,
    };
    if (lease.schema_version != 1U || !valid_identity_reference_command(binding) ||
        lease.lease_id.empty() || lease.lease_id.size() > 256U || lease.lease_nonce.empty() ||
        lease.lease_nonce.size() > 256U || lease.shared_memory_name.size() != 83U ||
        !lease.shared_memory_name.starts_with(expected_name_prefix) || lease.width == 0U ||
        lease.height == 0U || lease.width > 8192U || lease.height > 8192U ||
        lease.stride_bytes != lease.width * 4U || lease.byte_length != expected_bytes ||
        lease.byte_length > 64U * 1024U * 1024U || lease.pixel_format != "b8g8r8a8_unorm" ||
        !valid_lower_sha256(lease.content_sha256) ||
        !valid_lower_sha256(lease.source_asset_sha256) ||
        (lease.source_media_type != "image/png" && lease.source_media_type != "image/jpeg") ||
        lease.expires_qpc == 0U || lease.qpc_frequency == 0U ||
        lease.cancellation_generation == 0U || lease.capture_session_id.empty() ||
        lease.capture_session_id.size() > 64U || lease.selected_process_id == 0U ||
        lease.selected_window_handle == 0U || lease.selected_executable_name.empty() ||
        lease.selected_executable_name.size() > 260U ||
        lease.selected_executable_name.find('/') != std::string::npos ||
        lease.selected_executable_name.find('\\') != std::string::npos ||
        lease.source_device_generation == 0U || lease.source_geometry_epoch == 0U) {
        return std::nullopt;
    }
    Writer writer;
    writer.varint_field(1, lease.schema_version);
    writer.varint_field(2, lease.worker_process_id);
    writer.fixed64_field(3, lease.worker_process_creation_time);
    writer.string_field(4, lease.worker_executable_name);
    writer.string_field(5, lease.lease_id);
    writer.string_field(6, lease.shared_memory_name);
    writer.string_field(7, lease.lease_nonce);
    writer.varint_field(8, lease.byte_length);
    writer.varint_field(9, lease.width);
    writer.varint_field(10, lease.height);
    writer.varint_field(11, lease.stride_bytes);
    writer.string_field(12, lease.pixel_format);
    writer.string_field(13, lease.content_sha256);
    writer.string_field(14, lease.source_asset_sha256);
    writer.string_field(15, lease.source_media_type);
    writer.varint_field(16, lease.expires_qpc);
    writer.varint_field(17, lease.qpc_frequency);
    writer.varint_field(18, lease.cancellation_generation);
    writer.string_field(19, lease.capture_session_id);
    writer.varint_field(20, lease.selected_process_id);
    writer.fixed64_field(21, lease.selected_window_handle);
    writer.string_field(22, lease.selected_executable_name);
    writer.varint_field(23, lease.source_device_generation);
    writer.varint_field(24, lease.source_geometry_epoch);
    writer.string_field(25, lease.picker_consent_token);
    writer.string_field(26, lease.game_profile_id);
    writer.string_field(27, lease.character_id);
    writer.string_field(28, lease.subject_id);
    writer.string_field(29, lease.reference_id);
    writer.string_field(30, lease.subject_display_name);
    writer.varint_field(31, static_cast<std::uint32_t>(lease.source_class));
    writer.string_field(32, lease.owner_user_id);
    writer.string_field(33, lease.original_work_license);
    writer.varint_field(34, lease.explicit_user_consent ? 1U : 0U);
    writer.varint_field(35, lease.local_only ? 1U : 0U);
    writer.varint_field(36, lease.imported_at_unix_ms);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<IdentityReferenceImportLease> decode_identity_reference_import_lease(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    IdentityReferenceImportLease lease{};
    std::array<bool, 37> seen{};
    std::uint32_t source_class{};
    std::uint32_t explicit_user_consent{};
    std::uint32_t local_only{};
    while (const auto field = reader.next()) {
        if (field->number == 0U) return std::nullopt;
        if (field->number >= seen.size()) {
            if (!reader.skip(*field)) return std::nullopt;
            continue;
        }
        if (seen[field->number]) return std::nullopt;
        seen[field->number] = true;
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, lease.schema_version)) return std::nullopt; break;
        case 2: if (!read_varint_as(reader, *field, lease.worker_process_id)) return std::nullopt; break;
        case 3: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.worker_process_creation_time = *value; break; }
        case 4: { auto value = reader.read_string(*field, 260U); if (!value) return std::nullopt; lease.worker_executable_name = std::move(*value); break; }
        case 5: { auto value = reader.read_string(*field, 256U); if (!value) return std::nullopt; lease.lease_id = std::move(*value); break; }
        case 6: { auto value = reader.read_string(*field, 83U); if (!value) return std::nullopt; lease.shared_memory_name = std::move(*value); break; }
        case 7: { auto value = reader.read_string(*field, 256U); if (!value) return std::nullopt; lease.lease_nonce = std::move(*value); break; }
        case 8: if (!read_varint_as(reader, *field, lease.byte_length)) return std::nullopt; break;
        case 9: if (!read_varint_as(reader, *field, lease.width)) return std::nullopt; break;
        case 10: if (!read_varint_as(reader, *field, lease.height)) return std::nullopt; break;
        case 11: if (!read_varint_as(reader, *field, lease.stride_bytes)) return std::nullopt; break;
        case 12: { auto value = reader.read_string(*field, 32U); if (!value) return std::nullopt; lease.pixel_format = std::move(*value); break; }
        case 13: { auto value = reader.read_string(*field, 64U); if (!value) return std::nullopt; lease.content_sha256 = std::move(*value); break; }
        case 14: { auto value = reader.read_string(*field, 64U); if (!value) return std::nullopt; lease.source_asset_sha256 = std::move(*value); break; }
        case 15: { auto value = reader.read_string(*field, 32U); if (!value) return std::nullopt; lease.source_media_type = std::move(*value); break; }
        case 16: if (!read_varint_as(reader, *field, lease.expires_qpc)) return std::nullopt; break;
        case 17: if (!read_varint_as(reader, *field, lease.qpc_frequency)) return std::nullopt; break;
        case 18: if (!read_varint_as(reader, *field, lease.cancellation_generation)) return std::nullopt; break;
        case 19: { auto value = reader.read_string(*field, 64U); if (!value) return std::nullopt; lease.capture_session_id = std::move(*value); break; }
        case 20: if (!read_varint_as(reader, *field, lease.selected_process_id)) return std::nullopt; break;
        case 21: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.selected_window_handle = *value; break; }
        case 22: { auto value = reader.read_string(*field, 260U); if (!value) return std::nullopt; lease.selected_executable_name = std::move(*value); break; }
        case 23: if (!read_varint_as(reader, *field, lease.source_device_generation)) return std::nullopt; break;
        case 24: if (!read_varint_as(reader, *field, lease.source_geometry_epoch)) return std::nullopt; break;
        case 25: { auto value = reader.read_string(*field, 128U); if (!value) return std::nullopt; lease.picker_consent_token = std::move(*value); break; }
        case 26: { auto value = reader.read_string(*field, 128U); if (!value) return std::nullopt; lease.game_profile_id = std::move(*value); break; }
        case 27: { auto value = reader.read_string(*field, 128U); if (!value) return std::nullopt; lease.character_id = std::move(*value); break; }
        case 28: { auto value = reader.read_string(*field, 128U); if (!value) return std::nullopt; lease.subject_id = std::move(*value); break; }
        case 29: { auto value = reader.read_string(*field, 128U); if (!value) return std::nullopt; lease.reference_id = std::move(*value); break; }
        case 30: { auto value = reader.read_string(*field, 256U); if (!value) return std::nullopt; lease.subject_display_name = std::move(*value); break; }
        case 31: if (!read_varint_as(reader, *field, source_class) || source_class < 1U || source_class > 2U) return std::nullopt; lease.source_class = static_cast<IdentityReferenceSourceClass>(source_class); break;
        case 32: { auto value = reader.read_string(*field, 128U); if (!value) return std::nullopt; lease.owner_user_id = std::move(*value); break; }
        case 33: { auto value = reader.read_string(*field, 512U); if (!value) return std::nullopt; lease.original_work_license = std::move(*value); break; }
        case 34: if (!read_varint_as(reader, *field, explicit_user_consent) || explicit_user_consent > 1U) return std::nullopt; lease.explicit_user_consent = explicit_user_consent != 0U; break;
        case 35: if (!read_varint_as(reader, *field, local_only) || local_only > 1U) return std::nullopt; lease.local_only = local_only != 0U; break;
        case 36: if (!read_varint_as(reader, *field, lease.imported_at_unix_ms)) return std::nullopt; break;
        default: return std::nullopt;
        }
    }
    if (!reader.good()) return std::nullopt;
    for (std::size_t field = 1U; field < seen.size(); ++field) {
        if (!seen[field]) return std::nullopt;
    }
    return encode_identity_reference_import_lease(lease).has_value()
               ? std::optional<IdentityReferenceImportLease>{std::move(lease)}
               : std::nullopt;
}

std::optional<VisualSourceLease> decode_visual_source_lease(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    VisualSourceLease lease{};
    std::array<bool, 29> seen{};
    while (const auto field = reader.next()) {
        if (field->number == 0U || field->number >= seen.size()) {
            if (!reader.skip(*field)) return std::nullopt;
            continue;
        }
        seen[field->number] = true;
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, lease.schema_version)) return std::nullopt; break;
        case 2: if (!read_varint_as(reader, *field, lease.broker_process_id)) return std::nullopt; break;
        case 3: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.broker_process_creation_time = *value; break; }
        case 4: { auto value = reader.read_string(*field, 260U); if (!value) return std::nullopt; lease.broker_executable_name = std::move(*value); break; }
        case 5: if (!read_varint_as(reader, *field, lease.worker_process_id)) return std::nullopt; break;
        case 6: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.worker_process_creation_time = *value; break; }
        case 7: { auto value = reader.read_string(*field, 260U); if (!value) return std::nullopt; lease.worker_executable_name = std::move(*value); break; }
        case 8: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.worker_handle_value = *value; break; }
        case 9: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.lease_nonce_high = *value; break; }
        case 10: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.lease_nonce_low = *value; break; }
        case 11: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.adapter_luid = *value; break; }
        case 12: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.keyed_mutex_acquire_key = *value; break; }
        case 13: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; lease.keyed_mutex_release_key = *value; break; }
        case 14: if (!read_varint_as(reader, *field, lease.width)) return std::nullopt; break;
        case 15: if (!read_varint_as(reader, *field, lease.height)) return std::nullopt; break;
        case 16: if (!read_varint_as(reader, *field, lease.stride_bytes)) return std::nullopt; break;
        case 17: if (!read_varint_as(reader, *field, lease.dxgi_format)) return std::nullopt; break;
        case 18: if (!read_varint_as(reader, *field, lease.alpha_mode)) return std::nullopt; break;
        case 19: if (!read_varint_as(reader, *field, lease.expires_qpc)) return std::nullopt; break;
        case 20: if (!read_varint_as(reader, *field, lease.qpc_frequency)) return std::nullopt; break;
        case 21: if (!read_varint_as(reader, *field, lease.cancellation_generation)) return std::nullopt; break;
        case 22: if (!read_varint_as(reader, *field, lease.source_device_generation)) return std::nullopt; break;
        case 23: if (!read_varint_as(reader, *field, lease.source_geometry_epoch)) return std::nullopt; break;
        case 24: if (!read_varint_as(reader, *field, lease.source_frame_sequence)) return std::nullopt; break;
        case 25: if (!read_varint_as(reader, *field, lease.source_frame_qpc)) return std::nullopt; break;
        case 26: if (!read_varint_as(reader, *field, lease.actor_id)) return std::nullopt; break;
        case 27: if (!read_varint_as(reader, *field, lease.track_id)) return std::nullopt; break;
        case 28: if (!read_varint_as(reader, *field, lease.track_epoch)) return std::nullopt; break;
        default: if (!reader.skip(*field)) return std::nullopt;
        }
    }
    if (!reader.good()) return std::nullopt;
    for (std::size_t field = 1U; field < seen.size(); ++field) {
        if (!seen[field]) return std::nullopt;
    }
    return encode_visual_source_lease(lease).has_value()
        ? std::optional<VisualSourceLease>{std::move(lease)}
        : std::nullopt;
}

ResidualContractStatus validate_residual_contract(
    const SubmitPatchCommand& command,
    const ResidualValidationContext& context) noexcept {
    constexpr std::uint32_t bgra8_unorm = 87U;
    constexpr std::uint32_t premultiplied_alpha = 1U;
    constexpr double minimum_patch_confidence = 0.82;
    constexpr double maximum_width = 0.45;
    constexpr double maximum_height = 0.35;
    constexpr double maximum_area = 0.12;

    if (context.capture_backend != CaptureBackend::windows_graphics_capture ||
        !context.overlay_visuals_allowed) {
        return ResidualContractStatus::unavailable_capture_path;
    }
    if (!command.shared_texture) {
        return ResidualContractStatus::missing_texture;
    }
    const auto& texture = *command.shared_texture;
    if (texture.schema_version != 1U || texture.session_nonce != context.nonce ||
        texture.session_id != context.session_id) {
        return ResidualContractStatus::session_mismatch;
    }
    if (texture.worker_process_id == 0U ||
        texture.worker_process_id == context.broker_process_id ||
        texture.worker_process_id == context.selected_target_process_id ||
        texture.worker_process_creation_time == 0U || texture.worker_executable_name.empty() ||
        texture.worker_executable_name.find('/') != std::string::npos ||
        texture.worker_executable_name.find('\\') != std::string::npos) {
        return ResidualContractStatus::invalid_worker;
    }
    if (texture.lease_nonce_high == 0U || texture.lease_nonce_low == 0U) {
        return ResidualContractStatus::invalid_lease_nonce;
    }
    if (texture.source_process_handle_value == 0U) {
        return ResidualContractStatus::invalid_handle;
    }
    if (texture.adapter_luid == 0U) {
        return ResidualContractStatus::invalid_adapter;
    }
    if (texture.keyed_mutex_acquire_key == 0U || texture.keyed_mutex_release_key == 0U ||
        texture.keyed_mutex_acquire_key == texture.keyed_mutex_release_key) {
        return ResidualContractStatus::invalid_mutex_keys;
    }
    if (texture.dxgi_format != bgra8_unorm || texture.alpha_mode != premultiplied_alpha) {
        return ResidualContractStatus::invalid_format;
    }
    if (!context.latest_frame_size_px.valid() || texture.width == 0U || texture.height == 0U ||
        texture.stride_bytes != texture.width * 4U) {
        return ResidualContractStatus::invalid_extent;
    }
    if (command.cancellation_generation != context.cancellation_generation) {
        return ResidualContractStatus::wrong_cancellation_generation;
    }
    if (command.source_device_generation != context.device_generation) {
        return ResidualContractStatus::wrong_device_generation;
    }
    if (command.source_geometry_epoch != context.geometry_epoch) {
        return ResidualContractStatus::wrong_geometry_epoch;
    }
    // A CPU landmark pass is intentionally allowed to finish against the
    // immutable frame that the broker leased even when WGC has advanced. Only
    // future/unknown sequence numbers are rejected here; the Windows platform
    // separately proves that this exact frame was exported to the declared
    // worker/actor/track and the timing checks below cap its age.
    if (command.source_frame_sequence == 0U ||
        command.source_frame_sequence > context.latest_frame_sequence) {
        return ResidualContractStatus::wrong_frame;
    }
    if (command.source_frame_qpc == 0U ||
        command.source_frame_qpc > context.latest_frame_qpc) {
        return ResidualContractStatus::wrong_frame_qpc;
    }
    if (command.actor_id == 0U || command.track_id == 0U || command.track_epoch == 0U) {
        return ResidualContractStatus::invalid_track;
    }
    const RectF bounds{command.left, command.top, command.right, command.bottom};
    if (!unit_value(bounds.left) || !unit_value(bounds.top) ||
        !unit_value(bounds.right) || !unit_value(bounds.bottom) ||
        bounds.right <= bounds.left || bounds.bottom <= bounds.top ||
        bounds.width() > maximum_width || bounds.height() > maximum_height ||
        bounds.width() * bounds.height() > maximum_area) {
        return ResidualContractStatus::invalid_bounds;
    }
    if (!unit_value(command.confidence) || command.confidence < minimum_patch_confidence) {
        return ResidualContractStatus::invalid_confidence;
    }

    const auto frame_width = static_cast<double>(context.latest_frame_size_px.width);
    const auto frame_height = static_cast<double>(context.latest_frame_size_px.height);
    // Keep the portable contract bit-for-bit aligned with geometry.cpp's
    // map_normalized_rect rounding. A worker may only allocate the exact mouth
    // rectangle that the Windows compositor will address.
    const auto expected_left = static_cast<std::int64_t>(std::llround(bounds.left * frame_width));
    const auto expected_top = static_cast<std::int64_t>(std::llround(bounds.top * frame_height));
    const auto expected_right = static_cast<std::int64_t>(std::llround(bounds.right * frame_width));
    const auto expected_bottom = static_cast<std::int64_t>(std::llround(bounds.bottom * frame_height));
    if (expected_right <= expected_left || expected_bottom <= expected_top ||
        static_cast<std::uint64_t>(expected_right - expected_left) != texture.width ||
        static_cast<std::uint64_t>(expected_bottom - expected_top) != texture.height) {
        return ResidualContractStatus::invalid_extent;
    }

    if (context.qpc_frequency == 0U || context.now_qpc == 0U ||
        command.source_frame_qpc == 0U || command.produced_qpc < command.source_frame_qpc ||
        texture.expires_qpc < command.produced_qpc || texture.expires_qpc < context.now_qpc) {
        return ResidualContractStatus::invalid_timing;
    }
    const auto within_milliseconds = [&](const std::uint64_t later,
                                         const std::uint64_t earlier,
                                         const std::uint64_t milliseconds) {
        if (later < earlier) return false;
        const auto elapsed = static_cast<long double>(later - earlier);
        const auto limit = static_cast<long double>(context.qpc_frequency) *
                           static_cast<long double>(milliseconds) / 1000.0L;
        return elapsed <= limit;
    };
    if (!within_milliseconds(command.produced_qpc, command.source_frame_qpc,
                             visual_worker_result_deadline_ms) ||
        !within_milliseconds(context.now_qpc, command.source_frame_qpc,
                             visual_presentation_deadline_ms) ||
        !within_milliseconds(texture.expires_qpc, command.produced_qpc, 100U)) {
        return ResidualContractStatus::invalid_timing;
    }
    if (command.produced_qpc > context.now_qpc &&
        !within_milliseconds(command.produced_qpc, context.now_qpc, 2U)) {
        return ResidualContractStatus::invalid_timing;
    }
    return ResidualContractStatus::accepted;
}

std::string_view to_string(const ResidualContractStatus status) noexcept {
    switch (status) {
    case ResidualContractStatus::accepted: return "accepted";
    case ResidualContractStatus::unavailable_capture_path: return "unavailable_capture_path";
    case ResidualContractStatus::missing_texture: return "missing_texture";
    case ResidualContractStatus::session_mismatch: return "session_mismatch";
    case ResidualContractStatus::invalid_worker: return "invalid_worker";
    case ResidualContractStatus::invalid_lease_nonce: return "invalid_lease_nonce";
    case ResidualContractStatus::invalid_handle: return "invalid_handle";
    case ResidualContractStatus::invalid_adapter: return "invalid_adapter";
    case ResidualContractStatus::invalid_mutex_keys: return "invalid_mutex_keys";
    case ResidualContractStatus::invalid_format: return "invalid_format";
    case ResidualContractStatus::invalid_extent: return "invalid_extent";
    case ResidualContractStatus::wrong_cancellation_generation: return "wrong_cancellation_generation";
    case ResidualContractStatus::wrong_device_generation: return "wrong_device_generation";
    case ResidualContractStatus::wrong_geometry_epoch: return "wrong_geometry_epoch";
    case ResidualContractStatus::wrong_frame: return "wrong_frame";
    case ResidualContractStatus::wrong_frame_qpc: return "wrong_frame_qpc";
    case ResidualContractStatus::invalid_track: return "invalid_track";
    case ResidualContractStatus::invalid_bounds: return "invalid_bounds";
    case ResidualContractStatus::invalid_confidence: return "invalid_confidence";
    case ResidualContractStatus::invalid_timing: return "invalid_timing";
    }
    return "unknown";
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

std::optional<std::vector<std::byte>> encode_visual_audio_envelope(
    const VisualAudioEnvelope& envelope) {
    const auto valid = [](const std::string& text) {
        return !text.empty() && text.size() <= 128 && text.find('\0') == std::string::npos;
    };
    const auto valid_cues = [](const std::vector<playback::VisualSpeechCue>& cues) {
        if (cues.size() > playback::maximum_visual_speech_cues) return false;
        std::uint64_t previous_end{};
        bool first = true;
        for (const auto& cue : cues) {
            if (!playback::valid_visual_speech_cue(
                    cue, std::numeric_limits<std::uint64_t>::max()) ||
                (!first && cue.start_sample < previous_end)) return false;
            previous_end = cue.start_sample + cue.duration_samples;
            first = false;
        }
        return true;
    };
    const bool source_range_valid =
        envelope.source_sample_start <=
            std::numeric_limits<std::uint64_t>::max() - envelope.source_sample_count &&
        envelope.source_frames >= envelope.source_sample_start + envelope.source_sample_count;
    if (envelope.schema_version != 1 || !valid(envelope.session_id) ||
        !valid(envelope.turn_id) || envelope.generation == 0 ||
        !valid(envelope.stream_id) || !valid(envelope.segment_id) ||
        envelope.source_sample_count == 0 || envelope.sample_rate < 8'000 ||
        envelope.sample_rate > 192'000 || envelope.channels == 0 || envelope.channels > 2 ||
        envelope.device_write_qpc == 0 || envelope.qpc_frequency == 0 || !source_range_valid ||
        envelope.device_frames == 0 || envelope.cancelled || !valid_cues(envelope.visual_speech_cues) ||
        (envelope.active == envelope.draining)) return std::nullopt;
    Writer writer;
    writer.varint_field(1, envelope.schema_version);
    writer.string_field(2, envelope.session_id);
    writer.string_field(3, envelope.turn_id);
    writer.varint_field(4, envelope.generation);
    writer.string_field(5, envelope.stream_id);
    writer.string_field(6, envelope.segment_id);
    writer.varint_field(7, envelope.source_sample_start);
    writer.varint_field(8, envelope.source_sample_count);
    writer.varint_field(9, envelope.sample_rate);
    writer.varint_field(10, envelope.channels);
    writer.varint_field(11, envelope.device_write_qpc);
    writer.varint_field(12, envelope.qpc_frequency);
    writer.varint_field(13, envelope.source_frames);
    writer.varint_field(14, envelope.device_frames);
    for (const auto value : envelope.mono_rms_q15) writer.varint_field(15, value);
    for (const auto value : envelope.mono_peak_q15) writer.varint_field(16, value);
    writer.varint_field(17, envelope.active ? 1U : 0U);
    writer.varint_field(18, envelope.draining ? 1U : 0U);
    writer.varint_field(19, envelope.cancelled ? 1U : 0U);
    for (const auto& cue : envelope.visual_speech_cues) {
        Writer nested;
        nested.varint_field(1, cue.start_sample);
        nested.varint_field(2, cue.duration_samples);
        nested.varint_field(3, cue.canonical_viseme);
        nested.varint_field(4, cue.strength_q15);
        writer.bytes_field(20, std::move(nested).finish());
    }
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)}
                                                 : std::nullopt;
}

std::optional<VisualAudioEnvelope> decode_visual_audio_envelope(
    const std::span<const std::byte> bytes) {
    Reader reader(bytes);
    VisualAudioEnvelope result;
    bool schema_seen{}, session_seen{}, turn_seen{}, generation_seen{}, stream_seen{},
         segment_seen{}, start_seen{}, count_seen{}, rate_seen{}, channels_seen{},
         write_seen{}, frequency_seen{}, source_frames_seen{}, device_frames_seen{},
         active_seen{}, draining_seen{}, cancelled_seen{};
    std::size_t rms_count{}, peak_count{};
    while (const auto field = reader.next()) {
        switch (field->number) {
        case 1: schema_seen = read_varint_as(reader, *field, result.schema_version); break;
        case 2: { auto value = reader.read_string(*field, 128); if (!value) return std::nullopt;
                  result.session_id = std::move(*value); session_seen = true; break; }
        case 3: { auto value = reader.read_string(*field, 128); if (!value) return std::nullopt;
                  result.turn_id = std::move(*value); turn_seen = true; break; }
        case 4: generation_seen = read_varint_as(reader, *field, result.generation); break;
        case 5: { auto value = reader.read_string(*field, 128); if (!value) return std::nullopt;
                  result.stream_id = std::move(*value); stream_seen = true; break; }
        case 6: { auto value = reader.read_string(*field, 128); if (!value) return std::nullopt;
                  result.segment_id = std::move(*value); segment_seen = true; break; }
        case 7: start_seen = read_varint_as(reader, *field, result.source_sample_start); break;
        case 8: count_seen = read_varint_as(reader, *field, result.source_sample_count); break;
        case 9: rate_seen = read_varint_as(reader, *field, result.sample_rate); break;
        case 10: channels_seen = read_varint_as(reader, *field, result.channels); break;
        case 11: write_seen = read_varint_as(reader, *field, result.device_write_qpc); break;
        case 12: frequency_seen = read_varint_as(reader, *field, result.qpc_frequency); break;
        case 13: source_frames_seen = read_varint_as(reader, *field, result.source_frames); break;
        case 14: device_frames_seen = read_varint_as(reader, *field, result.device_frames); break;
        case 15:
            if (rms_count >= result.mono_rms_q15.size() ||
                !read_varint_as(reader, *field, result.mono_rms_q15[rms_count++])) return std::nullopt;
            break;
        case 16:
            if (peak_count >= result.mono_peak_q15.size() ||
                !read_varint_as(reader, *field, result.mono_peak_q15[peak_count++])) return std::nullopt;
            break;
        case 17: active_seen = read_varint_as(reader, *field, result.active); break;
        case 18: draining_seen = read_varint_as(reader, *field, result.draining); break;
        case 19: cancelled_seen = read_varint_as(reader, *field, result.cancelled); break;
        case 20: {
            if (result.visual_speech_cues.size() >= playback::maximum_visual_speech_cues) {
                return std::nullopt;
            }
            const auto nested_bytes = reader.read_bytes(*field);
            if (!nested_bytes) return std::nullopt;
            Reader nested(*nested_bytes);
            playback::VisualSpeechCue cue;
            std::uint32_t viseme{};
            std::uint32_t strength{};
            bool cue_start_seen{}, cue_duration_seen{}, cue_viseme_seen{}, cue_strength_seen{};
            while (const auto cue_field = nested.next()) {
                switch (cue_field->number) {
                case 1:
                    if (cue_start_seen || !read_varint_as(nested, *cue_field, cue.start_sample)) {
                        return std::nullopt;
                    }
                    cue_start_seen = true;
                    break;
                case 2:
                    if (cue_duration_seen ||
                        !read_varint_as(nested, *cue_field, cue.duration_samples)) {
                        return std::nullopt;
                    }
                    cue_duration_seen = true;
                    break;
                case 3:
                    if (cue_viseme_seen || !read_varint_as(nested, *cue_field, viseme)) {
                        return std::nullopt;
                    }
                    cue_viseme_seen = true;
                    break;
                case 4:
                    if (cue_strength_seen || !read_varint_as(nested, *cue_field, strength)) {
                        return std::nullopt;
                    }
                    cue_strength_seen = true;
                    break;
                default:
                    if (!nested.skip(*cue_field)) return std::nullopt;
                    break;
                }
            }
            if (!nested.good() || !cue_start_seen || !cue_duration_seen || !cue_viseme_seen ||
                !cue_strength_seen || viseme > 10 || strength > 32'767) return std::nullopt;
            cue.canonical_viseme = static_cast<std::uint8_t>(viseme);
            cue.strength_q15 = static_cast<std::uint16_t>(strength);
            if (!playback::valid_visual_speech_cue(
                    cue, std::numeric_limits<std::uint64_t>::max())) return std::nullopt;
            if (!result.visual_speech_cues.empty()) {
                const auto& previous = result.visual_speech_cues.back();
                if (cue.start_sample < previous.start_sample + previous.duration_samples) {
                    return std::nullopt;
                }
            }
            result.visual_speech_cues.push_back(cue);
            break;
        }
        default: if (!reader.skip(*field)) return std::nullopt;
        }
    }
    const auto encoded = encode_visual_audio_envelope(result);
    return reader.good() && schema_seen && session_seen && turn_seen && generation_seen &&
                   stream_seen && segment_seen && start_seen && count_seen && rate_seen &&
                   channels_seen && write_seen && frequency_seen && source_frames_seen &&
                   device_frames_seen && active_seen && draining_seen && cancelled_seen &&
                   rms_count == 8 && peak_count == 8 && encoded
               ? std::optional{std::move(result)} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_ptt_activation_state(
    const PttActivationState& state) {
    const bool valid_state = state.state == PttState::released ||
                             state.state == PttState::pressed;
    const bool valid_release = state.release_transition_sequence > 0 &&
        state.release_transition_sequence <= state.transition_sequence &&
        state.released_qpc > 0 && state.released_qpc <= state.transition_qpc;
    const bool coherent = state.state == PttState::released
        ? state.release_transition_sequence == state.transition_sequence &&
              state.released_qpc == state.transition_qpc
        : state.release_transition_sequence < state.transition_sequence &&
              state.released_qpc < state.transition_qpc;
    if (state.schema_version != 1 || state.virtual_key == 0 || !valid_state ||
        state.transition_sequence == 0 || state.transition_qpc == 0 ||
        !valid_release || !coherent) return std::nullopt;
    Writer writer;
    writer.varint_field(1, state.schema_version);
    writer.varint_field(2, state.virtual_key);
    writer.varint_field(3, state.state == PttState::pressed ? 1U : 0U);
    writer.varint_field(4, state.transition_sequence);
    writer.varint_field(5, state.transition_qpc);
    writer.varint_field(6, state.release_transition_sequence);
    writer.varint_field(7, state.released_qpc);
    return std::move(writer).finish();
}

std::optional<PttActivationState> decode_ptt_activation_state(
    const std::span<const std::byte> bytes) {
    Reader reader(bytes);
    PttActivationState result;
    bool schema_seen{}, key_seen{}, state_seen{}, sequence_seen{}, qpc_seen{},
         release_sequence_seen{}, release_qpc_seen{};
    while (const auto field = reader.next()) {
        switch (field->number) {
        case 1: schema_seen = read_varint_as(reader, *field, result.schema_version); break;
        case 2: key_seen = read_varint_as(reader, *field, result.virtual_key); break;
        case 3: {
            std::uint32_t value{};
            state_seen = read_varint_as(reader, *field, value);
            if (!state_seen || value > 1) return std::nullopt;
            result.state = value == 1 ? PttState::pressed : PttState::released;
            break;
        }
        case 4: sequence_seen = read_varint_as(reader, *field, result.transition_sequence); break;
        case 5: qpc_seen = read_varint_as(reader, *field, result.transition_qpc); break;
        case 6:
            release_sequence_seen = read_varint_as(
                reader, *field, result.release_transition_sequence);
            break;
        case 7: release_qpc_seen = read_varint_as(reader, *field, result.released_qpc); break;
        default: if (!reader.skip(*field)) return std::nullopt;
        }
    }
    return reader.good() && schema_seen && key_seen && state_seen && sequence_seen &&
                   qpc_seen && release_sequence_seen && release_qpc_seen &&
                   encode_ptt_activation_state(result).has_value()
               ? std::optional{result} : std::nullopt;
}

std::optional<std::vector<std::byte>> encode_manual_actor_picker_receipt(
    const ManualActorPickerReceipt& receipt) {
    if (!valid_manual_actor_receipt(receipt)) return std::nullopt;
    Writer writer;
    writer.varint_field(1, receipt.schema_version);
    writer.string_field(2, receipt.request_id);
    writer.varint_field(3, static_cast<std::uint32_t>(receipt.status));
    writer.fixed64_field(4, receipt.receipt_nonce_high);
    writer.fixed64_field(5, receipt.receipt_nonce_low);
    writer.string_field(6, receipt.capture_session_id);
    writer.varint_field(7, receipt.cancellation_generation);
    writer.varint_field(8, receipt.selected_process_id);
    writer.fixed64_field(9, receipt.selected_window_handle);
    writer.string_field(10, receipt.selected_executable_name);
    writer.varint_field(11, receipt.source_device_generation);
    writer.varint_field(12, receipt.source_geometry_epoch);
    writer.varint_field(13, receipt.source_frame_sequence);
    writer.varint_field(14, receipt.source_frame_qpc);
    writer.varint_field(15, receipt.selected_actor_id);
    writer.varint_field(16, receipt.selected_track_id);
    writer.varint_field(17, receipt.selected_track_epoch);
    writer.varint_field(18, receipt.candidate_count);
    writer.string_field(19, receipt.candidate_set_sha256);
    writer.varint_field(20, receipt.began_qpc);
    writer.varint_field(21, receipt.clicked_qpc);
    writer.varint_field(22, receipt.attested_at_qpc);
    writer.varint_field(23, receipt.qpc_frequency);
    writer.varint_field(24, static_cast<std::uint32_t>(receipt.pointer_kind));
    writer.varint_field(25, receipt.frozen_wgc_frame_verified ? 1U : 0U);
    writer.varint_field(26, receipt.overlay_capture_excluded ? 1U : 0U);
    writer.varint_field(27, receipt.overlay_nonactivating ? 1U : 0U);
    writer.varint_field(28, receipt.single_hardware_pointer_click ? 1U : 0U);
    writer.varint_field(29, receipt.pixels_withheld_from_webview ? 1U : 0U);
    writer.varint_field(30, receipt.coordinates_withheld_from_webview ? 1U : 0U);
    auto result = std::move(writer).finish();
    return result.size() <= maximum_frame_bytes ? std::optional{std::move(result)}
                                                 : std::nullopt;
}

std::optional<ManualActorPickerReceipt> decode_manual_actor_picker_receipt(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_frame_bytes) return std::nullopt;
    Reader reader(bytes);
    ManualActorPickerReceipt result;
    std::array<bool, 31> seen{};
    std::uint32_t status{}, pointer{};
    const auto read_boolean = [&](const Reader::Field& field, bool& destination) {
        std::uint32_t value{};
        if (!read_varint_as(reader, field, value) || value > 1U) return false;
        destination = value != 0U;
        return true;
    };
    while (const auto field = reader.next()) {
        if (field->number == 0U || field->number >= seen.size() || seen[field->number]) {
            return std::nullopt;
        }
        seen[field->number] = true;
        switch (field->number) {
        case 1: if (!read_varint_as(reader, *field, result.schema_version)) return std::nullopt; break;
        case 2: { auto value = reader.read_string(*field, 128U); if (!value) return std::nullopt; result.request_id = std::move(*value); break; }
        case 3: if (!read_varint_as(reader, *field, status)) return std::nullopt; result.status = static_cast<ManualActorPickerStatus>(status); break;
        case 4: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; result.receipt_nonce_high = *value; break; }
        case 5: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; result.receipt_nonce_low = *value; break; }
        case 6: { auto value = reader.read_string(*field, 64U); if (!value) return std::nullopt; result.capture_session_id = std::move(*value); break; }
        case 7: if (!read_varint_as(reader, *field, result.cancellation_generation)) return std::nullopt; break;
        case 8: if (!read_varint_as(reader, *field, result.selected_process_id)) return std::nullopt; break;
        case 9: { const auto value = reader.read_fixed64(*field); if (!value) return std::nullopt; result.selected_window_handle = *value; break; }
        case 10: { auto value = reader.read_string(*field, 260U); if (!value) return std::nullopt; result.selected_executable_name = std::move(*value); break; }
        case 11: if (!read_varint_as(reader, *field, result.source_device_generation)) return std::nullopt; break;
        case 12: if (!read_varint_as(reader, *field, result.source_geometry_epoch)) return std::nullopt; break;
        case 13: if (!read_varint_as(reader, *field, result.source_frame_sequence)) return std::nullopt; break;
        case 14: if (!read_varint_as(reader, *field, result.source_frame_qpc)) return std::nullopt; break;
        case 15: if (!read_varint_as(reader, *field, result.selected_actor_id)) return std::nullopt; break;
        case 16: if (!read_varint_as(reader, *field, result.selected_track_id)) return std::nullopt; break;
        case 17: if (!read_varint_as(reader, *field, result.selected_track_epoch)) return std::nullopt; break;
        case 18: if (!read_varint_as(reader, *field, result.candidate_count)) return std::nullopt; break;
        case 19: { auto value = reader.read_string(*field, 64U); if (!value) return std::nullopt; result.candidate_set_sha256 = std::move(*value); break; }
        case 20: if (!read_varint_as(reader, *field, result.began_qpc)) return std::nullopt; break;
        case 21: if (!read_varint_as(reader, *field, result.clicked_qpc)) return std::nullopt; break;
        case 22: if (!read_varint_as(reader, *field, result.attested_at_qpc)) return std::nullopt; break;
        case 23: if (!read_varint_as(reader, *field, result.qpc_frequency)) return std::nullopt; break;
        case 24: if (!read_varint_as(reader, *field, pointer)) return std::nullopt; result.pointer_kind = static_cast<ManualActorPointerKind>(pointer); break;
        case 25: if (!read_boolean(*field, result.frozen_wgc_frame_verified)) return std::nullopt; break;
        case 26: if (!read_boolean(*field, result.overlay_capture_excluded)) return std::nullopt; break;
        case 27: if (!read_boolean(*field, result.overlay_nonactivating)) return std::nullopt; break;
        case 28: if (!read_boolean(*field, result.single_hardware_pointer_click)) return std::nullopt; break;
        case 29: if (!read_boolean(*field, result.pixels_withheld_from_webview)) return std::nullopt; break;
        case 30: if (!read_boolean(*field, result.coordinates_withheld_from_webview)) return std::nullopt; break;
        default: return std::nullopt;
        }
    }
    return reader.good() &&
                   std::all_of(seen.begin() + 1, seen.end(), [](const bool value) { return value; }) &&
                   valid_manual_actor_receipt(result)
               ? std::optional<ManualActorPickerReceipt>{std::move(result)}
               : std::nullopt;
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
