#ifdef _WIN32

#include "windows_input.hpp"

#include "windows_service.hpp"

#include <Windows.h>
#include <audioclient.h>
#include <bcrypt.h>
#include <propkeydef.h>
#include <propsys.h>
#include <functiondiscoverykeys_devpkey.h>
#include <ksmedia.h>
#include <mmdeviceapi.h>
#include <propvarutil.h>
#include <sddl.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstring>
#include <limits>
#include <mutex>
#include <span>
#include <thread>
#include <utility>
#include <vector>

namespace npc::media::windows {

using Microsoft::WRL::ComPtr;

namespace {

constexpr std::uint64_t allocation_lifetime_seconds = 60;
constexpr std::size_t maximum_live_rehearsals = 1;

struct ComApartment {
    HRESULT result{CoInitializeEx(nullptr, COINIT_MULTITHREADED)};
    ~ComApartment() { if (SUCCEEDED(result)) CoUninitialize(); }
    [[nodiscard]] bool usable() const noexcept {
        return SUCCEEDED(result) || result == RPC_E_CHANGED_MODE;
    }
};

[[nodiscard]] std::string utf8(const std::wstring_view value) {
    if (value.empty()) return {};
    const auto size = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                                          static_cast<int>(value.size()), nullptr, 0,
                                          nullptr, nullptr);
    if (size <= 0) return {};
    std::string result(static_cast<std::size_t>(size), '\0');
    if (WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), result.data(), size,
                            nullptr, nullptr) != size) return {};
    return result;
}

[[nodiscard]] std::wstring wide(const std::string_view value) {
    if (value.empty()) return {};
    const auto size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                          static_cast<int>(value.size()), nullptr, 0);
    if (size <= 0) return {};
    std::wstring result(static_cast<std::size_t>(size), L'\0');
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), result.data(), size) != size) return {};
    return result;
}

[[nodiscard]] std::uint64_t hash_bytes(std::uint64_t hash,
                                       const std::string_view value) noexcept {
    constexpr std::uint64_t prime = 1099511628211ULL;
    for (const unsigned char byte : value) {
        hash ^= byte;
        hash *= prime;
    }
    return hash;
}

[[nodiscard]] AudioInputState input_state(const DWORD value) noexcept {
    if ((value & DEVICE_STATE_ACTIVE) != 0) return AudioInputState::active;
    if ((value & DEVICE_STATE_DISABLED) != 0) return AudioInputState::disabled;
    if ((value & DEVICE_STATE_UNPLUGGED) != 0) return AudioInputState::unplugged;
    return AudioInputState::not_present;
}

[[nodiscard]] std::optional<AudioInputEndpoint> describe_input(
    IMMDevice* device, const std::string_view default_id) {
    if (!device) return std::nullopt;
    LPWSTR raw_id{};
    if (FAILED(device->GetId(&raw_id)) || !raw_id) return std::nullopt;
    const auto endpoint_id = utf8(raw_id);
    CoTaskMemFree(raw_id);
    if (endpoint_id.empty() || endpoint_id.size() > playback::maximum_endpoint_id_bytes) {
        return std::nullopt;
    }
    DWORD raw_state{};
    if (FAILED(device->GetState(&raw_state))) return std::nullopt;
    std::string friendly_name;
    ComPtr<IPropertyStore> properties;
    if (SUCCEEDED(device->OpenPropertyStore(STGM_READ, &properties))) {
        PROPVARIANT value;
        PropVariantInit(&value);
        if (SUCCEEDED(properties->GetValue(PKEY_Device_FriendlyName, &value)) &&
            value.vt == VT_LPWSTR && value.pwszVal) {
            friendly_name = utf8(value.pwszVal);
        }
        PropVariantClear(&value);
    }
    if (friendly_name.empty()) friendly_name = endpoint_id;
    if (friendly_name.size() > 512) friendly_name.resize(512);
    auto generation = hash_bytes(1469598103934665603ULL, endpoint_id);
    generation = hash_bytes(generation, friendly_name);
    generation ^= raw_state;
    generation *= 1099511628211ULL;
    if (generation == 0) generation = 1;
    return AudioInputEndpoint{endpoint_id, friendly_name, input_state(raw_state),
                              endpoint_id == default_id, generation};
}

