#include "npc/mouth_worker/service_protocol.hpp"

#include <bit>
#include <cstring>
#include <limits>
#include <type_traits>
#include <utility>

namespace npc::mouth::service {
namespace {

template <typename T, bool = std::is_enum_v<T>>
struct ScalarRaw {
    using type = T;
};

template <typename T>
struct ScalarRaw<T, true> {
    using type = std::underlying_type_t<T>;
};

class Writer final {
public:
    template <typename T>
    void scalar(const T value) {
        static_assert(std::is_integral_v<T> || std::is_enum_v<T>);
        using Raw = typename ScalarRaw<T>::type;
        using Unsigned = std::make_unsigned_t<Raw>;
        Unsigned raw = static_cast<Unsigned>(value);
        for (std::size_t index = 0; index < sizeof(Unsigned); ++index) {
            bytes_.push_back(static_cast<std::byte>((raw >> (index * 8U)) & 0xffU));
        }
    }

    void boolean(const bool value) { scalar<std::uint8_t>(value ? 1U : 0U); }
    void floating(const double value) { scalar(std::bit_cast<std::uint64_t>(value)); }

    void raw(const std::span<const std::byte> value) {
        bytes_.insert(bytes_.end(), value.begin(), value.end());
    }

    void string(const std::string_view value) {
        scalar(static_cast<std::uint32_t>(value.size()));
        raw(std::as_bytes(std::span{value.data(), value.size()}));
    }

    [[nodiscard]] std::vector<std::byte> take() && { return std::move(bytes_); }

private:
    std::vector<std::byte> bytes_;
};

class Reader final {
public:
    explicit Reader(const std::span<const std::byte> bytes) : bytes_(bytes) {}

    template <typename T>
    [[nodiscard]] bool scalar(T& value) {
        static_assert(std::is_integral_v<T> || std::is_enum_v<T>);
        using Raw = typename ScalarRaw<T>::type;
        using Unsigned = std::make_unsigned_t<Raw>;
        if (remaining() < sizeof(Unsigned)) return false;
        Unsigned raw{};
        for (std::size_t index = 0; index < sizeof(Unsigned); ++index) {
            raw |= static_cast<Unsigned>(std::to_integer<std::uint8_t>(bytes_[offset_ + index]))
                   << (index * 8U);
        }
        offset_ += sizeof(Unsigned);
        value = static_cast<T>(raw);
        return true;
    }

    [[nodiscard]] bool boolean(bool& value) {
        std::uint8_t raw{};
        if (!scalar(raw) || raw > 1U) return false;
        value = raw == 1U;
        return true;
    }

    [[nodiscard]] bool floating(double& value) {
        std::uint64_t raw{};
        if (!scalar(raw)) return false;
        value = std::bit_cast<double>(raw);
        return true;
    }

    [[nodiscard]] bool raw(const std::span<std::byte> destination) {
        if (remaining() < destination.size()) return false;
        std::memcpy(destination.data(), bytes_.data() + offset_, destination.size());
        offset_ += destination.size();
        return true;
    }

    [[nodiscard]] bool vector(std::vector<std::byte>& value, const std::uint32_t maximum) {
        std::uint32_t count{};
        if (!scalar(count) || count > maximum || remaining() < count) return false;
        value.assign(bytes_.begin() + static_cast<std::ptrdiff_t>(offset_),
                     bytes_.begin() + static_cast<std::ptrdiff_t>(offset_ + count));
        offset_ += count;
        return true;
    }

    [[nodiscard]] bool string(std::string& value, const std::uint32_t maximum) {
        std::uint32_t count{};
        if (!scalar(count) || count > maximum || remaining() < count) return false;
        value.assign(reinterpret_cast<const char*>(bytes_.data() + offset_), count);
        offset_ += count;
        return true;
    }

