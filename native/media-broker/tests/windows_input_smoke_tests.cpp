#ifdef _WIN32

#include "windows_input.hpp"

#include <Windows.h>

#include <array>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <functional>
#include <stdexcept>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace {

using namespace npc::media;

void require(const bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

std::wstring wide(const std::string& value) {
    if (value.empty()) return {};
    const auto size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                          static_cast<int>(value.size()), nullptr, 0);
    require(size > 0, "convert pipe endpoint to UTF-16");
    std::wstring result(static_cast<std::size_t>(size), L'\0');
    require(MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                static_cast<int>(value.size()), result.data(), size) == size,
            "convert complete pipe endpoint to UTF-16");
    return result;
}

void write_exact(const HANDLE pipe, const std::span<const std::byte> bytes) {
    std::size_t position{};
    while (position < bytes.size()) {
        DWORD written{};
        require(WriteFile(pipe, bytes.data() + position,
                          static_cast<DWORD>(bytes.size() - position), &written, nullptr) != FALSE &&
                    written > 0,
                "write authenticated input pipe frame");
        position += written;
    }
}

void read_exact(const HANDLE pipe, const std::span<std::byte> bytes) {
    std::size_t position{};
    while (position < bytes.size()) {
        DWORD read{};
        require(ReadFile(pipe, bytes.data() + position,
                         static_cast<DWORD>(bytes.size() - position), &read, nullptr) != FALSE &&
                    read > 0,
                "read authenticated input pipe frame");
        position += read;
    }
}

std::vector<std::byte> read_frame(const HANDLE pipe) {
    std::array<std::byte, 4> prefix{};
    read_exact(pipe, prefix);
    std::uint32_t size{};
    for (unsigned shift = 0; shift < 32; shift += 8) {
        size |= static_cast<std::uint32_t>(
                    std::to_integer<unsigned char>(prefix[shift / 8])) << shift;
    }
    require(size > 0 && size <= input::maximum_chunk_bytes + 4096U,
            "bounded input pipe frame");
    std::vector<std::byte> body(size);
    read_exact(pipe, body);
    return body;
}

void write_frame(const HANDLE pipe, const std::span<const std::byte> body) {
    require(body.size() <= 64, "bounded consumer ack");
    std::array<std::byte, 4> prefix{};
    const auto size = static_cast<std::uint32_t>(body.size());
    for (unsigned shift = 0; shift < 32; shift += 8) {
        prefix[shift / 8] = static_cast<std::byte>((size >> shift) & 0xffU);
    }
    write_exact(pipe, prefix);
    write_exact(pipe, body);
}

HANDLE connect_pipe(const std::wstring& endpoint) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (std::chrono::steady_clock::now() < deadline) {
        const auto pipe = CreateFileW(endpoint.c_str(), GENERIC_READ | GENERIC_WRITE,
                                      0, nullptr, OPEN_EXISTING, 0, nullptr);
        if (pipe != INVALID_HANDLE_VALUE) return pipe;
        const auto error = GetLastError();
        if (error != ERROR_PIPE_BUSY && error != ERROR_FILE_NOT_FOUND) break;
        (void)WaitNamedPipeW(endpoint.c_str(), 100);
        std::this_thread::sleep_for(std::chrono::milliseconds(25));
    }
    return INVALID_HANDLE_VALUE;
}

std::pair<input::RehearsalReceipt, std::uint64_t> consume_stream(
    input::RehearsalLease& lease, const std::function<void()>& after_hello = {}) {
    const auto pipe = connect_pipe(wide(lease.producer_endpoint));
    require(pipe != INVALID_HANDLE_VALUE, "connect one-use input pipe");
    write_exact(pipe, lease.one_time_token);
    const auto hello_body = read_frame(pipe);
    const auto hello = input::decode_hello(hello_body);
    require(hello && hello->stream_id == lease.stream_id &&
                hello->session_id == lease.session_id && hello->turn_id == lease.turn_id &&
                hello->generation == lease.generation &&
                hello->sample_rate == lease.sample_rate && hello->channels == lease.channels &&
                hello->activation_source == lease.activation_source &&
                hello->ptt_press_transition_sequence == lease.ptt_press_transition_sequence,
            "authenticated input hello equals lease");
    if (after_hello) after_hello();
    std::uint64_t expected_sequence{1};
    std::uint64_t expected_frame_index{};
    std::uint64_t streamed_frames{};
    input::RehearsalReceipt receipt;
    for (;;) {
        const auto body = read_frame(pipe);
        if (const auto chunk = input::decode_chunk(body, lease.channels)) {
            require(chunk->sequence == expected_sequence &&
                        chunk->first_frame_index == expected_frame_index &&
                        chunk->first_frame_qpc > 0 && chunk->frame_count > 0,
                    "ordered timestamped real input PCM chunk");
            const input::ConsumerAck ack{input::schema_version, expected_sequence,
                                         input::AckAction::continue_stream};
            const auto ack_body = input::encode_ack(ack);
            require(ack_body.has_value(), "encode real input ack");
            write_frame(pipe, *ack_body);
            ++expected_sequence;
            expected_frame_index += chunk->frame_count;
            streamed_frames += chunk->frame_count;
            continue;
        }
        const auto decoded = input::decode_receipt(body);
        require(decoded.has_value(), "decode terminal real input receipt");
        receipt = *decoded;
        break;
    }
    CloseHandle(pipe);
    playback::clear_authentication_token(lease.one_time_token);
    return {std::move(receipt), streamed_frames};
}