[[nodiscard]] std::optional<AudioInputSnapshot> enumerate_inputs(std::string& error) {
    const ComApartment apartment;
    if (!apartment.usable()) {
        error = "Initialize COM for audio input enumeration failed";
        return std::nullopt;
    }
    ComPtr<IMMDeviceEnumerator> enumerator;
    if (FAILED(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_INPROC_SERVER,
                                IID_PPV_ARGS(&enumerator)))) {
        error = "Create Windows audio input enumerator failed";
        return std::nullopt;
    }
    std::string default_id;
    ComPtr<IMMDevice> default_device;
    if (SUCCEEDED(enumerator->GetDefaultAudioEndpoint(eCapture, eConsole, &default_device))) {
        LPWSTR raw{};
        if (SUCCEEDED(default_device->GetId(&raw)) && raw) {
            default_id = utf8(raw);
            CoTaskMemFree(raw);
        }
    }
    ComPtr<IMMDeviceCollection> collection;
    if (FAILED(enumerator->EnumAudioEndpoints(eCapture, DEVICE_STATEMASK_ALL, &collection))) {
        error = "Enumerate Windows capture endpoints failed";
        return std::nullopt;
    }
    UINT count{};
    if (FAILED(collection->GetCount(&count)) || count > 128) {
        error = "Windows capture endpoint count is invalid";
        return std::nullopt;
    }
    AudioInputSnapshot snapshot;
    snapshot.endpoints.reserve(count);
    for (UINT index = 0; index < count; ++index) {
        ComPtr<IMMDevice> device;
        if (SUCCEEDED(collection->Item(index, &device))) {
            if (auto endpoint = describe_input(device.Get(), default_id)) {
                snapshot.endpoints.push_back(std::move(*endpoint));
            }
        }
    }
    std::sort(snapshot.endpoints.begin(), snapshot.endpoints.end(),
              [](const auto& left, const auto& right) {
                  return left.endpoint_id < right.endpoint_id;
              });
    auto catalog = 1469598103934665603ULL;
    for (const auto& endpoint : snapshot.endpoints) {
        catalog = hash_bytes(catalog, endpoint.endpoint_id);
        catalog ^= endpoint.generation;
        catalog *= 1099511628211ULL;
        catalog ^= static_cast<std::uint64_t>(endpoint.system_default);
        catalog *= 1099511628211ULL;
    }
    snapshot.catalog_generation = catalog == 0 ? 1 : catalog;
    return snapshot;
}

[[nodiscard]] std::optional<SelectedAudioInput> resolve_input(
    const AudioInputSelection& selection, std::string& error) {
    if (selection.mode != playback::AudioOutputSelectionMode::system_default &&
        selection.mode != playback::AudioOutputSelectionMode::endpoint_id) {
        error = "Audio input selection mode is invalid";
        return std::nullopt;
    }
    if (selection.mode == playback::AudioOutputSelectionMode::endpoint_id &&
        (selection.endpoint_id.empty() ||
         selection.endpoint_id.size() > playback::maximum_endpoint_id_bytes ||
         selection.endpoint_id.find('\0') != std::string::npos)) {
        error = "Selected Windows audio input endpoint ID is malformed";
        return std::nullopt;
    }
    const auto snapshot = enumerate_inputs(error);
    if (!snapshot) return std::nullopt;
    const auto match = std::find_if(snapshot->endpoints.begin(), snapshot->endpoints.end(),
        [&](const AudioInputEndpoint& endpoint) {
            return selection.mode == playback::AudioOutputSelectionMode::system_default
                       ? endpoint.system_default
                       : endpoint.endpoint_id == selection.endpoint_id;
        });
    if (match == snapshot->endpoints.end()) {
        error = selection.mode == playback::AudioOutputSelectionMode::system_default
                    ? "Windows has no system-default capture endpoint"
                    : "Selected Windows audio input is no longer present";
        return std::nullopt;
    }
    if (match->state != AudioInputState::active) {
        error = "Selected Windows audio input is not active";
        return std::nullopt;
    }
    return SelectedAudioInput{1, selection, *match};
}

[[nodiscard]] std::string random_hex(const std::size_t bytes) {
    std::vector<unsigned char> value(bytes);
    if (!BCRYPT_SUCCESS(BCryptGenRandom(nullptr, value.data(),
                                        static_cast<ULONG>(value.size()),
                                        BCRYPT_USE_SYSTEM_PREFERRED_RNG))) return {};
    constexpr char hex[] = "0123456789abcdef";
    std::string result;
    result.reserve(bytes * 2);
    for (const auto byte : value) {
        result.push_back(hex[byte >> 4U]);
        result.push_back(hex[byte & 0x0fU]);
    }
    SecureZeroMemory(value.data(), value.size());
    return result;
}

[[nodiscard]] bool random_token(playback::AuthenticationToken& token) {
    return BCRYPT_SUCCESS(BCryptGenRandom(nullptr, reinterpret_cast<PUCHAR>(token.data()),
                                          static_cast<ULONG>(token.size()),
                                          BCRYPT_USE_SYSTEM_PREFERRED_RNG));
}

[[nodiscard]] std::vector<std::byte> token_user_sid(const HANDLE process) {
    HANDLE token{};
    if (!OpenProcessToken(process, TOKEN_QUERY, &token)) return {};
    DWORD size{};
    GetTokenInformation(token, TokenUser, nullptr, 0, &size);
    std::vector<std::byte> bytes(size);
    if (!GetTokenInformation(token, TokenUser, bytes.data(), size, &size)) bytes.clear();
    CloseHandle(token);
    return bytes;
}