    [[nodiscard]] bool done() const noexcept { return offset_ == bytes_.size(); }
    [[nodiscard]] std::size_t remaining() const noexcept { return bytes_.size() - offset_; }

private:
    std::span<const std::byte> bytes_;
    std::size_t offset_{};
};

void write_session(Writer& writer, const SessionBindingV1& value) {
    writer.raw(value.nonce);
    writer.scalar(value.session_id_high);
    writer.scalar(value.session_id_low);
}

[[nodiscard]] bool read_session(Reader& reader, SessionBindingV1& value) {
    return reader.raw(value.nonce) && reader.scalar(value.session_id_high) &&
           reader.scalar(value.session_id_low);
}

void write_request_identity(Writer& writer, const ProductRequestIdentity& value) {
    writer.scalar(value.request_id);
    writer.scalar(value.session_id_high);
    writer.scalar(value.session_id_low);
    writer.scalar(value.turn_id_high);
    writer.scalar(value.turn_id_low);
    writer.scalar(value.sentence_id);
}

[[nodiscard]] bool read_request_identity(Reader& reader, ProductRequestIdentity& value) {
    return reader.scalar(value.request_id) && reader.scalar(value.session_id_high) &&
           reader.scalar(value.session_id_low) && reader.scalar(value.turn_id_high) &&
           reader.scalar(value.turn_id_low) && reader.scalar(value.sentence_id);
}

void write_track(Writer& writer, const TrackBinding& value) {
    writer.scalar(value.cancellation_generation);
    writer.scalar(value.actor_id);
    writer.scalar(value.track_id);
    writer.scalar(value.track_epoch);
}

[[nodiscard]] bool read_track(Reader& reader, TrackBinding& value) {
    return reader.scalar(value.cancellation_generation) && reader.scalar(value.actor_id) &&
           reader.scalar(value.track_id) && reader.scalar(value.track_epoch);
}

void write_frame(Writer& writer, const FrameIdentity& value) {
    writer.scalar(value.sequence);
    writer.scalar(value.device_generation);
    writer.scalar(value.geometry_epoch);
    writer.scalar(value.captured_at_ns);
}

[[nodiscard]] bool read_frame(Reader& reader, FrameIdentity& value) {
    return reader.scalar(value.sequence) && reader.scalar(value.device_generation) &&
           reader.scalar(value.geometry_epoch) && reader.scalar(value.captured_at_ns);
}

void write_rect(Writer& writer, const NormalizedRect& value) {
    writer.floating(value.x);
    writer.floating(value.y);
    writer.floating(value.width);
    writer.floating(value.height);
}

[[nodiscard]] bool read_rect(Reader& reader, NormalizedRect& value) {
    return reader.floating(value.x) && reader.floating(value.y) &&
           reader.floating(value.width) && reader.floating(value.height);
}

void write_landmark(Writer& writer, const NormalizedLandmark& value) {
    writer.floating(value.x);
    writer.floating(value.y);
    writer.floating(value.confidence);
}

[[nodiscard]] bool read_landmark(Reader& reader, NormalizedLandmark& value) {
    return reader.floating(value.x) && reader.floating(value.y) &&
           reader.floating(value.confidence);
}

void write_texture(Writer& writer, const TextureLeaseDescriptor& value) {
    writer.scalar(value.schema_version);
    writer.scalar(value.transport);
    writer.scalar(value.lease_nonce_high);
    writer.scalar(value.lease_nonce_low);
    writer.scalar(value.owner_process_id);
    writer.scalar(value.intended_consumer_process_id);
    writer.scalar(value.native_handle_value);
    writer.scalar(value.adapter_luid_low);
    writer.scalar(value.adapter_luid_high);
    writer.scalar(value.keyed_mutex_acquire_key);
    writer.scalar(value.keyed_mutex_release_key);
    writer.scalar(value.width);
    writer.scalar(value.height);
    writer.scalar(value.stride_bytes);
    writer.scalar(value.format);
    writer.scalar(value.expires_at_ns);
}

[[nodiscard]] bool read_texture(Reader& reader, TextureLeaseDescriptor& value) {
    return reader.scalar(value.schema_version) && reader.scalar(value.transport) &&
           reader.scalar(value.lease_nonce_high) && reader.scalar(value.lease_nonce_low) &&
           reader.scalar(value.owner_process_id) &&
           reader.scalar(value.intended_consumer_process_id) &&
           reader.scalar(value.native_handle_value) && reader.scalar(value.adapter_luid_low) &&
           reader.scalar(value.adapter_luid_high) &&
           reader.scalar(value.keyed_mutex_acquire_key) &&
           reader.scalar(value.keyed_mutex_release_key) && reader.scalar(value.width) &&
           reader.scalar(value.height) && reader.scalar(value.stride_bytes) &&
           reader.scalar(value.format) && reader.scalar(value.expires_at_ns);
}

void write_coefficients(Writer& writer, const MouthCoefficients& value) {
    writer.floating(value.jaw_open);
    writer.floating(value.lip_close);
    writer.floating(value.funnel);
    writer.floating(value.pucker);
    writer.floating(value.smile_left);
    writer.floating(value.smile_right);
    writer.floating(value.upper_lip_raise);
    writer.floating(value.lower_lip_depress);
}

void write_pose(Writer& writer, const HeadPoseDegrees& value) {
    writer.floating(value.yaw);
    writer.floating(value.pitch);
    writer.floating(value.roll);
}

[[nodiscard]] bool read_pose(Reader& reader, HeadPoseDegrees& value) {
    return reader.floating(value.yaw) && reader.floating(value.pitch) &&
           reader.floating(value.roll);
}

[[nodiscard]] bool read_coefficients(Reader& reader, MouthCoefficients& value) {
    return reader.floating(value.jaw_open) && reader.floating(value.lip_close) &&
           reader.floating(value.funnel) && reader.floating(value.pucker) &&
           reader.floating(value.smile_left) && reader.floating(value.smile_right) &&
           reader.floating(value.upper_lip_raise) &&
           reader.floating(value.lower_lip_depress);
}

void write_drive(Writer& writer, const MouthDrive& value) {
    writer.scalar(value.kind);
    writer.scalar(value.clock.stream_generation);
    writer.scalar(value.clock.segment_id);
    writer.scalar(value.clock.first_sample_index);
    writer.scalar(value.clock.sample_rate);
    writer.scalar(value.clock.channels);
    writer.scalar(value.clock.playback_at_ns);
    write_coefficients(writer, value.coefficients);
    writer.scalar(value.viseme);
    writer.floating(value.viseme_strength);
    writer.scalar(static_cast<std::uint32_t>(value.interleaved_pcm.size()));
    for (const float sample : value.interleaved_pcm) {
        writer.scalar(std::bit_cast<std::uint32_t>(sample));
    }
}

[[nodiscard]] bool read_drive(Reader& reader, MouthDrive& value) {
    std::uint32_t count{};
    if (!reader.scalar(value.kind) || !reader.scalar(value.clock.stream_generation) ||
        !reader.scalar(value.clock.segment_id) ||
        !reader.scalar(value.clock.first_sample_index) ||
        !reader.scalar(value.clock.sample_rate) || !reader.scalar(value.clock.channels) ||
        !reader.scalar(value.clock.playback_at_ns) ||
        !read_coefficients(reader, value.coefficients) || !reader.scalar(value.viseme) ||
        !reader.floating(value.viseme_strength) || !reader.scalar(count) ||
        count > maximum_pcm_samples || reader.remaining() < count * sizeof(std::uint32_t)) {
        return false;
    }
    value.interleaved_pcm.resize(count);
    for (auto& sample : value.interleaved_pcm) {
        std::uint32_t raw{};
        if (!reader.scalar(raw)) return false;
        sample = std::bit_cast<float>(raw);
    }
    return true;
}

void write_appearance(Writer& writer, const AppearanceGateEvidenceV1& value) {
    writer.scalar(value.schema_version);
    writer.scalar(value.runtime_actor_id);
    writer.scalar(value.descriptor_revision);
    writer.scalar(value.expected_descriptor_digest_high);
    writer.scalar(value.expected_descriptor_digest_low);
    writer.scalar(value.observed_descriptor_digest_high);
    writer.scalar(value.observed_descriptor_digest_low);
    writer.floating(value.similarity);
    writer.floating(value.temporal_iou);
    writer.floating(value.blocker_coverage);
    writer.boolean(value.identity_locked);
    writer.boolean(value.target_visible);
    writer.boolean(value.scene_transition);
}

[[nodiscard]] bool read_appearance(Reader& reader, AppearanceGateEvidenceV1& value) {
    return reader.scalar(value.schema_version) && reader.scalar(value.runtime_actor_id) &&
           reader.scalar(value.descriptor_revision) &&
           reader.scalar(value.expected_descriptor_digest_high) &&
           reader.scalar(value.expected_descriptor_digest_low) &&
           reader.scalar(value.observed_descriptor_digest_high) &&
           reader.scalar(value.observed_descriptor_digest_low) &&
           reader.floating(value.similarity) && reader.floating(value.temporal_iou) &&
           reader.floating(value.blocker_coverage) && reader.boolean(value.identity_locked) &&
           reader.boolean(value.target_visible) && reader.boolean(value.scene_transition);
}

void write_resources(Writer& writer, const VisualResourceStateV1& value) {
    writer.scalar(value.schema_version);
    writer.scalar(value.pressure);
    writer.scalar(value.admitted_signal_rate_hz);
    writer.boolean(value.local_visuals_admitted);
}

[[nodiscard]] bool read_resources(Reader& reader, VisualResourceStateV1& value) {
    return reader.scalar(value.schema_version) && reader.scalar(value.pressure) &&
           reader.scalar(value.admitted_signal_rate_hz) &&
           reader.boolean(value.local_visuals_admitted);
}

void write_source(Writer& writer, const SourceTextureLeaseV1& value) {
    writer.scalar(value.schema_version);
    write_session(writer, value.session);
    write_texture(writer, value.texture);
    writer.scalar(value.broker_process_id);
    writer.scalar(value.broker_process_creation_time);
    writer.string(value.broker_executable_name);
    writer.scalar(value.source_frame_qpc);
    writer.scalar(value.qpc_frequency);
}

[[nodiscard]] bool read_source(Reader& reader, SourceTextureLeaseV1& value) {
    return reader.scalar(value.schema_version) && read_session(reader, value.session) &&
           read_texture(reader, value.texture) && reader.scalar(value.broker_process_id) &&
           reader.scalar(value.broker_process_creation_time) &&
           reader.string(value.broker_executable_name, 260U) &&
           reader.scalar(value.source_frame_qpc) && reader.scalar(value.qpc_frequency);
}

void write_receipt(Writer& writer, const PresentationReceiptV1& value) {
    writer.scalar(value.schema_version);
    write_request_identity(writer, value.request);
    write_track(writer, value.track);
    write_frame(writer, value.source_frame);
    writer.scalar(value.disposition);
    writer.scalar(value.signal_disposition);
    writer.scalar(value.worker_disposition);
    writer.scalar(value.drive_kind);
    writer.scalar(value.admitted_signal_rate_hz);
    writer.scalar(value.latch_generation);
    writer.scalar(value.submitted_at_ns);
    writer.scalar(value.completed_at_ns);
    writer.scalar(value.queue_replacements);
}

[[nodiscard]] bool read_receipt(Reader& reader, PresentationReceiptV1& value) {
    return reader.scalar(value.schema_version) && read_request_identity(reader, value.request) &&
           read_track(reader, value.track) && read_frame(reader, value.source_frame) &&
           reader.scalar(value.disposition) && reader.scalar(value.signal_disposition) &&
           reader.scalar(value.worker_disposition) && reader.scalar(value.drive_kind) &&
           reader.scalar(value.admitted_signal_rate_hz) &&
           reader.scalar(value.latch_generation) && reader.scalar(value.submitted_at_ns) &&
           reader.scalar(value.completed_at_ns) && reader.scalar(value.queue_replacements);
}

[[nodiscard]] bool valid_session(const SessionBindingV1& value) noexcept {
    bool nonce_nonzero = false;
    for (const auto byte : value.nonce) nonce_nonzero |= byte != std::byte{};
    return nonce_nonzero && (value.session_id_high != 0U || value.session_id_low != 0U);
}

[[nodiscard]] std::string path_utf8(const std::filesystem::path& value) {
    const auto encoded = value.generic_u8string();
    return {reinterpret_cast<const char*>(encoded.data()), encoded.size()};
}

[[nodiscard]] std::filesystem::path path_from_utf8(const std::string& value) {
    const auto* begin = reinterpret_cast<const char8_t*>(value.data());
    return std::filesystem::path(std::u8string(begin, begin + value.size()));
}

} // namespace

EnvelopeValidator::EnvelopeValidator(ValidationConfig config) : config_(std::move(config)) {}

StatusCode EnvelopeValidator::validate(const EnvelopeV1& envelope,
                                       const Nanoseconds now_ns,
                                       const std::uint64_t active_generation) noexcept {
    if (envelope.magic != protocol_magic || envelope.version != protocol_version) {
        return StatusCode::unsupported_version;
    }
    if (!valid_session(config_.session) || envelope.session.nonce != config_.session.nonce) {
        return StatusCode::authentication_failed;
    }
    if (envelope.session.session_id_high != config_.session.session_id_high ||
        envelope.session.session_id_low != config_.session.session_id_low) {
        return StatusCode::session_mismatch;
    }
    if (envelope.sequence == 0U || envelope.sequence <= last_sequence_) {
        return StatusCode::sequence_replayed;
    }
    if (now_ns <= 0 || envelope.deadline_ns < now_ns) {
        return StatusCode::deadline_expired;
    }
    if (envelope.deadline_ns - now_ns > config_.maximum_future_deadline_ns) {
        return StatusCode::invalid_frame;
    }
    if (envelope.command != CommandKind::cancel_generation &&
        envelope.cancellation_generation != active_generation) {
        return StatusCode::cancellation_mismatch;
    }
    last_sequence_ = envelope.sequence;
    return StatusCode::ok;
}

std::uint64_t EnvelopeValidator::last_sequence() const noexcept { return last_sequence_; }

std::optional<std::vector<std::byte>> encode_envelope(const EnvelopeV1& envelope) {
    if (envelope.payload.size() > maximum_message_bytes) return std::nullopt;
    Writer writer;
    writer.scalar(envelope.magic);
    writer.scalar(envelope.version);
    writer.scalar(envelope.command);
    write_session(writer, envelope.session);
    writer.scalar(envelope.sequence);
    writer.scalar(envelope.cancellation_generation);
    writer.scalar(envelope.deadline_ns);
    writer.scalar(static_cast<std::uint32_t>(envelope.payload.size()));
    writer.raw(envelope.payload);
    auto result = std::move(writer).take();
    if (result.size() > maximum_message_bytes) return std::nullopt;
    return result;
}

std::optional<EnvelopeV1> decode_envelope(const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_message_bytes) return std::nullopt;
    Reader reader(bytes);
    EnvelopeV1 value{};
    if (!reader.scalar(value.magic) || !reader.scalar(value.version) ||
        !reader.scalar(value.command) || !read_session(reader, value.session) ||
        !reader.scalar(value.sequence) || !reader.scalar(value.cancellation_generation) ||
        !reader.scalar(value.deadline_ns) ||
        !reader.vector(value.payload, maximum_message_bytes) || !reader.done()) {
        return std::nullopt;
    }
    return value;
}

