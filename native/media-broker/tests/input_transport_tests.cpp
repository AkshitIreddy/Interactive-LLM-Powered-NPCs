#include "npc/media_broker/input_transport.hpp"
#include "npc/media_broker/ipc.hpp"

#include <cstddef>
#include <cstdint>
#include <iostream>
#include <stdexcept>
#include <variant>
#include <vector>

namespace {

using namespace npc::media;

void require(const bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

playback::AuthenticationToken token() {
    playback::AuthenticationToken value{};
    for (std::size_t index = 0; index < value.size(); ++index) {
        value[index] = static_cast<std::byte>(index + 1U);
    }
    return value;
}

input::RehearsalLease lease() {
    return {input::schema_version,
            "mic-stream-001",
            R"(\\.\pipe\npc-media-input-fixture)",
            token(),
            "session-001",
            "turn-001",
            7,
            500,
            16'000,
            1,
            8'000,
            input::maximum_chunk_bytes,
            20'000,
            10'000'000,
            playback::AudioOutputSelectionMode::endpoint_id,
            R"({0.0.1.00000000}.{fixture-input})",
            17};
}

void hello_chunk_ack_and_receipt_are_strict_and_bounded() {
    const input::StreamHello hello{input::schema_version, "mic-stream-001",
        "session-001", "turn-001", 7, 16'000, 1, 8'000,
        input::maximum_chunk_bytes, 10'000'000};
    const auto hello_wire = input::encode_hello(hello);
    require(hello_wire.has_value(), "encode hello");
    const auto hello_round_trip = input::decode_hello(*hello_wire);
    require(hello_round_trip && hello_round_trip->turn_id == hello.turn_id &&
                hello_round_trip->qpc_frequency == hello.qpc_frequency,
            "decode exact hello");
    auto malformed_hello = *hello_wire;
    malformed_hello.push_back(std::byte{});
    require(!input::decode_hello(malformed_hello), "reject trailing hello byte");

    input::PcmChunk chunk{input::schema_version, 1, 30'000, 0, 32'768,
                          std::vector<std::byte>(input::maximum_chunk_bytes,
                                                 std::byte{0x2a})};
    const auto chunk_wire = input::encode_chunk(chunk, 1);
    require(chunk_wire.has_value(), "encode 64KiB PCM chunk");
    const auto chunk_round_trip = input::decode_chunk(*chunk_wire, 1);
    require(chunk_round_trip && chunk_round_trip->sequence == 1 &&
                chunk_round_trip->first_frame_qpc == 30'000 &&
                chunk_round_trip->first_frame_index == 0 &&
                chunk_round_trip->pcm_s16le.size() == input::maximum_chunk_bytes,
            "decode exact PCM chunk");
    chunk.pcm_s16le.push_back(std::byte{});
    require(!input::encode_chunk(chunk, 1), "reject oversized PCM chunk");

    const input::ConsumerAck ack{input::schema_version, 1,
                                 input::AckAction::continue_stream};
    const auto ack_wire = input::encode_ack(ack);
    require(ack_wire.has_value(), "encode ack");
    const auto ack_round_trip = input::decode_ack(*ack_wire);
    require(ack_round_trip && ack_round_trip->sequence == 1 &&
                ack_round_trip->action == input::AckAction::continue_stream,
            "decode ack");
    auto replay = *ack_wire;
    replay[8] = std::byte{};
    replay[9] = std::byte{};
    replay[10] = std::byte{};
    replay[11] = std::byte{};
    replay[12] = std::byte{};
    replay[13] = std::byte{};
    replay[14] = std::byte{};
    replay[15] = std::byte{};
    require(!input::decode_ack(replay), "reject zero/replayed ack sequence");

    const input::RehearsalReceipt receipt{input::schema_version,
        "mic-receipt-001", "mic-stream-001", "session-001", "turn-001", 7,
        playback::AudioOutputSelectionMode::endpoint_id,
        R"({0.0.1.00000000}.{fixture-input})", 17, 16'000, 1, 8'000,
        500'000, -2'000, -18'000, 2, 2'400, true, false, true, false, false};
    const auto receipt_wire = input::encode_receipt(receipt);
    require(receipt_wire.has_value(), "encode input receipt");
    const auto receipt_round_trip = input::decode_receipt(*receipt_wire);
    require(receipt_round_trip && receipt_round_trip->receipt_id == receipt.receipt_id &&
                receipt_round_trip->input_endpoint_generation == 17 &&
                receipt_round_trip->captured_frames == 8'000 &&
                receipt_round_trip->source_capture_complete &&
                receipt_round_trip->clipping_detected,
            "decode exact input receipt");
    auto cancelled = receipt;
    cancelled.source_capture_complete = false;
    cancelled.cancelled = true;
    require(input::encode_receipt(cancelled).has_value(), "encode cancelled receipt");
    cancelled.source_capture_complete = true;
    require(!input::encode_receipt(cancelled), "reject completed cancelled receipt");

    auto ptt_receipt = receipt;
    ptt_receipt.activation_source = input::ActivationSource::push_to_talk;
    ptt_receipt.ptt_virtual_key = 0x77;
    ptt_receipt.ptt_press_transition_sequence = 8;
    ptt_receipt.ptt_pressed_qpc = 30'000;
    ptt_receipt.ptt_release_transition_sequence = 9;
    ptt_receipt.ptt_released_qpc = 35'000;
    const auto ptt_wire = input::encode_receipt(ptt_receipt);
    require(ptt_wire.has_value(), "encode broker-attested PTT receipt");
    const auto ptt_round_trip = input::decode_receipt(*ptt_wire);
    require(ptt_round_trip &&
                ptt_round_trip->activation_source == input::ActivationSource::push_to_talk &&
                ptt_round_trip->ptt_press_transition_sequence == 8 &&
                ptt_round_trip->ptt_release_transition_sequence == 9,
            "decode exact PTT press/release provenance");
    ptt_receipt.ptt_release_transition_sequence = 8;
    require(!input::encode_receipt(ptt_receipt), "reject nonmonotonic PTT release");
}

void authenticated_control_codecs_preserve_device_and_consumer_binding() {
    const ipc::SelectAudioInputCommand select{
        playback::AudioOutputSelectionMode::endpoint_id,
        R"({0.0.1.00000000}.{fixture-input})"};
    const auto select_wire = ipc::encode_command(ipc::CommandKind::select_audio_input, select);
    require(select_wire.has_value(), "encode input selection command");
    const auto select_round_trip = ipc::decode_command(
        ipc::CommandKind::select_audio_input, *select_wire);
    const auto* decoded_select = select_round_trip
        ? std::get_if<ipc::SelectAudioInputCommand>(&*select_round_trip) : nullptr;
    require(decoded_select && decoded_select->mode == select.mode &&
                decoded_select->endpoint_id == select.endpoint_id,
            "decode exact input selection command");

    const ipc::AudioInputSnapshot snapshot{1, 99,
        {{select.endpoint_id, "Fixture Microphone", ipc::AudioInputState::active, true, 17}}};
    const auto snapshot_wire = ipc::encode_audio_input_snapshot(snapshot);
    require(snapshot_wire.has_value(), "encode input snapshot");
    const auto snapshot_round_trip = ipc::decode_audio_input_snapshot(*snapshot_wire);
    require(snapshot_round_trip && snapshot_round_trip->catalog_generation == 99 &&
                snapshot_round_trip->endpoints.size() == 1 &&
                snapshot_round_trip->endpoints[0].endpoint_id == select.endpoint_id,
            "decode input snapshot");

    const ipc::SelectedAudioInput selected{1, select.mode, select.endpoint_id,
                                           snapshot.endpoints.front()};
    const auto selected_wire = ipc::encode_selected_audio_input(selected);
    require(selected_wire.has_value(), "encode selected input");
    const auto selected_round_trip = ipc::decode_selected_audio_input(*selected_wire);
    require(selected_round_trip && selected_round_trip->resolved.generation == 17,
            "decode selected input generation");

    const ipc::AllocateAudioInputRehearsalCommand allocate{
        "session-001", "turn-001", 7, 500, 16'000, 1, 8'000, 4242,
        input::ActivationSource::push_to_talk};
    const auto allocate_wire = ipc::encode_command(
        ipc::CommandKind::allocate_audio_input_rehearsal, allocate);
    require(allocate_wire.has_value(), "encode input allocation command");
    const auto allocate_round_trip = ipc::decode_command(
        ipc::CommandKind::allocate_audio_input_rehearsal, *allocate_wire);
    const auto* decoded_allocate = allocate_round_trip
        ? std::get_if<ipc::AllocateAudioInputRehearsalCommand>(&*allocate_round_trip) : nullptr;
    require(decoded_allocate && decoded_allocate->turn_id == allocate.turn_id &&
                decoded_allocate->sample_rate == allocate.sample_rate &&
                decoded_allocate->expected_producer_process_id == 4242 &&
                decoded_allocate->activation_source == input::ActivationSource::push_to_talk,
            "decode consumer-bound input allocation");

    const ipc::CancelAudioInputRehearsalCommand cancel{"mic-stream-001", 7};
    const auto cancel_wire = ipc::encode_command(
        ipc::CommandKind::cancel_audio_input_rehearsal, cancel);
    require(cancel_wire.has_value(), "encode exact input cancellation");
    const auto cancel_round_trip = ipc::decode_command(
        ipc::CommandKind::cancel_audio_input_rehearsal, *cancel_wire);
    const auto* decoded_cancel = cancel_round_trip
        ? std::get_if<ipc::CancelAudioInputRehearsalCommand>(&*cancel_round_trip) : nullptr;
    require(decoded_cancel && decoded_cancel->stream_id == cancel.stream_id &&
                decoded_cancel->generation == cancel.generation,
            "decode exact input cancellation");

    auto allocated_lease = lease();
    const auto lease_wire = ipc::encode_input_rehearsal_lease(allocated_lease);
    require(lease_wire.has_value(), "encode input lease");
    const auto lease_round_trip = ipc::decode_input_rehearsal_lease(*lease_wire);
    require(lease_round_trip && lease_round_trip->stream_id == allocated_lease.stream_id &&
                lease_round_trip->turn_id == allocated_lease.turn_id &&
                lease_round_trip->one_time_token == allocated_lease.one_time_token &&
                lease_round_trip->input_endpoint_id == allocated_lease.input_endpoint_id &&
                lease_round_trip->input_endpoint_generation == 17,
            "decode exact input lease");
    auto malformed_lease = *lease_wire;
    malformed_lease.push_back(std::byte{});
    require(!ipc::decode_input_rehearsal_lease(malformed_lease),
            "reject trailing input lease bytes");

    const ipc::QueryPttActivationStateCommand ptt_query;
    const auto ptt_query_wire = ipc::encode_command(
        ipc::CommandKind::query_ptt_activation_state, ptt_query);
    require(ptt_query_wire && ptt_query_wire->empty(), "encode empty native PTT query");
    const auto ptt_query_round_trip = ipc::decode_command(
        ipc::CommandKind::query_ptt_activation_state, *ptt_query_wire);
    require(ptt_query_round_trip &&
                std::holds_alternative<ipc::QueryPttActivationStateCommand>(*ptt_query_round_trip),
            "decode empty native PTT query");
    const ipc::PttActivationState released{
        1, 0x77, PttState::released, 4, 40'000, 4, 40'000};
    const auto released_wire = ipc::encode_ptt_activation_state(released);
    require(released_wire.has_value(), "encode released PTT baseline");
    const auto released_round_trip = ipc::decode_ptt_activation_state(*released_wire);
    require(released_round_trip && released_round_trip->virtual_key == 0x77 &&
                released_round_trip->state == PttState::released &&
                released_round_trip->transition_sequence == 4,
            "decode released PTT baseline");
    const ipc::PttActivationState pressed{
        1, 0x77, PttState::pressed, 5, 50'000, 4, 40'000};
    const auto pressed_wire = ipc::encode_ptt_activation_state(pressed);
    require(pressed_wire && ipc::decode_ptt_activation_state(*pressed_wire),
            "encode newer pressed PTT transition");
    auto incoherent = pressed;
    incoherent.release_transition_sequence = incoherent.transition_sequence;
    require(!ipc::encode_ptt_activation_state(incoherent),
            "reject incoherent PTT press/release provenance");

    static_assert(ipc::command_ids_are_unique());
    static_assert(static_cast<std::uint32_t>(ipc::CommandKind::enumerate_audio_inputs) == 24);
    static_assert(static_cast<std::uint32_t>(ipc::CommandKind::cancel_audio_input_rehearsal) == 28);
    static_assert(static_cast<std::uint32_t>(ipc::CommandKind::query_ptt_activation_state) == 30);
}

} // namespace

int main() {
    hello_chunk_ack_and_receipt_are_strict_and_bounded();
    authenticated_control_codecs_preserve_device_and_consumer_binding();
    std::cout << "input transport tests passed\n";
}