struct LocalSecurityDescriptor {
    PSECURITY_DESCRIPTOR descriptor{};
    ~LocalSecurityDescriptor() { if (descriptor) LocalFree(descriptor); }
};

[[nodiscard]] bool current_user_pipe_security(LocalSecurityDescriptor& owner,
                                              SECURITY_ATTRIBUTES& attributes) {
    const auto current_user = token_user_sid(GetCurrentProcess());
    if (current_user.empty()) return false;
    const auto* user = reinterpret_cast<const TOKEN_USER*>(current_user.data());
    LPWSTR sid_text{};
    if (!ConvertSidToStringSidW(user->User.Sid, &sid_text)) return false;
    const std::wstring sddl = L"D:P(A;;GA;;;" + std::wstring(sid_text) + L")";
    LocalFree(sid_text);
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.c_str(), SDDL_REVISION_1, &owner.descriptor, nullptr)) return false;
    attributes = {sizeof(attributes), owner.descriptor, FALSE};
    return true;
}

[[nodiscard]] bool pipe_io(const HANDLE pipe, const bool write,
                           const std::span<std::byte> buffer, DWORD& transferred) {
    OVERLAPPED operation{};
    operation.hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (!operation.hEvent) return false;
    const BOOL immediate = write
        ? WriteFile(pipe, buffer.data(), static_cast<DWORD>(buffer.size()), &transferred, &operation)
        : ReadFile(pipe, buffer.data(), static_cast<DWORD>(buffer.size()), &transferred, &operation);
    bool complete = immediate != FALSE;
    const auto operation_error = complete ? ERROR_SUCCESS : GetLastError();
    if (!complete && operation_error == ERROR_IO_PENDING &&
        WaitForSingleObject(operation.hEvent, 2'000) == WAIT_OBJECT_0) {
        complete = GetOverlappedResult(pipe, &operation, &transferred, FALSE) != FALSE;
    } else if (!complete && operation_error == ERROR_IO_PENDING) {
        CancelIoEx(pipe, &operation);
        (void)WaitForSingleObject(operation.hEvent, 100);
    }
    CloseHandle(operation.hEvent);
    return complete;
}

[[nodiscard]] bool read_exact(const HANDLE pipe, const std::span<std::byte> output) {
    std::size_t position{};
    while (position < output.size()) {
        DWORD read{};
        if (!pipe_io(pipe, false, output.subspan(position), read) || read == 0) return false;
        position += read;
    }
    return true;
}

[[nodiscard]] bool write_exact(const HANDLE pipe, const std::span<const std::byte> input) {
    std::size_t position{};
    while (position < input.size()) {
        DWORD written{};
        auto remaining = std::span{const_cast<std::byte*>(input.data() + position),
                                   input.size() - position};
        if (!pipe_io(pipe, true, remaining, written) || written == 0) return false;
        position += written;
    }
    return true;
}

[[nodiscard]] bool write_frame(const HANDLE pipe, const std::span<const std::byte> body) {
    if (body.empty() || body.size() > input::maximum_chunk_bytes + 4096U) return false;
    std::array<std::byte, 4> prefix{};
    const auto size = static_cast<std::uint32_t>(body.size());
    for (unsigned shift = 0; shift < 32; shift += 8) {
        prefix[shift / 8] = static_cast<std::byte>((size >> shift) & 0xffU);
    }
    return write_exact(pipe, prefix) && write_exact(pipe, body);
}

[[nodiscard]] std::optional<std::vector<std::byte>> read_frame(
    const HANDLE pipe, const std::uint32_t maximum) {
    std::array<std::byte, 4> prefix{};
    if (!read_exact(pipe, prefix)) return std::nullopt;
    std::uint32_t size{};
    for (unsigned shift = 0; shift < 32; shift += 8) {
        size |= static_cast<std::uint32_t>(std::to_integer<unsigned char>(prefix[shift / 8]))
                << shift;
    }
    if (size == 0 || size > maximum) return std::nullopt;
    std::vector<std::byte> body(size);
    return read_exact(pipe, body) ? std::optional{std::move(body)} : std::nullopt;
}

[[nodiscard]] double sample_value(const BYTE* data, const std::size_t offset,
                                  const WORD bits, const bool floating) noexcept {
    if (floating && bits == 32) {
        float value{};
        std::memcpy(&value, data + offset, sizeof(value));
        return std::clamp(static_cast<double>(value), -1.0, 1.0);
    }
    if (!floating && bits == 16) {
        std::int16_t value{};
        std::memcpy(&value, data + offset, sizeof(value));
        return static_cast<double>(value) / 32768.0;
    }
    if (!floating && bits == 24) {
        std::int32_t value = static_cast<std::int32_t>(data[offset]) |
            (static_cast<std::int32_t>(data[offset + 1]) << 8) |
            (static_cast<std::int32_t>(data[offset + 2]) << 16);
        if ((value & 0x800000) != 0) value |= ~0xffffff;
        return static_cast<double>(value) / 8388608.0;
    }
    if (!floating && bits == 32) {
        std::int32_t value{};
        std::memcpy(&value, data + offset, sizeof(value));
        return static_cast<double>(value) / 2147483648.0;
    }
    return 0.0;
}

[[nodiscard]] std::int32_t milli_dbfs(const double amplitude) noexcept {
    if (!(amplitude > 0.000001)) return -120'000;
    return static_cast<std::int32_t>(std::clamp(20.0 * std::log10(amplitude) * 1000.0,
                                                -120'000.0, 0.0));
}

[[nodiscard]] bool capture_rehearsal(const input::RehearsalLease& lease,
                                     const std::atomic_bool& cancelled,
                                     const HANDLE pipe,
                                     const std::shared_ptr<PttActivationTracker>& ptt_tracker,
                                     input::RehearsalReceipt& receipt,
                                     std::string& error) {
    ComPtr<IMMDeviceEnumerator> enumerator;
    if (FAILED(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_INPROC_SERVER,
                                IID_PPV_ARGS(&enumerator)))) {
        error = "Create WASAPI capture enumerator failed";
        receipt.device_lost = true;
        return false;
    }
    ComPtr<IMMDevice> device;
    HRESULT hr{};
    if (lease.input_selection_mode == playback::AudioOutputSelectionMode::system_default) {
        hr = enumerator->GetDefaultAudioEndpoint(eCapture, eConsole, &device);
    } else {
        const auto endpoint = wide(lease.input_endpoint_id);
        hr = endpoint.empty() ? E_INVALIDARG : enumerator->GetDevice(endpoint.c_str(), &device);
    }
    if (FAILED(hr)) {
        error = "Open selected WASAPI capture endpoint failed";
        receipt.device_lost = true;
        return false;
    }
    const auto described = describe_input(device.Get(),
        lease.input_selection_mode == playback::AudioOutputSelectionMode::system_default
            ? lease.input_endpoint_id : std::string_view{});
    if (!described || described->endpoint_id != lease.input_endpoint_id ||
        described->generation != lease.input_endpoint_generation ||
        described->state != AudioInputState::active) {
        error = "Selected WASAPI capture endpoint changed before rehearsal";
        receipt.device_lost = true;
        return false;
    }
    ComPtr<IAudioClient> client;
    if (FAILED(device->Activate(__uuidof(IAudioClient), CLSCTX_INPROC_SERVER, nullptr,
                                reinterpret_cast<void**>(client.GetAddressOf())))) {
        error = "Activate WASAPI capture client failed";
        receipt.device_lost = true;
        return false;
    }
    WAVEFORMATEX requested{};
    requested.wFormatTag = WAVE_FORMAT_PCM;
    requested.nChannels = lease.channels;
    requested.nSamplesPerSec = lease.sample_rate;
    requested.wBitsPerSample = 16;
    requested.nBlockAlign = static_cast<WORD>(requested.nChannels * 2U);
    requested.nAvgBytesPerSec = requested.nSamplesPerSec * requested.nBlockAlign;
    receipt.sample_rate = requested.nSamplesPerSec;
    receipt.channels = requested.nChannels;
    HANDLE ready = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!ready) {
        error = "Create WASAPI capture event failed";
        receipt.device_lost = true;
        return false;
    }
    constexpr DWORD flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK |
                            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM |
                            AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    hr = client->Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 0, 0, &requested, nullptr);
    if (SUCCEEDED(hr)) hr = client->SetEventHandle(ready);
    ComPtr<IAudioCaptureClient> capture;
    if (SUCCEEDED(hr)) hr = client->GetService(IID_PPV_ARGS(&capture));
    if (SUCCEEDED(hr)) hr = client->Start();
    if (FAILED(hr)) {
        CloseHandle(ready);
        error = "Start WASAPI capture rehearsal failed";
        receipt.device_lost = true;
        return false;
    }
    const auto deadline = std::chrono::steady_clock::now() +
                          std::chrono::milliseconds(lease.duration_ms);
    long double sum_squares{};
    double peak{};
    std::uint64_t samples{};
    std::uint64_t sequence{};
    bool consumer_stop{};
    bool consumer_cancel{};
    bool ptt_released{};
    while (!cancelled.load(std::memory_order_acquire) &&
           !consumer_stop && !consumer_cancel && receipt.captured_frames < lease.max_frames &&
           std::chrono::steady_clock::now() < deadline) {
        if (lease.activation_source == input::ActivationSource::push_to_talk) {
            const auto activation = ptt_tracker ? ptt_tracker->snapshot()
                                                : PttActivationSnapshot{};
            if (activation.release_transition_sequence >
                lease.ptt_press_transition_sequence) {
                receipt.ptt_release_transition_sequence =
                    activation.release_transition_sequence;
                receipt.ptt_released_qpc = activation.released_qpc;
                ptt_released = true;
                break;
            }
        }
        const auto wait = WaitForSingleObject(ready, 100);
        if (wait != WAIT_OBJECT_0 && wait != WAIT_TIMEOUT) {
            receipt.device_lost = true;
            break;
        }
        UINT32 packet{};
        if (FAILED(capture->GetNextPacketSize(&packet))) {
            receipt.device_lost = true;
            break;
        }
        while (packet > 0) {
            BYTE* data{};
            UINT32 frames{};
            DWORD packet_flags{};
            if (FAILED(capture->GetBuffer(&data, &frames, &packet_flags, nullptr, nullptr))) {
                receipt.device_lost = true;
                break;
            }
            const auto accepted = static_cast<UINT32>(std::min<std::uint64_t>(
                frames, lease.max_frames - receipt.captured_frames));
            const auto frames_per_chunk = input::maximum_chunk_bytes / requested.nBlockAlign;
            UINT32 consumed{};
            while (consumed < accepted && !consumer_stop && !consumer_cancel) {
                const auto chunk_frames = std::min(accepted - consumed, frames_per_chunk);
                std::vector<std::byte> pcm(static_cast<std::size_t>(chunk_frames) *
                                           requested.nBlockAlign);
                if ((packet_flags & AUDCLNT_BUFFERFLAGS_SILENT) == 0 && data) {
                    std::memcpy(pcm.data(), data + static_cast<std::size_t>(consumed) *
                                             requested.nBlockAlign, pcm.size());
                }
                for (UINT32 frame = 0; frame < chunk_frames; ++frame) {
                    bool frame_silent = true;
                    for (WORD channel = 0; channel < requested.nChannels; ++channel) {
                        const auto offset = static_cast<std::size_t>(frame) *
                                                requested.nBlockAlign +
                                            static_cast<std::size_t>(channel) * 2U;
                        const auto value = sample_value(
                            reinterpret_cast<const BYTE*>(pcm.data()), offset, 16, false);
                        const auto magnitude = std::abs(value);
                        peak = std::max(peak, magnitude);
                        sum_squares += static_cast<long double>(value) * value;
                        ++samples;
                        if (magnitude >= 0.99) ++receipt.clipped_samples;
                        if (magnitude >= 0.01) frame_silent = false;
                    }
                    if (frame_silent) ++receipt.silent_frames;
                }
                const auto now = qpc_now();
                const auto duration_ticks = static_cast<std::uint64_t>(chunk_frames) *
                                            lease.qpc_frequency / lease.sample_rate;
                input::PcmChunk chunk{input::schema_version, ++sequence,
                    now > duration_ticks ? now - duration_ticks : 1,
                    receipt.captured_frames, chunk_frames, std::move(pcm)};
                const auto body = input::encode_chunk(chunk, lease.channels);
                if (!body || !write_frame(pipe, *body)) {
                    consumer_cancel = true;
                    break;
                }
                const auto ack_body = read_frame(pipe, 64);
                const auto ack = ack_body ? input::decode_ack(*ack_body) : std::nullopt;
                if (!ack || ack->sequence != sequence) {
                    consumer_cancel = true;
                    break;
                }
                consumer_stop = ack->action == input::AckAction::stop;
                consumer_cancel = ack->action == input::AckAction::cancel;
                if (consumer_stop &&
                    lease.activation_source == input::ActivationSource::push_to_talk) {
                    // The runtime cannot manufacture a PTT release. A Stop ACK
                    // before the broker observes release is a cancelled source.
                    consumer_stop = false;
                    consumer_cancel = true;
                }
                receipt.captured_frames += chunk_frames;
                consumed += chunk_frames;
            }
            capture->ReleaseBuffer(frames);
            if (FAILED(capture->GetNextPacketSize(&packet))) {
                receipt.device_lost = true;
                break;
            }
        }
        if (receipt.device_lost) break;
    }
    client->Stop();
    CloseHandle(ready);
    receipt.cancelled = cancelled.load(std::memory_order_acquire) || consumer_cancel;
    receipt.captured_duration_micros = receipt.sample_rate == 0 ? 0 :
        receipt.captured_frames * 1'000'000ULL / receipt.sample_rate;
    receipt.peak_milli_dbfs = milli_dbfs(peak);
    receipt.rms_milli_dbfs = milli_dbfs(samples == 0 ? 0.0 :
        std::sqrt(static_cast<double>(sum_squares / samples)));
    receipt.clipping_detected = receipt.clipped_samples > 0;
    receipt.silence_detected = receipt.captured_frames == 0 ||
        receipt.silent_frames * 100ULL >= receipt.captured_frames * 80ULL ||
        receipt.rms_milli_dbfs <= -50'000;
    const auto after = resolve_input({lease.input_selection_mode,
                                      lease.input_selection_mode ==
                                              playback::AudioOutputSelectionMode::endpoint_id
                                          ? lease.input_endpoint_id : std::string{}}, error);
    if (!after || after->resolved.endpoint_id != lease.input_endpoint_id ||
        after->resolved.generation != lease.input_endpoint_generation) {
        receipt.device_lost = true;
    }
    receipt.source_capture_complete = !receipt.cancelled && !receipt.device_lost &&
                                      receipt.captured_frames > 0 &&
        (lease.activation_source == input::ActivationSource::explicit_rehearsal ||
         ptt_released);
    return receipt.source_capture_complete;
}