std::optional<std::vector<std::byte>> encode_response(const WorkerResponseV1& response) {
    Writer writer;
    writer.scalar(response.magic);
    writer.scalar(response.version);
    writer.scalar(response.status);
    writer.scalar(response.response_to_sequence);
    writer.scalar(response.cancellation_generation);
    write_receipt(writer, response.receipt);
    writer.boolean(response.residual.has_value());
    if (response.residual) {
        const auto& value = *response.residual;
        writer.scalar(value.schema_version);
        write_request_identity(writer, value.request);
        write_track(writer, value.track);
        write_frame(writer, value.source_frame);
        writer.scalar(value.source_frame_qpc);
        write_rect(writer, value.normalized_bounds);
        write_texture(writer, value.residual);
        writer.floating(value.confidence);
        if (value.schema_version >= 2U) {
            writer.floating(value.detector_confidence);
            writer.floating(value.landmark_confidence);
            writer.floating(value.visibility_ratio);
            writer.boolean(value.mouth_occluded);
            writer.scalar(value.landmarks_measured_at_ns);
        }
        writer.scalar(value.produced_at_ns);
    }
    if (response.detail.size() > 1024U) return std::nullopt;
    writer.string(response.detail);
    auto result = std::move(writer).take();
    if (result.size() > maximum_message_bytes) return std::nullopt;
    return result;
}

