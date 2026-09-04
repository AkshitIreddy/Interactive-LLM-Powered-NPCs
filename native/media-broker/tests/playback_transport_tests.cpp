#include "npc/media_broker/playback_transport.hpp"
#include "npc/media_broker/ipc.hpp"

#include <array>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <limits>
#include <span>
#include <type_traits>
#include <vector>

namespace {

using namespace npc::media::playback;

AuthenticationToken token(const unsigned seed = 1) {
    AuthenticationToken value{};
    for (std::size_t index = 0; index < value.size(); ++index) {
        value[index] = static_cast<std::byte>((index + seed) & 0xffU);
    }
    return value;
}

PlaybackLease lease() {
    return {schema_version, "stream-001", R"(\\.\pipe\npc-playback-stream-001)", token(),
            "session-001", "turn-001", 7, 24'000, 1, 48'000,
            maximum_chunk_bytes, 20'000, AudioOutputSelectionMode::system_default,
            "{0.0.0.00000000}.fixture-output", 17};
}

ProducerEnvelope envelope(const ProducerCommand command, const std::uint64_t sequence,
                          std::vector<std::byte> payload = {}) {
    return {schema_version, command, sequence, 11'000, 7, "stream-001", "session-001",
            "turn-001", token(), std::move(payload)};
}

template <typename T>
void append_little(std::vector<std::byte>& output, const T value) {
    static_assert(std::is_unsigned_v<T>);
    for (unsigned shift = 0; shift < sizeof(T) * 8; shift += 8) {
        output.push_back(static_cast<std::byte>((value >> shift) & 0xffU));
    }
}

std::vector<std::byte> visual_cue_payload(
    const std::uint64_t start_sample = 240,
    const std::uint64_t duration_samples = 120,
    const std::uint8_t canonical_viseme = 3,
    const std::uint16_t strength_q15 = 24'000,
    const std::uint32_t schema = visual_speech_cue_schema_version,
    const std::uint8_t reserved = 0) {
    std::vector<std::byte> payload;
    payload.reserve(visual_speech_cue_payload_bytes);
    append_little(payload, schema);
    append_little(payload, start_sample);
    append_little(payload, duration_samples);
    payload.push_back(static_cast<std::byte>(canonical_viseme));
    payload.push_back(static_cast<std::byte>(reserved));
    append_little(payload, strength_q15);
    return payload;
}

void append_varint(std::vector<std::byte>& output, std::uint64_t value) {
    while (value >= 0x80U) {
        output.push_back(static_cast<std::byte>((value & 0x7fU) | 0x80U));
        value >>= 7U;
    }
    output.push_back(static_cast<std::byte>(value));
}

void append_nested_visual_cue(std::vector<std::byte>& output,
                              const std::uint64_t start,
                              const std::uint64_t duration,
                              const std::uint32_t viseme,
                              const std::uint32_t strength) {
    std::vector<std::byte> nested;
    append_varint(nested, 1U << 3U);
    append_varint(nested, start);
    append_varint(nested, 2U << 3U);
    append_varint(nested, duration);
    append_varint(nested, 3U << 3U);
    append_varint(nested, viseme);
    append_varint(nested, 4U << 3U);
    append_varint(nested, strength);
    append_varint(output, (20U << 3U) | 2U);
    append_varint(output, nested.size());
    output.insert(output.end(), nested.begin(), nested.end());
}

void allocation_policy_is_bounded() {
    AllocationRequest valid{"session-001", "turn-001", 7, 24'000, 1, 48'000, 42};
    assert(valid_allocation(valid));
    valid.expected_producer_process_id = 0;
    assert(!valid_allocation(valid));
    valid.expected_producer_process_id = 42;
    valid.max_frames = maximum_source_frames + 1;
    assert(!valid_allocation(valid));
    valid.max_frames = 48'000;
    valid.channels = 3;
    assert(!valid_allocation(valid));
}

void wire_round_trip_and_size_rejection() {
    auto value = envelope(ProducerCommand::chunk, 2, std::vector<std::byte>(4096, std::byte{0x2a}));
    const auto encoded = encode_envelope(value);
    assert(encoded && encoded->size() > value.payload.size());
    std::array<std::byte, 4> prefix{};
    std::copy_n(encoded->begin(), 4, prefix.begin());
    const auto body_size = decode_frame_size(prefix);
    assert(body_size && *body_size == encoded->size() - 4);
    const auto decoded = decode_envelope(std::span{*encoded}.subspan(4));
    assert(decoded && decoded->command == value.command && decoded->sequence == value.sequence);
    assert(decoded->payload == value.payload && decoded->stream_id == value.stream_id);

    value.payload.resize(maximum_chunk_bytes + 1);
    assert(!encode_envelope(value));
    prefix.fill(std::byte{0xff});
    assert(!decode_frame_size(prefix));
}

void visual_cue_wire_and_session_contract_is_strict_and_zero_frame() {
    static_assert(static_cast<std::uint16_t>(ProducerCommand::visual_cue) == 5);
    static_assert(visual_speech_cue_payload_bytes == 24);
    auto cue = envelope(ProducerCommand::visual_cue, 2, visual_cue_payload());
    const auto encoded = encode_envelope(cue);
    assert(encoded);
    const auto decoded = decode_envelope(std::span{*encoded}.subspan(4));
    assert(decoded && decoded->command == ProducerCommand::visual_cue &&
           decoded->payload == cue.payload);
    const auto decoded_cue = decode_visual_speech_cue_payload(cue.payload, 48'000);
    assert(decoded_cue && decoded_cue->start_sample == 240 &&
           decoded_cue->duration_samples == 120);
    auto malformed_wire = *encoded;
    malformed_wire[malformed_wire.size() - visual_speech_cue_payload_bytes] = std::byte{2};
    assert(!decode_envelope(std::span{malformed_wire}.subspan(4)));

    PlaybackSession before_begin(lease(), 42, 24'000, {1'000, 5'000});
    assert(before_begin.process(cue, 42, 10'000).status == ProducerStatus::invalid_state);

    PlaybackSession session(lease(), 42, 24'000, {1'000, 5'000});
    assert(session.process(envelope(ProducerCommand::begin, 1), 42, 10'000).status ==
           ProducerStatus::ok);
    const auto accepted = session.process(cue, 42, 10'000);
    assert(accepted.status == ProducerStatus::ok && accepted.accepted_source_frames == 0);
    assert(session.source_frames() == 0 && session.visual_speech_cues().size() == 1);
    assert(session.visual_speech_cues()[0].start_sample == 240 &&
           session.visual_speech_cues()[0].duration_samples == 120 &&
           session.visual_speech_cues()[0].canonical_viseme == 3 &&
           session.visual_speech_cues()[0].strength_q15 == 24'000);

    std::uint64_t sequence = 3;
    auto rejected = [&](std::vector<std::byte> payload) {
        const auto response = session.process(
            envelope(ProducerCommand::visual_cue, sequence++, std::move(payload)), 42, 10'000);
        assert(response.status == ProducerStatus::invalid_frame);
    };
    rejected(std::vector<std::byte>(visual_speech_cue_payload_bytes - 1, std::byte{}));
    rejected(visual_cue_payload(360, 120, 3, 24'000, 2));
    rejected(visual_cue_payload(360, 120, 3, 24'000, 1, 1));
    rejected(visual_cue_payload(360, 0));
    rejected(visual_cue_payload(360, 120, 11));
    rejected(visual_cue_payload(360, 120, 3, 32'768));
    rejected(visual_cue_payload(47'950, 51));
    rejected(visual_cue_payload(std::numeric_limits<std::uint64_t>::max(), 2));
    rejected(visual_cue_payload(300, 120)); // overlaps the accepted [240, 360) cue
    const auto cancelled = session.process(
        envelope(ProducerCommand::cancel, sequence++), 42, 10'000);
    assert(cancelled.status == ProducerStatus::cancelled &&
           session.visual_speech_cues().empty());

    PlaybackSession bounded(lease(), 42, 24'000, {1'000, 5'000});
    assert(bounded.process(envelope(ProducerCommand::begin, 1), 42, 10'000).status ==
           ProducerStatus::ok);
    for (std::size_t index = 0; index < maximum_visual_speech_cues; ++index) {
        const auto start = static_cast<std::uint64_t>(index) * 10U;
        const auto response = bounded.process(
            envelope(ProducerCommand::visual_cue, index + 2,
                     visual_cue_payload(start, 10, static_cast<std::uint8_t>(index % 11U))),
            42, 10'000);
        assert(response.status == ProducerStatus::ok);
    }
    assert(bounded.visual_speech_cues().size() == maximum_visual_speech_cues);
    assert(bounded.process(
               envelope(ProducerCommand::visual_cue, maximum_visual_speech_cues + 2,
                        visual_cue_payload(maximum_visual_speech_cues * 10U, 10)),
               42, 10'000).status == ProducerStatus::invalid_frame);
}

void authentication_order_budget_cancel_and_drain() {
    PlaybackSession session(lease(), 42, 24'000, {1'000, 5'000});
    auto begin = envelope(ProducerCommand::begin, 1);
    auto wrong = begin;
    wrong.token[0] ^= std::byte{0xff};
    assert(session.process(wrong, 42, 10'000).status == ProducerStatus::authentication_failed);
    assert(session.process(begin, 41, 10'000).status == ProducerStatus::producer_mismatch);
    assert(session.process(begin, 42, 10'000).status == ProducerStatus::ok);
    assert(session.state() == SessionState::streaming);
    assert(session.process(begin, 42, 10'000).status == ProducerStatus::sequence_replayed);

    auto chunk = envelope(ProducerCommand::chunk, 2, std::vector<std::byte>(4'800, std::byte{0x01}));
    const auto accepted = session.process(chunk, 42, 10'000);
    assert(accepted.status == ProducerStatus::ok && accepted.accepted_source_frames == 2'400);
    std::vector<std::byte> output(4'800);
    const auto transfer = session.ring().read(output, 2'400);
    assert(transfer.transferred_frames == 2'400);
    session.record_device_frames(2'400);

    auto finish = envelope(ProducerCommand::finish, 3);
    assert(session.process(finish, 42, 10'000).status == ProducerStatus::ok);
    assert(session.state() == SessionState::source_finished);
    session.mark_device_drained();
    assert(session.state() == SessionState::drained);
    const auto receipt = session.receipt();
    assert(receipt.source_frames == 2'400 && receipt.device_frames == 2'400);
    assert(receipt.source_duration_micros == 100'000);
    assert(receipt.source_submission_complete && receipt.endpoint_drain_complete && !receipt.cancelled);

    PlaybackSession cancelled(lease(), 42, 24'000, {1'000, 5'000});
    assert(cancelled.process(envelope(ProducerCommand::begin, 1), 42, 10'000).status == ProducerStatus::ok);
    const auto cancel_response = cancelled.process(envelope(ProducerCommand::cancel, 2), 42, 10'000);
    assert(cancel_response.status == ProducerStatus::cancelled && cancel_response.receipt);
    assert(cancel_response.receipt->cancelled && !cancel_response.receipt->endpoint_drain_complete);
}

void rejects_deadlines_backpressure_and_budget_overrun() {
    PlaybackSession session(lease(), 42, 1'000, {1'000, 5'000});
    auto future = envelope(ProducerCommand::begin, 1);
    future.deadline_qpc = 20'001;
    assert(session.process(future, 42, 10'000).status == ProducerStatus::deadline_too_far);
    auto expired = envelope(ProducerCommand::begin, 2);
    expired.deadline_qpc = 9'999;
    assert(session.process(expired, 42, 10'000).status == ProducerStatus::deadline_expired);
    assert(session.process(envelope(ProducerCommand::begin, 3), 42, 10'000).status == ProducerStatus::ok);
    auto too_much_for_ring = envelope(ProducerCommand::chunk, 4,
        std::vector<std::byte>(2'002, std::byte{}));
    assert(session.process(too_much_for_ring, 42, 10'000).status == ProducerStatus::backpressure);

    auto oversized = lease();
    oversized.max_frames = 10;
    PlaybackSession budget(std::move(oversized), 42, 100, {1'000, 5'000});
    assert(budget.process(envelope(ProducerCommand::begin, 1), 42, 10'000).status == ProducerStatus::ok);
    auto chunk = envelope(ProducerCommand::chunk, 2, std::vector<std::byte>(22, std::byte{}));
    assert(budget.process(chunk, 42, 10'000).status == ProducerStatus::frame_budget_exceeded);
}

void allocation_expiry_only_gates_begin_but_connected_stream_has_total_deadline() {
    auto expiring = lease();
    expiring.expires_qpc = 10'050;
    PlaybackSession session(std::move(expiring), 42, 1'000, {1'000, 5'000});
    assert(session.process(envelope(ProducerCommand::begin, 1), 42, 10'000).status ==
           ProducerStatus::ok);
    auto after_allocation_expiry = envelope(ProducerCommand::chunk, 2,
                                             std::vector<std::byte>(20, std::byte{}));
    after_allocation_expiry.deadline_qpc = 11'500;
    assert(session.process(after_allocation_expiry, 42, 11'000).status == ProducerStatus::ok);
    auto beyond_total = envelope(ProducerCommand::chunk, 3,
                                  std::vector<std::byte>(20, std::byte{}));
    beyond_total.deadline_qpc = 135'001;
    assert(session.process(beyond_total, 42, 130'001).status == ProducerStatus::deadline_expired);
}

void response_receipt_round_trip() {
    const PlaybackReceipt receipt{schema_version, "receipt-stream-001", "stream-001",
        "session-001", "turn-001", 7, 2'400, 2'400, 100'000, true, true, false};
    auto endpoint_receipt = receipt;
    endpoint_receipt.output_selection_mode = AudioOutputSelectionMode::system_default;
    endpoint_receipt.output_endpoint_id = "{0.0.0.00000000}.fixture-output";
    endpoint_receipt.output_endpoint_generation = 17;
    ProducerResponse response{schema_version, 8, ProducerStatus::ok, 0, endpoint_receipt};
    const auto encoded = encode_response(response);
    assert(encoded);
    const auto decoded = decode_response(std::span{*encoded}.subspan(4));
    assert(decoded && decoded->receipt && decoded->receipt->receipt_id == receipt.receipt_id);
    assert(decoded->receipt->endpoint_drain_complete && !decoded->receipt->cancelled);
    assert(decoded->receipt->output_endpoint_id == endpoint_receipt.output_endpoint_id);
    assert(decoded->receipt->output_endpoint_generation == 17);

    auto legacy = envelope(ProducerCommand::begin, 9);
    legacy.schema_version = 1;
    assert(!encode_envelope(legacy));
}

void authenticated_control_allocation_codec_is_bounded_and_external_only() {
    const npc::media::ipc::AllocatePlaybackStreamCommand allocation{
        "session-001", "turn-001", 7, 24'000, 1, 48'000, 42};
    const auto encoded = npc::media::ipc::encode_command(
        npc::media::ipc::CommandKind::allocate_playback_stream, allocation);
    assert(encoded);
    const auto decoded = npc::media::ipc::decode_command(
        npc::media::ipc::CommandKind::allocate_playback_stream, *encoded);
    assert(decoded);
    const auto* value = std::get_if<npc::media::ipc::AllocatePlaybackStreamCommand>(&*decoded);
    (void)value;
    assert(value && value->session_id == allocation.session_id &&
           value->turn_id == allocation.turn_id && value->generation == allocation.generation &&
           value->expected_producer_process_id == allocation.expected_producer_process_id);
    assert(npc::media::ipc::target_process_effect(
               npc::media::ipc::CommandKind::allocate_playback_stream) ==
           npc::media::ipc::TargetProcessEffect::none);

    auto response_lease = lease();
    const auto wire = npc::media::ipc::encode_playback_lease(response_lease);
    assert(wire && wire->size() < npc::media::ipc::maximum_frame_bytes);
    const auto decoded_lease = npc::media::ipc::decode_playback_lease(*wire);
    assert(decoded_lease && decoded_lease->stream_id == response_lease.stream_id &&
           decoded_lease->producer_endpoint == response_lease.producer_endpoint &&
           decoded_lease->one_time_token == response_lease.one_time_token &&
           decoded_lease->max_chunk_bytes == maximum_chunk_bytes);
}

void audio_output_control_codec_binds_stable_endpoint_generation() {
    const npc::media::ipc::SelectAudioOutputCommand select{
        AudioOutputSelectionMode::endpoint_id,
        "{0.0.0.00000000}.{fixture-output}"};
    const auto command = npc::media::ipc::encode_command(
        npc::media::ipc::CommandKind::select_audio_output, select);
    assert(command);
    const auto decoded_command = npc::media::ipc::decode_command(
        npc::media::ipc::CommandKind::select_audio_output, *command);
    const auto* decoded_select = decoded_command
        ? std::get_if<npc::media::ipc::SelectAudioOutputCommand>(&*decoded_command)
        : nullptr;
    (void)decoded_select;
    assert(decoded_select && decoded_select->mode == select.mode &&
           decoded_select->endpoint_id == select.endpoint_id);

    const npc::media::ipc::AudioOutputSnapshot snapshot{
        1, 77,
        {{select.endpoint_id, "Fixture Speakers", npc::media::ipc::AudioOutputState::active,
          true, 17}}};
    const auto snapshot_wire = npc::media::ipc::encode_audio_output_snapshot(snapshot);
    assert(snapshot_wire);
    const auto decoded_snapshot = npc::media::ipc::decode_audio_output_snapshot(*snapshot_wire);
    assert(decoded_snapshot && decoded_snapshot->catalog_generation == 77 &&
           decoded_snapshot->endpoints.size() == 1 &&
           decoded_snapshot->endpoints[0].endpoint_id == select.endpoint_id &&
           decoded_snapshot->endpoints[0].generation == 17);

    const npc::media::ipc::SelectedAudioOutput selected{
        1, AudioOutputSelectionMode::endpoint_id, select.endpoint_id,
        snapshot.endpoints[0]};
    const auto selected_wire = npc::media::ipc::encode_selected_audio_output(selected);
    assert(selected_wire);
    const auto selected_round_trip = npc::media::ipc::decode_selected_audio_output(*selected_wire);
    assert(selected_round_trip && selected_round_trip->requested_endpoint_id == select.endpoint_id &&
           selected_round_trip->resolved.generation == 17);
}

void visual_audio_envelope_codec_is_exact_bounded_and_causal() {
    const npc::media::ipc::QueryVisualAudioEnvelopeCommand query{
        "session-001", "turn-001", 7, "stream-001", "stream-001"};
    const auto query_wire = npc::media::ipc::encode_command(
        npc::media::ipc::CommandKind::query_visual_audio_envelope, query);
    assert(query_wire);
    const auto decoded_query = npc::media::ipc::decode_command(
        npc::media::ipc::CommandKind::query_visual_audio_envelope, *query_wire);
    const auto* exact = decoded_query
        ? std::get_if<npc::media::ipc::QueryVisualAudioEnvelopeCommand>(&*decoded_query)
        : nullptr;
    assert(exact && exact->session_id == query.session_id && exact->turn_id == query.turn_id &&
           exact->generation == query.generation && exact->stream_id == query.stream_id &&
           exact->segment_id == query.segment_id);

    npc::media::ipc::VisualAudioEnvelope envelope;
    envelope.session_id = query.session_id;
    envelope.turn_id = query.turn_id;
    envelope.generation = query.generation;
    envelope.stream_id = query.stream_id;
    envelope.segment_id = query.segment_id;
    envelope.source_sample_start = 240;
    envelope.source_sample_count = 240;
    envelope.sample_rate = 24'000;
    envelope.channels = 1;
    envelope.device_write_qpc = 500;
    envelope.qpc_frequency = 10'000'000;
    envelope.source_frames = 480;
    envelope.device_frames = 480;
    envelope.mono_rms_q15 = {1, 2, 3, 4, 5, 6, 7, 8};
    envelope.mono_peak_q15 = {8, 7, 6, 5, 4, 3, 2, 1};
    envelope.visual_speech_cues = {
        {0, 120, 1, 20'000},
        {120, 240, 7, 32'767},
    };
    envelope.active = true;
    const auto wire = npc::media::ipc::encode_visual_audio_envelope(envelope);
    assert(wire && wire->size() < 1024);
    const auto decoded = npc::media::ipc::decode_visual_audio_envelope(*wire);
    assert(decoded && decoded->stream_id == envelope.stream_id &&
           decoded->source_sample_start == envelope.source_sample_start &&
           decoded->mono_rms_q15 == envelope.mono_rms_q15 && decoded->active &&
           !decoded->draining && !decoded->cancelled &&
           decoded->visual_speech_cues == envelope.visual_speech_cues);

    auto legacy = envelope;
    legacy.visual_speech_cues.clear();
    const auto legacy_wire = npc::media::ipc::encode_visual_audio_envelope(legacy);
    const auto legacy_round_trip = legacy_wire
        ? npc::media::ipc::decode_visual_audio_envelope(*legacy_wire) : std::nullopt;
    assert(legacy_round_trip && legacy_round_trip->visual_speech_cues.empty());

    auto invalid = envelope;
    invalid.visual_speech_cues[1].start_sample = 119;
    assert(!npc::media::ipc::encode_visual_audio_envelope(invalid));
    invalid = envelope;
    invalid.visual_speech_cues[0].duration_samples = 0;
    assert(!npc::media::ipc::encode_visual_audio_envelope(invalid));
    invalid = envelope;
    invalid.visual_speech_cues[0].canonical_viseme = 11;
    assert(!npc::media::ipc::encode_visual_audio_envelope(invalid));
    invalid = envelope;
    invalid.visual_speech_cues.resize(maximum_visual_speech_cues + 1,
                                      {1'000, 1, 1, 1});
    assert(!npc::media::ipc::encode_visual_audio_envelope(invalid));

    auto malformed_nested = *legacy_wire;
    append_nested_visual_cue(malformed_nested, 1, 0, 1, 1);
    assert(!npc::media::ipc::decode_visual_audio_envelope(malformed_nested));
    auto out_of_order_nested = *legacy_wire;
    append_nested_visual_cue(out_of_order_nested, 100, 20, 1, 1);
    append_nested_visual_cue(out_of_order_nested, 99, 1, 2, 2);
    assert(!npc::media::ipc::decode_visual_audio_envelope(out_of_order_nested));

    envelope.cancelled = true;
    assert(!npc::media::ipc::encode_visual_audio_envelope(envelope));
}

} // namespace

int main() {
    allocation_policy_is_bounded();
    wire_round_trip_and_size_rejection();
    visual_cue_wire_and_session_contract_is_strict_and_zero_frame();
    authentication_order_budget_cancel_and_drain();
    rejects_deadlines_backpressure_and_budget_overrun();
    allocation_expiry_only_gates_begin_but_connected_stream_has_total_deadline();
    response_receipt_round_trip();
    authenticated_control_allocation_codec_is_bounded_and_external_only();
    audio_output_control_codec_binds_stable_endpoint_generation();
    visual_audio_envelope_codec_is_exact_bounded_and_causal();
    std::cout << "playback transport tests passed\n";
}