void enumerate_select_and_stream_real_default_input() {
    auto ptt_tracker = std::make_shared<windows::PttActivationTracker>();
    windows::InputRehearsalService service("windows-input-smoke", ptt_tracker);
    std::string error;
    const auto snapshot = service.enumerate_audio_inputs(error);
    require(snapshot.has_value(), "enumerate real Windows capture endpoints");
    require(snapshot->catalog_generation > 0, "nonzero real input catalog generation");
    const auto selected = service.select_audio_input(
        {playback::AudioOutputSelectionMode::system_default, {}}, error);
    require(selected.has_value(), "select explicit system-default capture endpoint");
    require(selected->resolved.system_default && !selected->resolved.endpoint_id.empty() &&
                selected->resolved.generation > 0,
            "resolved stable real default input identity");
    const auto exact = service.select_audio_input(
        {playback::AudioOutputSelectionMode::endpoint_id,
         selected->resolved.endpoint_id}, error);
    require(exact && exact->resolved.endpoint_id == selected->resolved.endpoint_id &&
                exact->resolved.generation == selected->resolved.generation,
            "validate exact real input endpoint selection");
    const auto default_again = service.select_audio_input(
        {playback::AudioOutputSelectionMode::system_default, {}}, error);
    require(default_again.has_value(), "restore explicit system-default input selection");

    LARGE_INTEGER counter{};
    LARGE_INTEGER frequency{};
    require(QueryPerformanceCounter(&counter) && QueryPerformanceFrequency(&frequency),
            "query monotonic clock");
    const windows::InputRehearsalAllocation request{
        "windows-input-smoke-session", "windows-input-smoke-turn", 3,
        500, 16'000, 1, 8'000, GetCurrentProcessId()};
    auto lease = service.allocate(request, static_cast<std::uint64_t>(counter.QuadPart),
                                  static_cast<std::uint64_t>(frequency.QuadPart), error);
    require(lease.has_value(), "allocate authenticated real input rehearsal");
    require(lease->input_endpoint_id == default_again->resolved.endpoint_id &&
                lease->input_endpoint_generation == default_again->resolved.generation,
            "lease proves exact selected input endpoint generation");

    auto [receipt, streamed_frames] = consume_stream(*lease);
    require(streamed_frames > 0 && receipt.captured_frames == streamed_frames,
            "real input receipt accounts every acknowledged source frame");
    require(receipt.source_capture_complete && !receipt.cancelled && !receipt.device_lost,
            "real input source completed without false success");
    require(receipt.stream_id == lease->stream_id && receipt.session_id == lease->session_id &&
                receipt.turn_id == lease->turn_id && receipt.generation == lease->generation,
            "real input receipt proves session turn and generation");
    require(receipt.input_selection_mode == lease->input_selection_mode &&
                receipt.input_endpoint_id == lease->input_endpoint_id &&
                receipt.input_endpoint_generation == lease->input_endpoint_generation,
            "real input receipt proves exact endpoint identity and generation");
    require(receipt.sample_rate == lease->sample_rate && receipt.channels == lease->channels &&
                receipt.captured_duration_micros <= 600'000,
            "real input receipt proves bounded format and duration");
    service.reap_finished();
    require(!service.cancel(lease->stream_id, lease->generation),
            "completed one-use input stream cannot be cancelled or replayed");

    require(QueryPerformanceCounter(&counter), "query PTT configure clock");
    ptt_tracker->configure(0x77, static_cast<std::uint64_t>(counter.QuadPart));
    const auto armed_baseline = ptt_tracker->snapshot();
    require(armed_baseline.virtual_key == 0x77 &&
                armed_baseline.state == PttState::released,
            "arm records a released PTT baseline");
    std::jthread later_press([ptt_tracker] {
        std::this_thread::sleep_for(std::chrono::milliseconds(75));
        LARGE_INTEGER pressed_qpc{};
        if (QueryPerformanceCounter(&pressed_qpc)) {
            ptt_tracker->transition(PttState::pressed,
                static_cast<std::uint64_t>(pressed_qpc.QuadPart));
        }
    });
    windows::PttActivationSnapshot newer_press;
    const auto press_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(1);
    while (std::chrono::steady_clock::now() < press_deadline) {
        newer_press = ptt_tracker->snapshot();
        if (newer_press.state == PttState::pressed &&
            newer_press.transition_sequence > armed_baseline.transition_sequence) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(25)); // exactly 40 Hz maximum
    }
    later_press.join();
    require(newer_press.state == PttState::pressed &&
                newer_press.transition_sequence > armed_baseline.transition_sequence &&
                newer_press.transition_qpc > armed_baseline.transition_qpc,
            "arm observes a strictly newer later PTT press");
    const windows::InputRehearsalAllocation ptt_request{
        "windows-input-smoke-session", "windows-input-ptt-turn", 4,
        2'000, 16'000, 1, 32'000, GetCurrentProcessId(),
        input::ActivationSource::push_to_talk};
    require(QueryPerformanceCounter(&counter), "query PTT allocation clock");
    auto ptt_lease = service.allocate(ptt_request,
        static_cast<std::uint64_t>(counter.QuadPart),
        static_cast<std::uint64_t>(frequency.QuadPart), error);
    require(ptt_lease && ptt_lease->activation_source == input::ActivationSource::push_to_talk &&
                ptt_lease->ptt_virtual_key == 0x77 &&
                ptt_lease->ptt_press_transition_sequence > 0 &&
                ptt_lease->ptt_pressed_qpc > 0,
            "allocate broker-attested PTT input stream");
    std::jthread release([ptt_tracker] {
        std::this_thread::sleep_for(std::chrono::milliseconds(800));
        LARGE_INTEGER released{};
        if (QueryPerformanceCounter(&released)) {
            ptt_tracker->transition(PttState::released,
                                    static_cast<std::uint64_t>(released.QuadPart));
        }
    });
    auto [ptt_receipt, ptt_frames] = consume_stream(*ptt_lease);
    release.join();
    if (!(ptt_frames > 0 && ptt_receipt.source_capture_complete &&
          !ptt_receipt.cancelled && !ptt_receipt.device_lost)) {
        std::cerr << "PTT diagnostic frames=" << ptt_frames
                  << " receipt_frames=" << ptt_receipt.captured_frames
                  << " complete=" << ptt_receipt.source_capture_complete
                  << " cancelled=" << ptt_receipt.cancelled
                  << " device_lost=" << ptt_receipt.device_lost
                  << " press=" << ptt_receipt.ptt_press_transition_sequence
                  << " release=" << ptt_receipt.ptt_release_transition_sequence << '\n';
    }
    require(ptt_frames > 0 && ptt_receipt.source_capture_complete &&
                !ptt_receipt.cancelled && !ptt_receipt.device_lost,
            "PTT release completes real captured input");
    require(ptt_receipt.activation_source == input::ActivationSource::push_to_talk &&
                ptt_receipt.ptt_virtual_key == ptt_lease->ptt_virtual_key &&
                ptt_receipt.ptt_press_transition_sequence ==
                    ptt_lease->ptt_press_transition_sequence &&
                ptt_receipt.ptt_pressed_qpc == ptt_lease->ptt_pressed_qpc &&
                ptt_receipt.ptt_release_transition_sequence >
                    ptt_receipt.ptt_press_transition_sequence &&
                ptt_receipt.ptt_released_qpc >= ptt_receipt.ptt_pressed_qpc,
            "terminal receipt proves exact PTT press and release transitions");
    service.reap_finished();
    require(QueryPerformanceCounter(&counter), "query replay rejection clock");
    require(!service.allocate(ptt_request, static_cast<std::uint64_t>(counter.QuadPart),
                              static_cast<std::uint64_t>(frequency.QuadPart), error),
            "reject released or replayed PTT transition");
}

} // namespace

int main() {
    try {
        enumerate_select_and_stream_real_default_input();
        std::cout << "real Windows input enumeration and authenticated stream smoke passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "real Windows input smoke failed: " << error.what() << '\n';
        return 1;
    }
}

#endif
