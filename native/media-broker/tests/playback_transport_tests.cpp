#include "npc/media_broker/playback_transport.hpp"
#include "npc/media_broker/ipc.hpp"

#include <array>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <span>
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
    envelope.active = true;
    const auto wire = npc::media::ipc::encode_visual_audio_envelope(envelope);
    assert(wire && wire->size() < 1024);
    const auto decoded = npc::media::ipc::decode_visual_audio_envelope(*wire);
    assert(decoded && decoded->stream_id == envelope.stream_id &&
           decoded->source_sample_start == envelope.source_sample_start &&
           decoded->mono_rms_q15 == envelope.mono_rms_q15 && decoded->active &&
           !decoded->draining && !decoded->cancelled);

    envelope.cancelled = true;
    assert(!npc::media::ipc::encode_visual_audio_envelope(envelope));
}

} // namespace

int main() {
    allocation_policy_is_bounded();
    wire_round_trip_and_size_rejection();
    authentication_order_budget_cancel_and_drain();
    rejects_deadlines_backpressure_and_budget_overrun();
    allocation_expiry_only_gates_begin_but_connected_stream_has_total_deadline();
    response_receipt_round_trip();
    authenticated_control_allocation_codec_is_bounded_and_external_only();
    audio_output_control_codec_binds_stable_endpoint_generation();
    visual_audio_envelope_codec_is_exact_bounded_and_causal();
    std::cout << "playback transport tests passed\n";
}