class InputEndpoint final {
public:
    InputEndpoint(input::RehearsalLease lease, const std::uint32_t producer_pid,
                  std::shared_ptr<PttActivationTracker> ptt_tracker)
        : lease_(std::move(lease)), expected_producer_pid_(producer_pid),
          ptt_tracker_(std::move(ptt_tracker)) {}
    ~InputEndpoint() { cancel(); }

    [[nodiscard]] bool start(std::string& error) {
        LocalSecurityDescriptor descriptor;
        SECURITY_ATTRIBUTES attributes{};
        if (!current_user_pipe_security(descriptor, attributes)) {
            error = "Build current-user input rehearsal pipe ACL failed";
            return false;
        }
        const std::wstring pipe_name(lease_.producer_endpoint.begin(),
                                     lease_.producer_endpoint.end());
        pipe_ = CreateNamedPipeW(pipe_name.c_str(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1, 4096, 64, 0, &attributes);
        if (pipe_ == INVALID_HANDLE_VALUE) {
            error = "Create one-time input rehearsal pipe failed";
            return false;
        }
        worker_ = std::jthread([this](const std::stop_token token) { run(token); });
        return true;
    }

    void cancel() noexcept {
        cancelled_.store(true, std::memory_order_release);
        if (worker_.joinable()) worker_.request_stop();
        if (pipe_ != INVALID_HANDLE_VALUE) {
            CancelIoEx(pipe_, nullptr);
            DisconnectNamedPipe(pipe_);
        }
        if (worker_.joinable() && worker_.get_id() != std::this_thread::get_id()) worker_.join();
        if (pipe_ != INVALID_HANDLE_VALUE) CloseHandle(std::exchange(pipe_, INVALID_HANDLE_VALUE));
        playback::clear_authentication_token(lease_.one_time_token);
        finished_.store(true, std::memory_order_release);
    }

    [[nodiscard]] bool finished() const noexcept {
        return finished_.load(std::memory_order_acquire);
    }
    [[nodiscard]] bool terminal_receipt_sent() const noexcept {
        return terminal_receipt_sent_.load(std::memory_order_acquire);
    }
    [[nodiscard]] const input::RehearsalLease& lease() const noexcept { return lease_; }

private:
    [[nodiscard]] bool connect(const std::stop_token token) {
        OVERLAPPED operation{};
        operation.hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (!operation.hEvent) return false;
        const BOOL immediate = ConnectNamedPipe(pipe_, &operation);
        const auto code = immediate ? ERROR_SUCCESS : GetLastError();
        bool connected = immediate || code == ERROR_PIPE_CONNECTED;
        if (!connected && code != ERROR_IO_PENDING) {
            CloseHandle(operation.hEvent);
            return false;
        }
        while (!connected && !token.stop_requested()) {
            const auto now = qpc_now();
            if (now >= lease_.expires_qpc) break;
            const auto wait = WaitForSingleObject(operation.hEvent, 100);
            if (wait == WAIT_OBJECT_0) {
                DWORD transferred{};
                connected = GetOverlappedResult(pipe_, &operation, &transferred, FALSE) != FALSE;
                break;
            }
            if (wait != WAIT_TIMEOUT) break;
        }
        if (!connected) CancelIoEx(pipe_, &operation);
        CloseHandle(operation.hEvent);
        return connected;
    }

    void run(const std::stop_token token) {
        const ComApartment apartment;
        if (!apartment.usable() || !connect(token)) {
            finished_ = true;
            return;
        }
        ULONG producer_pid{};
        if (!GetNamedPipeClientProcessId(pipe_, &producer_pid) ||
            producer_pid != expected_producer_pid_) {
            finished_ = true;
            return;
        }
        HANDLE producer = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                                      FALSE, producer_pid);
        const bool trusted = producer && process_is_current_user_and_session(producer);
        if (producer) CloseHandle(producer);
        playback::AuthenticationToken supplied{};
        if (!trusted || !read_exact(pipe_, supplied) ||
            !playback::constant_time_equal(supplied, lease_.one_time_token)) {
            playback::clear_authentication_token(supplied);
            playback::clear_authentication_token(lease_.one_time_token);
            finished_ = true;
            return;
        }
        playback::clear_authentication_token(supplied);
        playback::clear_authentication_token(lease_.one_time_token);
        const input::StreamHello hello{
            input::schema_version,
            lease_.stream_id,
            lease_.session_id,
            lease_.turn_id,
            lease_.generation,
            lease_.sample_rate,
            lease_.channels,
            lease_.max_frames,
            lease_.max_chunk_bytes,
            lease_.qpc_frequency,
            lease_.activation_source,
            lease_.ptt_virtual_key,
            lease_.ptt_press_transition_sequence,
            lease_.ptt_pressed_qpc,
        };
        const auto hello_body = input::encode_hello(hello);
        if (!hello_body || !write_frame(pipe_, *hello_body)) {
            finished_ = true;
            return;
        }
        input::RehearsalReceipt receipt{
            input::schema_version,
            "mic-receipt-" + random_hex(16),
            lease_.stream_id,
            lease_.session_id,
            lease_.turn_id,
            lease_.generation,
            lease_.input_selection_mode,
            lease_.input_endpoint_id,
            lease_.input_endpoint_generation,
            lease_.sample_rate,
            lease_.channels,
            0,
            0,
            -120'000,
            -120'000,
            0,
            0,
            false,
            false,
            false,
            false,
            false,
            lease_.activation_source,
            lease_.ptt_virtual_key,
            lease_.ptt_press_transition_sequence,
            lease_.ptt_pressed_qpc,
            0,
            0,
        };
        std::string error;
        (void)capture_rehearsal(lease_, cancelled_, pipe_, ptt_tracker_, receipt, error);
        const auto encoded = input::encode_receipt(receipt);
        const bool terminal_sent = encoded && write_frame(pipe_, *encoded);
        terminal_receipt_sent_.store(terminal_sent, std::memory_order_release);
        FlushFileBuffers(pipe_);
        DisconnectNamedPipe(pipe_);
        finished_ = true;
    }