std::optional<WorkerResponseV1> decode_response(const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_message_bytes) return std::nullopt;
    Reader reader(bytes);
    WorkerResponseV1 value{};
    bool has_residual{};
    if (!reader.scalar(value.magic) || !reader.scalar(value.version) ||
        !reader.scalar(value.status) || !reader.scalar(value.response_to_sequence) ||
        !reader.scalar(value.cancellation_generation) || !read_receipt(reader, value.receipt) ||
        !reader.boolean(has_residual)) {
        return std::nullopt;
    }
    if (has_residual) {
        ResidualProposalV1 residual{};
        if (!reader.scalar(residual.schema_version) ||
            !read_request_identity(reader, residual.request) || !read_track(reader, residual.track) ||
            !read_frame(reader, residual.source_frame) ||
            !reader.scalar(residual.source_frame_qpc) ||
            !read_rect(reader, residual.normalized_bounds) ||
            !read_texture(reader, residual.residual) || !reader.floating(residual.confidence)) {
            return std::nullopt;
        }
        if (residual.schema_version >= 2U &&
            (!reader.floating(residual.detector_confidence) ||
             !reader.floating(residual.landmark_confidence) ||
             !reader.floating(residual.visibility_ratio) ||
             !reader.boolean(residual.mouth_occluded) ||
             !reader.scalar(residual.landmarks_measured_at_ns))) {
            return std::nullopt;
        }
        if (!reader.scalar(residual.produced_at_ns)) return std::nullopt;
        value.residual = std::move(residual);
    }
    if (!reader.string(value.detail, 1024U) || !reader.done()) return std::nullopt;
    return value;
}

