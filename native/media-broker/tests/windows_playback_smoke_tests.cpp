#ifdef _WIN32

#include "windows_playback.hpp"
#include "windows_service.hpp"

#include <Windows.h>

#include <algorithm>
#include <array>
#include <cassert>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <span>
#include <thread>
#include <vector>

namespace {

using namespace npc::media;

bool write_exact(const HANDLE pipe, const std::span<const std::byte> input) {
    std::size_t position{};
    while (position < input.size()) {
        DWORD written{};
        if (!WriteFile(pipe, input.data() + position,
                       static_cast<DWORD>(input.size() - position), &written, nullptr) || written == 0) return false;
        position += written;
    }
    return true;
}

bool read_exact(const HANDLE pipe, const std::span<std::byte> output) {
    std::size_t position{};
    while (position < output.size()) {
        DWORD read{};
        if (!ReadFile(pipe, output.data() + position,
                      static_cast<DWORD>(output.size() - position), &read, nullptr) || read == 0) return false;
        position += read;
    }
    return true;
}

std::optional<playback::ProducerResponse> transact(
    const HANDLE pipe, const playback::ProducerEnvelope& envelope) {
    const auto request = playback::encode_envelope(envelope);
    if (!request || !write_exact(pipe, *request)) return std::nullopt;
    std::array<std::byte, 4> prefix{};
    if (!read_exact(pipe, prefix)) return std::nullopt;
    const auto size = playback::decode_frame_size(prefix);
    if (!size) return std::nullopt;
    std::vector<std::byte> body(*size);
    if (!read_exact(pipe, body)) return std::nullopt;
    return playback::decode_response(body);
}

playback::ProducerEnvelope request(const playback::PlaybackLease& lease,
                                   const playback::ProducerCommand command,
                                   const std::uint64_t sequence,
                                   std::vector<std::byte> payload = {}) {
    return {playback::schema_version, command, sequence,
            windows::qpc_now() + windows::qpc_frequency() * 3,
            lease.generation, lease.stream_id, lease.session_id, lease.turn_id,
            lease.one_time_token, std::move(payload)};
}

std::vector<std::byte> quiet_sine(const std::uint32_t frames,
                                  const std::uint32_t sample_rate) {
    constexpr double tau = 6.28318530717958647692;
    std::vector<std::byte> bytes(static_cast<std::size_t>(frames) * 2);
    for (std::uint32_t frame = 0; frame < frames; ++frame) {
        const auto sample = static_cast<std::int16_t>(800.0 *
            std::sin(tau * 220.0 * static_cast<double>(frame) / sample_rate));
        bytes[static_cast<std::size_t>(frame) * 2] = static_cast<std::byte>(sample & 0xff);
        bytes[static_cast<std::size_t>(frame) * 2 + 1] =
            static_cast<std::byte>((static_cast<std::uint16_t>(sample) >> 8U) & 0xffU);
    }
    return bytes;
}

} // namespace