    input::RehearsalLease lease_;
    std::uint32_t expected_producer_pid_{};
    std::shared_ptr<PttActivationTracker> ptt_tracker_;
    HANDLE pipe_{INVALID_HANDLE_VALUE};
    std::jthread worker_;
    std::atomic_bool cancelled_{};
    std::atomic_bool finished_{};
    std::atomic_bool terminal_receipt_sent_{};
};

} // namespace

struct InputRehearsalService::Impl {
    Impl(std::string value, std::shared_ptr<PttActivationTracker> tracker)
        : broker_session_id(std::move(value)), ptt_tracker(std::move(tracker)) {}
    std::string broker_session_id;
    mutable std::mutex mutex;
    AudioInputSelection selection;
    std::shared_ptr<PttActivationTracker> ptt_tracker;
    std::uint64_t last_consumed_ptt_press_sequence{};
    std::vector<std::unique_ptr<InputEndpoint>> endpoints;
};

void PttActivationTracker::configure(const std::uint32_t virtual_key,
                                     const std::uint64_t at_qpc) noexcept {
    std::scoped_lock lock(mutex_);
    snapshot_.virtual_key = virtual_key;
    snapshot_.state = PttState::released;
    ++snapshot_.transition_sequence;
    snapshot_.transition_qpc = at_qpc;
    snapshot_.release_transition_sequence = snapshot_.transition_sequence;
    snapshot_.released_qpc = at_qpc;
}