std::optional<std::vector<std::byte>> encode_render_command(
    const RenderCurrentFrameCommandV1& command) {
    if (command.source.broker_executable_name.size() > 260U ||
        command.drive.interleaved_pcm.size() > maximum_pcm_samples) {
        return std::nullopt;
    }
    Writer writer;
    write_request_identity(writer, command.request);
    write_source(writer, command.source);
    write_track(writer, command.track);
    write_frame(writer, command.frame);
    writer.scalar(command.landmarks.schema_version);
    writer.scalar(command.landmarks.provider_instance_id);
    write_track(writer, command.landmarks.track);
    write_frame(writer, command.landmarks.frame);
    writer.scalar(command.landmarks.source_frame_qpc);
    writer.scalar(command.landmarks.qpc_frequency);
    write_rect(writer, command.landmarks.face_bounds);
    for (const auto& landmark : command.landmarks.landmarks) write_landmark(writer, landmark);
    writer.floating(command.landmarks.pose.yaw);
    writer.floating(command.landmarks.pose.pitch);
    writer.floating(command.landmarks.pose.roll);
    writer.floating(command.landmarks.detector_confidence);
    writer.floating(command.landmarks.landmark_confidence);
    writer.floating(command.landmarks.visibility_ratio);
    writer.boolean(command.landmarks.mouth_occluded);
    writer.scalar(command.landmarks.measured_at_ns);
    write_appearance(writer, command.appearance);
    write_resources(writer, command.resources);
    write_drive(writer, command.drive);
    writer.scalar(command.deadline_ns);
    auto result = std::move(writer).take();
    if (result.size() > maximum_message_bytes) return std::nullopt;
    return result;
}