int main() {
    constexpr std::uint32_t sample_rate = 24'000;
    constexpr std::uint32_t frames = 2'400;
    windows::PlaybackService service("wasapi-smoke");
    std::string error;
    const auto outputs = service.enumerate_audio_outputs(error);
    if (!outputs || outputs->catalog_generation == 0 || outputs->endpoints.empty()) {
        std::cerr << "real Windows audio output enumeration failed: " << error << '\n';
        return 1;
    }
    const auto default_output = std::find_if(
        outputs->endpoints.begin(), outputs->endpoints.end(), [](const auto& endpoint) {
            return endpoint.system_default && endpoint.state == windows::AudioOutputState::active;
        });
    if (default_output == outputs->endpoints.end()) {
        std::cerr << "Windows has no active system-default output endpoint\n";
        return 1;
    }
    if (!service.select_audio_output(
            {playback::AudioOutputSelectionMode::system_default, {}}, error)) {
        std::cerr << "explicit system-default selection failed: " << error << '\n';
        return 1;
    }
    const auto selected = service.select_audio_output(
        {playback::AudioOutputSelectionMode::endpoint_id, default_output->endpoint_id}, error);
    if (!selected || selected->resolved.endpoint_id != default_output->endpoint_id ||
        selected->resolved.generation != default_output->generation) {
        std::cerr << "explicit endpoint-ID selection failed: " << error << '\n';
        return 1;
    }
    const playback::AllocationRequest allocation{
        "smoke-session", "smoke-turn", 1, sample_rate, 1, frames, GetCurrentProcessId()};
    const auto lease = service.allocate(allocation, windows::qpc_now(),
                                        windows::qpc_frequency(), error);
    if (!lease) {
        std::cerr << "allocation failed: " << error << '\n';
        return 1;
    }
    const std::wstring endpoint(lease->producer_endpoint.begin(), lease->producer_endpoint.end());
    HANDLE pipe = INVALID_HANDLE_VALUE;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (std::chrono::steady_clock::now() < deadline) {
        pipe = CreateFileW(endpoint.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                           OPEN_EXISTING, 0, nullptr);
        if (pipe != INVALID_HANDLE_VALUE) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (pipe == INVALID_HANDLE_VALUE) {
        std::cerr << "producer pipe connection failed\n";
        return 1;
    }
    const auto begin = transact(pipe, request(*lease, playback::ProducerCommand::begin, 1));
    if (!begin || begin->status != playback::ProducerStatus::ok) {
        std::cerr << "Begin failed: response=" << static_cast<bool>(begin);
        if (begin) std::cerr << " status=" << playback::to_string(begin->status)
                             << " receipt=" << begin->receipt.has_value();
        std::cerr << '\n';
        return 1;
    }
    const auto chunk = transact(pipe, request(*lease, playback::ProducerCommand::chunk, 2,
                                              quiet_sine(frames, sample_rate)));
    if (!chunk || chunk->status != playback::ProducerStatus::ok ||
        chunk->accepted_source_frames != frames) {
        std::cerr << "Chunk failed\n";
        return 1;
    }
    std::optional<windows::VisualAudioEnvelope> live_envelope;
    const auto envelope_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (std::chrono::steady_clock::now() < envelope_deadline && !live_envelope) {
        live_envelope = service.query_visual_audio_envelope(
            lease->session_id, lease->turn_id, lease->generation,
            lease->stream_id, lease->stream_id);
        if (!live_envelope) std::this_thread::sleep_for(std::chrono::milliseconds(5));
    }
    if (!live_envelope || !live_envelope->active || live_envelope->draining ||
        live_envelope->cancelled || live_envelope->device_write_qpc == 0 ||
        live_envelope->qpc_frequency != windows::qpc_frequency() ||
        live_envelope->source_sample_count == 0 ||
        live_envelope->source_frames < live_envelope->source_sample_start +
                                           live_envelope->source_sample_count ||
        live_envelope->device_frames == 0 ||
        std::none_of(live_envelope->mono_peak_q15.begin(),
                     live_envelope->mono_peak_q15.end(),
                     [](const auto value) { return value > 0; })) {
        std::cerr << "post-ReleaseBuffer visual envelope was unavailable or malformed\n";
        return 1;
    }
    if (service.query_visual_audio_envelope(
            lease->session_id, lease->turn_id, lease->generation,
            "wrong-stream", "wrong-stream")) {
        std::cerr << "visual envelope accepted a mismatched stream identity\n";
        return 1;
    }
    const auto finish = transact(pipe, request(*lease, playback::ProducerCommand::finish, 3));
    CloseHandle(pipe);
    if (!finish || finish->status != playback::ProducerStatus::ok || !finish->receipt) {
        std::cerr << "Finish failed\n";
        return 1;
    }
    if (service.query_visual_audio_envelope(
            lease->session_id, lease->turn_id, lease->generation,
            lease->stream_id, lease->stream_id)) {
        std::cerr << "visual envelope survived endpoint drain\n";
        return 1;
    }
    const auto& receipt = *finish->receipt;
    if (receipt.source_frames != frames || receipt.device_frames != frames ||
        receipt.source_duration_micros != 100'000 || !receipt.source_submission_complete ||
        !receipt.endpoint_drain_complete || receipt.cancelled ||
        receipt.output_selection_mode != playback::AudioOutputSelectionMode::endpoint_id ||
        receipt.output_endpoint_id != default_output->endpoint_id ||
        receipt.output_endpoint_generation != default_output->generation) {
        std::cerr << "receipt mismatch: source=" << receipt.source_frames
                  << " device=" << receipt.device_frames
                  << " duration_us=" << receipt.source_duration_micros
                  << " source_complete=" << receipt.source_submission_complete
                  << " drained=" << receipt.endpoint_drain_complete
                  << " cancelled=" << receipt.cancelled << '\n';
        return 1;
    }
    service.cancel_all();
    windows::PlaybackService bounded_pool("pool-smoke");
    for (std::uint64_t index = 0; index < 16; ++index) {
        const playback::AllocationRequest item{
            "pool-session", "pool-turn-" + std::to_string(index), index + 1,
            sample_rate, 1, frames, GetCurrentProcessId()};
        if (!bounded_pool.allocate(item, windows::qpc_now(), windows::qpc_frequency(), error)) {
            std::cerr << "bounded pool rejected lease " << index << ": " << error << '\n';
            return 1;
        }
    }
    const playback::AllocationRequest seventeenth{
        "pool-session", "pool-turn-17", 17, sample_rate, 1, frames, GetCurrentProcessId()};
    if (bounded_pool.allocate(seventeenth, windows::qpc_now(), windows::qpc_frequency(), error)) {
        std::cerr << "bounded pool accepted a seventeenth live endpoint\n";
        return 1;
    }
    bounded_pool.cancel_all();
    std::cout << "real WASAPI playback receipt/drain smoke passed: source_frames="
              << receipt.source_frames << " device_frames=" << receipt.device_frames
              << " duration_us=" << receipt.source_duration_micros
              << " endpoint_generation=" << receipt.output_endpoint_generation
              << " visual_bins=" << live_envelope->mono_rms_q15.size() << '\n';
}

#endif