void PttActivationTracker::transition(const PttState state,
                                      const std::uint64_t at_qpc) noexcept {
    std::scoped_lock lock(mutex_);
    if (snapshot_.virtual_key == 0 || snapshot_.state == state || at_qpc == 0) return;
    snapshot_.state = state;
    ++snapshot_.transition_sequence;
    snapshot_.transition_qpc = at_qpc;
    if (state == PttState::released) {
        snapshot_.release_transition_sequence = snapshot_.transition_sequence;
        snapshot_.released_qpc = at_qpc;
    }
}

PttActivationSnapshot PttActivationTracker::snapshot() const noexcept {
    std::scoped_lock lock(mutex_);
    return snapshot_;
}

InputRehearsalService::InputRehearsalService(
    std::string broker_session_id, std::shared_ptr<PttActivationTracker> ptt_tracker)
    : impl_(std::make_unique<Impl>(std::move(broker_session_id),
                                  std::move(ptt_tracker))) {}
InputRehearsalService::~InputRehearsalService() { cancel_all(); }

std::optional<AudioInputSnapshot> InputRehearsalService::enumerate_audio_inputs(
    std::string& error) const {
    return enumerate_inputs(error);
}

std::optional<SelectedAudioInput> InputRehearsalService::select_audio_input(
    const AudioInputSelection& selection, std::string& error) {
    auto selected = resolve_input(selection, error);
    if (!selected) return std::nullopt;
    std::scoped_lock lock(impl_->mutex);
    if (impl_->selection.mode != selection.mode ||
        impl_->selection.endpoint_id != selection.endpoint_id) {
        for (auto& endpoint : impl_->endpoints) endpoint->cancel();
        impl_->endpoints.clear();
        impl_->selection = selection;
    }
    return selected;
}