std::optional<RenderCurrentFrameCommandV1> decode_render_command(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_message_bytes) return std::nullopt;
    Reader reader(bytes);
    RenderCurrentFrameCommandV1 value{};
    if (!read_request_identity(reader, value.request) ||
        !read_source(reader, value.source) ||
        !read_track(reader, value.track) || !read_frame(reader, value.frame) ||
        !reader.scalar(value.landmarks.schema_version) ||
        !reader.scalar(value.landmarks.provider_instance_id) ||
        !read_track(reader, value.landmarks.track) || !read_frame(reader, value.landmarks.frame) ||
        !reader.scalar(value.landmarks.source_frame_qpc) ||
        !reader.scalar(value.landmarks.qpc_frequency) ||
        !read_rect(reader, value.landmarks.face_bounds)) {
        return std::nullopt;
    }
    for (auto& landmark : value.landmarks.landmarks) {
        if (!read_landmark(reader, landmark)) return std::nullopt;
    }
    if (!reader.floating(value.landmarks.pose.yaw) ||
        !reader.floating(value.landmarks.pose.pitch) ||
        !reader.floating(value.landmarks.pose.roll) ||
        !reader.floating(value.landmarks.detector_confidence) ||
        !reader.floating(value.landmarks.landmark_confidence) ||
        !reader.floating(value.landmarks.visibility_ratio) ||
        !reader.boolean(value.landmarks.mouth_occluded) ||
        !reader.scalar(value.landmarks.measured_at_ns) ||
        !read_appearance(reader, value.appearance) ||
        !read_resources(reader, value.resources) ||
        !read_drive(reader, value.drive) || !reader.scalar(value.deadline_ns) || !reader.done()) {
        return std::nullopt;
    }
    return value;
}

std::optional<std::vector<std::byte>> encode_admitted_render_command(
    const RenderWithAdmittedLandmarksCommandV1& command) {
    if (command.source.broker_executable_name.size() > 260U ||
        command.drive.interleaved_pcm.size() > maximum_pcm_samples) {
        return std::nullopt;
    }
    Writer writer;
    write_request_identity(writer, command.request);
    write_source(writer, command.source);
    write_track(writer, command.track);
    write_frame(writer, command.frame);
    write_rect(writer, command.seed_face_bounds);
    write_appearance(writer, command.appearance);
    write_resources(writer, command.resources);
    write_drive(writer, command.drive);
    writer.scalar(command.deadline_ns);
    auto result = std::move(writer).take();
    if (result.size() > maximum_message_bytes) return std::nullopt;
    return result;
}

std::optional<RenderWithAdmittedLandmarksCommandV1> decode_admitted_render_command(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_message_bytes) return std::nullopt;
    Reader reader(bytes);
    RenderWithAdmittedLandmarksCommandV1 value{};
    if (!read_request_identity(reader, value.request) || !read_source(reader, value.source) ||
        !read_track(reader, value.track) || !read_frame(reader, value.frame) ||
        !read_rect(reader, value.seed_face_bounds) || !read_appearance(reader, value.appearance) ||
        !read_resources(reader, value.resources) || !read_drive(reader, value.drive) ||
        !reader.scalar(value.deadline_ns) || !reader.done()) {
        return std::nullopt;
    }
    return value;
}

std::optional<std::vector<std::byte>> encode_provider_configuration(
    const ConfigureAdmittedLandmarkProviderCommandV1& command) {
    if (!validate_landmark_provider_launch_v1(command.launch)) return std::nullopt;
    const auto root = path_utf8(command.launch.artifact_root);
    const auto detector = path_utf8(command.launch.detector_model);
    const auto landmark = path_utf8(command.launch.landmark_model);
    const auto runtime = path_utf8(command.launch.runtime_library);
    const auto runtime_shared = path_utf8(command.launch.runtime_shared_library);
    if (root.size() > 2048U || detector.size() > 2048U || landmark.size() > 2048U ||
        runtime.size() > 2048U || runtime_shared.size() > 2048U) {
        return std::nullopt;
    }
    Writer writer;
    writer.scalar(command.launch.schema_version);
    writer.string(command.launch.pack_id);
    writer.string(command.launch.pack_revision);
    writer.string(root);
    writer.string(detector);
    writer.string(landmark);
    writer.string(runtime);
    writer.string(runtime_shared);
    writer.scalar(command.launch.detector_size_bytes);
    writer.scalar(command.launch.landmark_size_bytes);
    writer.scalar(command.launch.runtime_size_bytes);
    writer.scalar(command.launch.runtime_shared_size_bytes);
    writer.string(command.launch.detector_sha256);
    writer.string(command.launch.landmark_sha256);
    writer.string(command.launch.runtime_sha256);
    writer.string(command.launch.runtime_shared_sha256);
    writer.string(command.launch.measured_envelope_sha256);
    writer.string(command.launch.runtime_revision);
    writer.string(command.launch.backend);
    writer.scalar(command.launch.maximum_signal_rate_hz);
    writer.scalar(command.launch.inference_threads);
    writer.scalar(command.launch.exact_target_process_id);
    auto result = std::move(writer).take();
    if (result.size() > maximum_message_bytes) return std::nullopt;
    return result;
}

std::optional<ConfigureAdmittedLandmarkProviderCommandV1> decode_provider_configuration(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_message_bytes) return std::nullopt;
    Reader reader(bytes);
    ConfigureAdmittedLandmarkProviderCommandV1 value{};
    std::string root;
    std::string detector;
    std::string landmark;
    std::string runtime;
    std::string runtime_shared;
    if (!reader.scalar(value.launch.schema_version) ||
        !reader.string(value.launch.pack_id, 128U) ||
        !reader.string(value.launch.pack_revision, 128U) || !reader.string(root, 2048U) ||
        !reader.string(detector, 2048U) || !reader.string(landmark, 2048U) ||
        !reader.string(runtime, 2048U) || !reader.string(runtime_shared, 2048U) ||
        !reader.scalar(value.launch.detector_size_bytes) ||
        !reader.scalar(value.launch.landmark_size_bytes) ||
        !reader.scalar(value.launch.runtime_size_bytes) ||
        !reader.scalar(value.launch.runtime_shared_size_bytes) ||
        !reader.string(value.launch.detector_sha256, 64U) ||
        !reader.string(value.launch.landmark_sha256, 64U) ||
        !reader.string(value.launch.runtime_sha256, 64U) ||
        !reader.string(value.launch.runtime_shared_sha256, 64U) ||
        !reader.string(value.launch.measured_envelope_sha256, 64U) ||
        !reader.string(value.launch.runtime_revision, 64U) ||
        !reader.string(value.launch.backend, 128U) ||
        !reader.scalar(value.launch.maximum_signal_rate_hz) ||
        !reader.scalar(value.launch.inference_threads) ||
        !reader.scalar(value.launch.exact_target_process_id) || !reader.done()) {
        return std::nullopt;
    }
    value.launch.artifact_root = path_from_utf8(root);
    value.launch.detector_model = path_from_utf8(detector);
    value.launch.landmark_model = path_from_utf8(landmark);
    value.launch.runtime_library = path_from_utf8(runtime);
    value.launch.runtime_shared_library = path_from_utf8(runtime_shared);
    if (!validate_landmark_provider_launch_v1(value.launch)) return std::nullopt;
    return value;
}