std::optional<SelectedAudioInput> InputRehearsalService::selected_audio_input(
    std::string& error) const {
    AudioInputSelection selection;
    {
        std::scoped_lock lock(impl_->mutex);
        selection = impl_->selection;
    }
    return resolve_input(selection, error);
}

std::optional<input::RehearsalLease> InputRehearsalService::allocate(
    const InputRehearsalAllocation& request, const std::uint64_t now_qpc,
    const std::uint64_t qpc_frequency, std::string& error) {
    if (request.session_id.empty() || request.session_id.size() > 128 ||
        request.session_id.find('\0') != std::string::npos || request.turn_id.empty() ||
        request.turn_id.size() > 128 || request.turn_id.find('\0') != std::string::npos ||
        request.generation == 0 ||
        request.duration_ms < input::minimum_rehearsal_duration_ms ||
        request.duration_ms > input::maximum_rehearsal_duration_ms ||
        request.sample_rate < playback::minimum_sample_rate ||
        request.sample_rate > playback::maximum_sample_rate || request.channels == 0 ||
        request.channels > playback::maximum_channels || request.max_frames == 0 ||
        request.max_frames > static_cast<std::uint64_t>(request.sample_rate) *
                                 request.duration_ms / 1000U ||
        request.expected_producer_process_id == 0 || now_qpc == 0 || qpc_frequency == 0) {
        error = "Input rehearsal allocation is malformed";
        return std::nullopt;
    }
    reap_finished();
    std::scoped_lock lock(impl_->mutex);
    if (impl_->endpoints.size() >= maximum_live_rehearsals) {
        error = "An input rehearsal is already active";
        return std::nullopt;
    }
    const auto selected = resolve_input(impl_->selection, error);
    if (!selected) return std::nullopt;
    PttActivationSnapshot activation;
    if (request.activation_source == input::ActivationSource::push_to_talk) {
        if (!impl_->ptt_tracker) {
            error = "Broker PTT attestation is unavailable";
            return std::nullopt;
        }
        activation = impl_->ptt_tracker->snapshot();
        if (activation.virtual_key == 0 || activation.state != PttState::pressed ||
            activation.transition_sequence <= impl_->last_consumed_ptt_press_sequence ||
            activation.transition_qpc == 0 || activation.transition_qpc > now_qpc ||
            now_qpc - activation.transition_qpc > qpc_frequency * 2U) {
            error = "A fresh unreplayed broker-attested PTT press is required";
            return std::nullopt;
        }
    } else if (request.activation_source != input::ActivationSource::explicit_rehearsal) {
        error = "Input activation source is invalid";
        return std::nullopt;
    }
    const auto random_id = random_hex(16);
    playback::AuthenticationToken token{};
    if (random_id.empty() || !random_token(token)) {
        error = "Generate input rehearsal authentication failed";
        return std::nullopt;
    }
    input::RehearsalLease lease{
        input::schema_version,
        "mic-" + random_id,
        R"(\\.\pipe\npc-media-input-)" + impl_->broker_session_id + "-" + random_id,
        token,
        request.session_id,
        request.turn_id,
        request.generation,
        request.duration_ms,
        request.sample_rate,
        request.channels,
        request.max_frames,
        input::maximum_chunk_bytes,
        now_qpc + qpc_frequency * allocation_lifetime_seconds,
        qpc_frequency,
        selected->selection.mode,
        selected->resolved.endpoint_id,
        selected->resolved.generation,
        request.activation_source,
        activation.virtual_key,
        request.activation_source == input::ActivationSource::push_to_talk
            ? activation.transition_sequence : 0,
        request.activation_source == input::ActivationSource::push_to_talk
            ? activation.transition_qpc : 0,
    };
    auto endpoint = std::make_unique<InputEndpoint>(
        lease, request.expected_producer_process_id, impl_->ptt_tracker);
    if (!endpoint->start(error)) {
        playback::clear_authentication_token(token);
        playback::clear_authentication_token(lease.one_time_token);
        return std::nullopt;
    }
    if (request.activation_source == input::ActivationSource::push_to_talk) {
        impl_->last_consumed_ptt_press_sequence = activation.transition_sequence;
    }
    impl_->endpoints.push_back(std::move(endpoint));
    playback::clear_authentication_token(token);
    return lease;
}

void InputRehearsalService::cancel_all() noexcept {
    if (!impl_) return;
    std::scoped_lock lock(impl_->mutex);
    for (auto& endpoint : impl_->endpoints) endpoint->cancel();
    impl_->endpoints.clear();
}

bool InputRehearsalService::cancel(const std::string_view stream_id,
                                   const std::uint64_t generation) noexcept {
    std::scoped_lock lock(impl_->mutex);
    const auto found = std::find_if(impl_->endpoints.begin(), impl_->endpoints.end(),
        [&](const auto& endpoint) {
            return endpoint->lease().stream_id == stream_id &&
                   endpoint->lease().generation == generation;
        });
    if (found == impl_->endpoints.end()) return false;
    const bool terminal = (*found)->terminal_receipt_sent();
    (*found)->cancel();
    impl_->endpoints.erase(found);
    return !terminal;
}

void InputRehearsalService::reap_finished() noexcept {
    std::scoped_lock lock(impl_->mutex);
    std::erase_if(impl_->endpoints, [](const auto& endpoint) {
        return endpoint->finished();
    });
}

} // namespace npc::media::windows

#endif