std::optional<std::vector<std::byte>> encode_character_mouth_atlas(
    const InstallCharacterMouthAtlasCommandV1& command) {
    const auto& atlas = command.atlas;
    if (atlas.states.size() < 4U || atlas.states.size() > 16U) return std::nullopt;
    std::uint64_t total_pixels{};
    Writer writer;
    writer.scalar(atlas.schema_version);
    writer.scalar(atlas.cancellation_generation);
    writer.scalar(atlas.actor_id);
    writer.scalar(atlas.identity_revision);
    writer.scalar(static_cast<std::uint32_t>(atlas.states.size()));
    for (const auto& state : atlas.states) {
        const auto& appearance = state.appearance;
        if (appearance.premultiplied_bgra.size() > maximum_atlas_bytes) return std::nullopt;
        total_pixels += appearance.premultiplied_bgra.size();
        if (total_pixels > maximum_atlas_bytes) return std::nullopt;
        write_coefficients(writer, state.coefficients);
        writer.scalar(appearance.width);
        writer.scalar(appearance.height);
        writer.scalar(appearance.stride_bytes);
        write_pose(writer, appearance.enrolled_pose);
        writer.scalar(static_cast<std::uint32_t>(appearance.premultiplied_bgra.size()));
        writer.raw(std::as_bytes(std::span{
            appearance.premultiplied_bgra.data(), appearance.premultiplied_bgra.size()}));
    }
    auto result = std::move(writer).take();
    if (result.size() > maximum_message_bytes) return std::nullopt;
    return result;
}

std::optional<InstallCharacterMouthAtlasCommandV1> decode_character_mouth_atlas(
    const std::span<const std::byte> bytes) {
    if (bytes.empty() || bytes.size() > maximum_message_bytes) return std::nullopt;
    Reader reader(bytes);
    InstallCharacterMouthAtlasCommandV1 value{};
    std::uint32_t state_count{};
    if (!reader.scalar(value.atlas.schema_version) ||
        !reader.scalar(value.atlas.cancellation_generation) ||
        !reader.scalar(value.atlas.actor_id) ||
        !reader.scalar(value.atlas.identity_revision) ||
        !reader.scalar(state_count) || state_count < 4U || state_count > 16U) {
        return std::nullopt;
    }
    value.atlas.states.reserve(state_count);
    std::uint64_t total_pixels{};
    for (std::uint32_t index = 0U; index < state_count; ++index) {
        MouthAtlasState state{};
        std::vector<std::byte> pixels;
        if (!read_coefficients(reader, state.coefficients) ||
            !reader.scalar(state.appearance.width) ||
            !reader.scalar(state.appearance.height) ||
            !reader.scalar(state.appearance.stride_bytes) ||
            !read_pose(reader, state.appearance.enrolled_pose) ||
            !reader.vector(pixels, maximum_atlas_bytes)) {
            return std::nullopt;
        }
        total_pixels += pixels.size();
        if (total_pixels > maximum_atlas_bytes) return std::nullopt;
        state.appearance.premultiplied_bgra.resize(pixels.size());
        std::memcpy(state.appearance.premultiplied_bgra.data(), pixels.data(), pixels.size());
        value.atlas.states.push_back(std::move(state));
    }
    if (!reader.done()) return std::nullopt;
    return value;
}

std::vector<std::byte> encode_cancel_command(const CancelGenerationCommandV1& command) {
    Writer writer;
    writer.scalar(command.new_generation);
    return std::move(writer).take();
}

std::optional<CancelGenerationCommandV1> decode_cancel_command(
    const std::span<const std::byte> bytes) {
    Reader reader(bytes);
    CancelGenerationCommandV1 value{};
    if (!reader.scalar(value.new_generation) || !reader.done()) return std::nullopt;
    return value;
}

std::vector<std::byte> encode_acknowledgement(const AcknowledgeResidualCommandV1& command) {
    Writer writer;
    writer.scalar(command.lease_nonce_high);
    writer.scalar(command.lease_nonce_low);
    writer.boolean(command.presented);
    return std::move(writer).take();
}

std::optional<AcknowledgeResidualCommandV1> decode_acknowledgement(
    const std::span<const std::byte> bytes) {
    Reader reader(bytes);
    AcknowledgeResidualCommandV1 value{};
    if (!reader.scalar(value.lease_nonce_high) || !reader.scalar(value.lease_nonce_low) ||
        !reader.boolean(value.presented) || !reader.done()) {
        return std::nullopt;
    }
    return value;
}

std::vector<std::byte> frame_message(const std::span<const std::byte> message) {
    if (message.size() > maximum_message_bytes) return {};
    Writer writer;
    writer.scalar(static_cast<std::uint32_t>(message.size()));
    writer.raw(message);
    return std::move(writer).take();
}

std::optional<std::uint32_t> decode_frame_size(
    const std::span<const std::byte, 4> prefix) noexcept {
    std::uint32_t value{};
    for (std::size_t index = 0; index < prefix.size(); ++index) {
        value |= static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(prefix[index]))
                 << (index * 8U);
    }
    if (value == 0U || value > maximum_message_bytes) return std::nullopt;
    return value;
}

} // namespace npc::mouth::service
