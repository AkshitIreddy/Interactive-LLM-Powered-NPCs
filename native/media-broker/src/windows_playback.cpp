#ifdef _WIN32

#include "windows_playback.hpp"
#include "windows_service.hpp"

#include <Windows.h>
#include <audioclient.h>
#include <avrt.h>
#include <bcrypt.h>
#include <propkeydef.h>
#include <propsys.h>
#include <functiondiscoverykeys_devpkey.h>
#include <mmdeviceapi.h>
#include <propvarutil.h>
#include <sddl.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <limits>
#include <mutex>
#include <span>
#include <thread>
#include <utility>
#include <vector>

namespace npc::media::windows {

using Microsoft::WRL::ComPtr;

struct VisualAudioEnvelopeStore {
    mutable std::mutex mutex;
    std::optional<VisualAudioEnvelope> value;
};

namespace {

constexpr auto drain_timeout = std::chrono::seconds(15);
constexpr std::size_t maximum_live_endpoints = 16;
constexpr std::uint64_t allocation_lifetime_seconds = 10ULL * 60ULL;

struct ComApartment final {
    HRESULT result{CoInitializeEx(nullptr, COINIT_MULTITHREADED)};
    ~ComApartment() { if (SUCCEEDED(result)) CoUninitialize(); }
    [[nodiscard]] bool usable() const noexcept {
        return SUCCEEDED(result) || result == RPC_E_CHANGED_MODE;
    }
};

[[nodiscard]] std::string utf8(const std::wstring_view value) {
    if (value.empty()) return {};
    const int size = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                                         static_cast<int>(value.size()), nullptr, 0, nullptr, nullptr);
    if (size <= 0) return {};
    std::string result(static_cast<std::size_t>(size), '\0');
    if (WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), result.data(), size,
                            nullptr, nullptr) != size) return {};
    return result;
}

[[nodiscard]] std::wstring wide(const std::string_view value) {
    if (value.empty()) return {};
    const int size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                         static_cast<int>(value.size()), nullptr, 0);
    if (size <= 0) return {};
    std::wstring result(static_cast<std::size_t>(size), L'\0');
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), result.data(), size) != size) return {};
    return result;
}

[[nodiscard]] std::uint64_t hash_bytes(std::uint64_t hash, const std::string_view value) noexcept {
    constexpr std::uint64_t prime = 1099511628211ULL;
    for (const unsigned char byte : value) {
        hash ^= byte;
        hash *= prime;
    }
    return hash;
}

[[nodiscard]] AudioOutputState output_state(const DWORD state) noexcept {
    if ((state & DEVICE_STATE_ACTIVE) != 0) return AudioOutputState::active;
    if ((state & DEVICE_STATE_DISABLED) != 0) return AudioOutputState::disabled;
    if ((state & DEVICE_STATE_UNPLUGGED) != 0) return AudioOutputState::unplugged;
    return AudioOutputState::not_present;
}

[[nodiscard]] std::optional<AudioOutputEndpoint> describe_output(
    IMMDevice* device, const std::string_view default_endpoint_id) {
    if (!device) return std::nullopt;
    LPWSTR raw_id{};
    if (FAILED(device->GetId(&raw_id)) || !raw_id) return std::nullopt;
    const std::string endpoint_id = utf8(raw_id);
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
    constexpr std::uint64_t offset = 1469598103934665603ULL;
    auto generation = hash_bytes(offset, endpoint_id);
    generation = hash_bytes(generation, friendly_name);
    generation ^= raw_state;
    generation *= 1099511628211ULL;
    if (generation == 0) generation = 1;
    return AudioOutputEndpoint{endpoint_id, friendly_name, output_state(raw_state),
                               endpoint_id == default_endpoint_id, generation};
}

[[nodiscard]] std::optional<AudioOutputSnapshot> enumerate_render_outputs(std::string& error) {
    const ComApartment apartment;
    if (!apartment.usable()) {
        error = "Initialize COM for audio output enumeration failed";
        return std::nullopt;
    }
    ComPtr<IMMDeviceEnumerator> enumerator;
    HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_INPROC_SERVER,
                                  IID_PPV_ARGS(&enumerator));
    if (FAILED(hr)) {
        error = "Create Windows audio endpoint enumerator failed";
        return std::nullopt;
    }
    std::string default_id;
    ComPtr<IMMDevice> default_device;
    if (SUCCEEDED(enumerator->GetDefaultAudioEndpoint(eRender, eConsole, &default_device))) {
        LPWSTR raw_default{};
        if (SUCCEEDED(default_device->GetId(&raw_default)) && raw_default) {
            default_id = utf8(raw_default);
            CoTaskMemFree(raw_default);
        }
    }
    ComPtr<IMMDeviceCollection> collection;
    hr = enumerator->EnumAudioEndpoints(eRender, DEVICE_STATEMASK_ALL, &collection);
    if (FAILED(hr)) {
        error = "Enumerate Windows render endpoints failed";
        return std::nullopt;
    }
    UINT count{};
    if (FAILED(collection->GetCount(&count)) || count > 128U) {
        error = "Windows render endpoint count is invalid";
        return std::nullopt;
    }
    AudioOutputSnapshot snapshot;
    snapshot.endpoints.reserve(count);
    for (UINT index = 0; index < count; ++index) {
        ComPtr<IMMDevice> device;
        if (SUCCEEDED(collection->Item(index, &device))) {
            if (auto endpoint = describe_output(device.Get(), default_id)) {
                snapshot.endpoints.push_back(std::move(*endpoint));
            }
        }
    }
    std::sort(snapshot.endpoints.begin(), snapshot.endpoints.end(),
              [](const auto& left, const auto& right) {
                  return left.endpoint_id < right.endpoint_id;
              });
    constexpr std::uint64_t offset = 1469598103934665603ULL;
    auto catalog = offset;
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

[[nodiscard]] std::optional<SelectedAudioOutput> resolve_output(
    const AudioOutputSelection& selection, std::string& error) {
    if (selection.mode == playback::AudioOutputSelectionMode::endpoint_id &&
        (selection.endpoint_id.empty() ||
         selection.endpoint_id.size() > playback::maximum_endpoint_id_bytes)) {
        error = "Selected Windows audio endpoint ID is malformed";
        return std::nullopt;
    }
    const auto snapshot = enumerate_render_outputs(error);
    if (!snapshot) return std::nullopt;
    const auto match = std::find_if(snapshot->endpoints.begin(), snapshot->endpoints.end(),
        [&](const AudioOutputEndpoint& endpoint) {
            return selection.mode == playback::AudioOutputSelectionMode::system_default
                       ? endpoint.system_default
                       : endpoint.endpoint_id == selection.endpoint_id;
        });
    if (match == snapshot->endpoints.end()) {
        error = selection.mode == playback::AudioOutputSelectionMode::system_default
                    ? "Windows has no current system-default render endpoint"
                    : "Selected Windows audio endpoint is no longer present";
        return std::nullopt;
    }
    if (match->state != AudioOutputState::active) {
        error = "Selected Windows audio endpoint is not active";
        return std::nullopt;
    }
    return SelectedAudioOutput{1, selection, *match};
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
    if (!complete && GetLastError() == ERROR_IO_PENDING &&
        WaitForSingleObject(operation.hEvent, INFINITE) == WAIT_OBJECT_0) {
        complete = GetOverlappedResult(pipe, &operation, &transferred, FALSE) != FALSE;
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
        auto remaining = std::span{const_cast<std::byte*>(input.data() + position), input.size() - position};
        if (!pipe_io(pipe, true, remaining, written) || written == 0) return false;
        position += written;
    }
    return true;
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

[[nodiscard]] std::string random_hex(const std::size_t bytes) {
    std::vector<unsigned char> value(bytes);
    if (!BCRYPT_SUCCESS(BCryptGenRandom(nullptr, value.data(), static_cast<ULONG>(value.size()),
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
    return BCRYPT_SUCCESS(BCryptGenRandom(nullptr,
        reinterpret_cast<PUCHAR>(token.data()), static_cast<ULONG>(token.size()),
        BCRYPT_USE_SYSTEM_PREFERRED_RNG));
}

class WasapiPlayback final {
public:
    ~WasapiPlayback() { stop(); }

    [[nodiscard]] bool start(SharedPcmRing& ring,
                             const playback::PlaybackLease& lease,
                             const std::shared_ptr<VisualAudioEnvelopeStore>& envelope_store,
                             std::string& error) {
        stop();
        ring_ = &ring;
        lease_ = &lease;
        envelope_store_ = envelope_store;
        HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_INPROC_SERVER,
                                      IID_PPV_ARGS(&enumerator_));
        if (FAILED(hr)) return fail(error, "Create WASAPI device enumerator", hr);
        if (lease.output_selection_mode == playback::AudioOutputSelectionMode::system_default) {
            hr = enumerator_->GetDefaultAudioEndpoint(eRender, eConsole, &device_);
            if (FAILED(hr)) return fail(error, "Get selected system-default WASAPI endpoint", hr);
        } else {
            const auto endpoint_id = wide(lease.output_endpoint_id);
            if (endpoint_id.empty()) {
                error = "Selected WASAPI endpoint ID is malformed";
                return false;
            }
            hr = enumerator_->GetDevice(endpoint_id.c_str(), &device_);
            if (FAILED(hr)) return fail(error, "Open selected WASAPI render endpoint", hr);
        }
        const auto current = describe_output(
            device_.Get(), lease.output_selection_mode ==
                                   playback::AudioOutputSelectionMode::system_default
                               ? lease.output_endpoint_id
                               : std::string_view{});
        if (!current || current->endpoint_id != lease.output_endpoint_id ||
            current->generation != lease.output_endpoint_generation ||
            current->state != AudioOutputState::active) {
            error = "Selected WASAPI endpoint was removed or changed after lease allocation";
            return false;
        }
        hr = device_->Activate(__uuidof(IAudioClient3), CLSCTX_INPROC_SERVER, nullptr,
                               reinterpret_cast<void**>(client_.GetAddressOf()));
        if (FAILED(hr)) return fail(error, "Activate WASAPI render client", hr);

        WAVEFORMATEX format{};
        format.wFormatTag = WAVE_FORMAT_PCM;
        format.nChannels = ring.format().channels;
        format.nSamplesPerSec = ring.format().sample_rate;
        format.wBitsPerSample = 16;
        format.nBlockAlign = static_cast<WORD>(format.nChannels * 2);
        format.nAvgBytesPerSec = format.nSamplesPerSec * format.nBlockAlign;
        constexpr DWORD flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK |
                                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM |
                                AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        hr = client_->Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 0, 0, &format, nullptr);
        if (FAILED(hr)) return fail(error, "Initialize shared WASAPI PCM16 stream", hr);
        hr = client_->GetBufferSize(&buffer_frames_);
        if (FAILED(hr)) return fail(error, "Get WASAPI render buffer size", hr);
        ready_event_ = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        shutdown_event_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (!ready_event_ || !shutdown_event_) {
            error = "Create WASAPI playback events failed";
            stop();
            return false;
        }
        hr = client_->SetEventHandle(ready_event_);
        if (FAILED(hr)) return fail(error, "Set WASAPI playback event", hr);
        hr = client_->GetService(IID_PPV_ARGS(&render_));
        if (FAILED(hr)) return fail(error, "Get WASAPI render service", hr);
        hr = client_->Start();
        if (FAILED(hr)) return fail(error, "Start WASAPI playback", hr);
        worker_ = std::jthread([this](const std::stop_token token) { run(token); });
        return true;
    }

    void finish_source() noexcept {
        std::scoped_lock lifecycle_lock(lifecycle_mutex_);
        source_finished_.store(true, std::memory_order_release);
        if (envelope_store_) {
            std::scoped_lock lock(envelope_store_->mutex);
            if (envelope_store_->value && lease_ &&
                envelope_store_->value->stream_id == lease_->stream_id) {
                envelope_store_->value->active = false;
                envelope_store_->value->draining = true;
            }
        }
        if (ready_event_) SetEvent(ready_event_);
    }

    [[nodiscard]] bool wait_for_drain(const std::chrono::milliseconds timeout) {
        std::unique_lock lock(mutex_);
        condition_.wait_for(lock, timeout, [this] {
            return drained_.load(std::memory_order_acquire) ||
                   failed_.load(std::memory_order_acquire) ||
                   cancelled_.load(std::memory_order_acquire);
        });
        return drained_.load(std::memory_order_acquire);
    }

    void cancel() noexcept {
        std::scoped_lock lifecycle_lock(lifecycle_mutex_);
        cancelled_.store(true, std::memory_order_release);
        if (ring_) ring_->clear();
        if (shutdown_event_) SetEvent(shutdown_event_);
        condition_.notify_all();
        stop_client_and_worker();
        clear_envelope();
    }

    void stop() noexcept {
        std::scoped_lock lifecycle_lock(lifecycle_mutex_);
        stop_unlocked();
    }

private:
    void stop_unlocked() noexcept {
        if (ring_ && !drained_.load(std::memory_order_acquire)) ring_->clear();
        if (shutdown_event_) SetEvent(shutdown_event_);
        stop_client_and_worker();
        render_.Reset();
        client_.Reset();
        device_.Reset();
        enumerator_.Reset();
        if (ready_event_) CloseHandle(std::exchange(ready_event_, nullptr));
        if (shutdown_event_) CloseHandle(std::exchange(shutdown_event_, nullptr));
        ring_ = nullptr;
        lease_ = nullptr;
        clear_envelope();
    }

public:
    [[nodiscard]] bool failed() const noexcept { return failed_.load(std::memory_order_acquire); }
    [[nodiscard]] std::uint64_t device_frames() const noexcept {
        return device_frames_.load(std::memory_order_acquire);
    }

private:
    [[nodiscard]] bool fail(std::string& error, const char* context, const HRESULT hr) {
        error = std::string(context) + " (HRESULT=" + std::to_string(static_cast<long>(hr)) + ")";
        stop();
        return false;
    }

    void stop_client_and_worker() noexcept {
        if (worker_.joinable()) worker_.request_stop();
        if (shutdown_event_) SetEvent(shutdown_event_);
        if (worker_.joinable() && worker_.get_id() != std::this_thread::get_id()) worker_.join();
        if (client_) {
            client_->Stop();
            client_->Reset();
        }
    }

    void run(const std::stop_token token) noexcept {
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        DWORD task_index{};
        const HANDLE mmcss = AvSetMmThreadCharacteristicsW(L"Pro Audio", &task_index);
        const HANDLE waits[]{shutdown_event_, ready_event_};
        while (!token.stop_requested()) {
            const auto wait = WaitForMultipleObjects(2, waits, FALSE, INFINITE);
            if (wait != WAIT_OBJECT_0 + 1 || cancelled_.load(std::memory_order_acquire)) break;
            UINT32 padding{};
            HRESULT hr = client_->GetCurrentPadding(&padding);
            if (FAILED(hr)) {
                failed_.store(true, std::memory_order_release);
                break;
            }
            const auto available = ring_ ? ring_->available_frames() : 0U;
            const auto writable = padding < buffer_frames_ ? buffer_frames_ - padding : 0U;
            const auto requested = std::min(available, writable);
            if (requested > 0) {
                BYTE* destination{};
                hr = render_->GetBuffer(requested, &destination);
                if (SUCCEEDED(hr)) {
                    const auto bytes = static_cast<std::size_t>(requested) * ring_->format().block_align;
                    const auto transfer = ring_->read(
                        {reinterpret_cast<std::byte*>(destination), bytes}, requested);
                    if (transfer.transferred_frames < requested) {
                        std::memset(destination + static_cast<std::size_t>(transfer.transferred_frames) *
                                        ring_->format().block_align,
                                    0, static_cast<std::size_t>(requested - transfer.transferred_frames) *
                                        ring_->format().block_align);
                    }
                    const auto source_start = device_frames_.load(std::memory_order_acquire);
                    auto envelope = build_envelope(destination, transfer.transferred_frames,
                                                   source_start);
                    hr = render_->ReleaseBuffer(requested, 0);
                    if (SUCCEEDED(hr)) {
                        publish_envelope(std::move(envelope));
                        device_frames_.fetch_add(transfer.transferred_frames, std::memory_order_release);
                    }
                }
                if (FAILED(hr)) {
                    failed_.store(true, std::memory_order_release);
                    break;
                }
            } else if (source_finished_.load(std::memory_order_acquire) && available == 0 && padding == 0) {
                drained_.store(true, std::memory_order_release);
                clear_envelope();
                break;
            }
        }
        if (mmcss) AvRevertMmThreadCharacteristics(mmcss);
        CoUninitialize();
        condition_.notify_all();
    }

    SharedPcmRing* ring_{};
    const playback::PlaybackLease* lease_{};
    std::shared_ptr<VisualAudioEnvelopeStore> envelope_store_;
    ComPtr<IMMDeviceEnumerator> enumerator_;
    ComPtr<IMMDevice> device_;
    ComPtr<IAudioClient3> client_;
    ComPtr<IAudioRenderClient> render_;
    HANDLE ready_event_{};
    HANDLE shutdown_event_{};
    UINT32 buffer_frames_{};
    std::jthread worker_;
    std::mutex lifecycle_mutex_;
    std::mutex mutex_;
    std::condition_variable condition_;
    std::atomic_bool source_finished_{};
    std::atomic_bool drained_{};
    std::atomic_bool failed_{};
    std::atomic_bool cancelled_{};
    std::atomic<std::uint64_t> device_frames_{};

    void clear_envelope() noexcept {
        if (!envelope_store_) return;
        std::scoped_lock lock(envelope_store_->mutex);
        if (envelope_store_->value && lease_ &&
            envelope_store_->value->stream_id == lease_->stream_id) {
            envelope_store_->value.reset();
        }
    }

    [[nodiscard]] std::optional<VisualAudioEnvelope> build_envelope(
        const BYTE* pcm, const std::uint32_t frames,
        const std::uint64_t source_start) const noexcept {
        if (!envelope_store_ || !lease_ || !pcm || frames == 0 || lease_->channels == 0) {
            return std::nullopt;
        }
        VisualAudioEnvelope envelope;
        envelope.session_id = lease_->session_id;
        envelope.turn_id = lease_->turn_id;
        envelope.stream_id = lease_->stream_id;
        envelope.segment_id = lease_->stream_id;
        envelope.generation = lease_->generation;
        envelope.source_sample_start = source_start;
        envelope.source_sample_count = frames;
        envelope.sample_rate = lease_->sample_rate;
        envelope.channels = lease_->channels;
        envelope.device_write_qpc = qpc_now();
        envelope.qpc_frequency = qpc_frequency();
        envelope.source_frames = source_start + frames;
        envelope.device_frames = source_start + frames;
        envelope.active = true;
        for (std::size_t bin = 0; bin < envelope.mono_rms_q15.size(); ++bin) {
            const auto begin = static_cast<std::uint32_t>(
                static_cast<std::uint64_t>(frames) * bin / envelope.mono_rms_q15.size());
            const auto end = static_cast<std::uint32_t>(
                static_cast<std::uint64_t>(frames) * (bin + 1) / envelope.mono_rms_q15.size());
            long double squares{};
            double peak{};
            std::uint32_t count{};
            for (auto frame = begin; frame < end; ++frame) {
                std::int32_t sum{};
                for (std::uint16_t channel = 0; channel < lease_->channels; ++channel) {
                    std::int16_t sample{};
                    const auto offset = (static_cast<std::size_t>(frame) * lease_->channels + channel) * 2U;
                    std::memcpy(&sample, pcm + offset, sizeof(sample));
                    sum += sample;
                }
                const auto mono = static_cast<double>(sum) /
                                  (32768.0 * static_cast<double>(lease_->channels));
                peak = std::max(peak, std::abs(mono));
                squares += mono * mono;
                ++count;
            }
            const auto rms = count == 0 ? 0.0 : std::sqrt(static_cast<double>(squares / count));
            envelope.mono_rms_q15[bin] = static_cast<std::uint16_t>(
                std::clamp(rms * 32767.0, 0.0, 32767.0));
            envelope.mono_peak_q15[bin] = static_cast<std::uint16_t>(
                std::clamp(peak * 32767.0, 0.0, 32767.0));
        }
        return envelope;
    }

    void publish_envelope(std::optional<VisualAudioEnvelope> envelope) noexcept {
        if (!envelope_store_ || !envelope) return;
        std::scoped_lock lock(envelope_store_->mutex);
        envelope_store_->value = std::move(*envelope);
    }
};

} // namespace

struct PlaybackEndpoint::Impl {
    Impl(playback::PlaybackLease value,
         const std::uint32_t producer_pid,
         const std::uint64_t frequency,
         std::shared_ptr<VisualAudioEnvelopeStore> store)
        : session(value, producer_pid,
                  std::max<std::uint32_t>(session_capacity_frames(session_rate(value), value.max_frames), 1U),
                  {frequency, frequency * 5}),
          expected_producer_process_id(producer_pid), qpc_frequency(frequency),
          envelope_store(std::move(store)) {}

    static std::uint32_t session_rate(const playback::PlaybackLease& lease) noexcept {
        return lease.sample_rate;
    }
    static std::uint32_t session_capacity_frames(const std::uint32_t rate,
                                                 const std::uint64_t maximum) noexcept {
        return static_cast<std::uint32_t>(std::min<std::uint64_t>(maximum, rate * 2ULL));
    }

    playback::PlaybackSession session;
    std::uint32_t expected_producer_process_id{};
    std::uint64_t qpc_frequency{};
    std::shared_ptr<VisualAudioEnvelopeStore> envelope_store;
    HANDLE pipe{INVALID_HANDLE_VALUE};
    std::jthread worker;
    WasapiPlayback device;
    std::atomic_bool finished{};

    [[nodiscard]] bool connect_until_expiry(const std::stop_token token) {
        OVERLAPPED overlapped{};
        overlapped.hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (!overlapped.hEvent) return false;
        const BOOL immediate = ConnectNamedPipe(pipe, &overlapped);
        const DWORD error = immediate ? ERROR_SUCCESS : GetLastError();
        if (!immediate && error != ERROR_IO_PENDING && error != ERROR_PIPE_CONNECTED) {
            CloseHandle(overlapped.hEvent);
            return false;
        }
        bool connected = immediate || error == ERROR_PIPE_CONNECTED;
        while (!connected && !token.stop_requested()) {
            const auto now = qpc_now();
            if (now >= session.lease().expires_qpc) break;
            const auto remaining_ticks = session.lease().expires_qpc - now;
            const auto remaining_ms = qpc_frequency == 0 ? 1ULL :
                std::max<std::uint64_t>(1, remaining_ticks * 1000ULL / qpc_frequency);
            const DWORD wait = WaitForSingleObject(overlapped.hEvent,
                static_cast<DWORD>(std::min<std::uint64_t>(remaining_ms, 100ULL)));
            if (wait == WAIT_OBJECT_0) {
                DWORD transferred{};
                connected = GetOverlappedResult(pipe, &overlapped, &transferred, FALSE) != FALSE;
                break;
            }
            if (wait != WAIT_TIMEOUT) break;
        }
        if (!connected) CancelIoEx(pipe, &overlapped);
        CloseHandle(overlapped.hEvent);
        return connected;
    }

    void run(const std::stop_token token) {
        const ComApartment apartment;
        if (FAILED(apartment.result)) { session.cancel(); finished = true; return; }
        if (!connect_until_expiry(token)) { session.cancel(); finished = true; return; }
        ULONG producer_pid{};
        if (!GetNamedPipeClientProcessId(pipe, &producer_pid) ||
            producer_pid != expected_producer_process_id) {
            session.cancel();
            finished = true;
            DisconnectNamedPipe(pipe);
            return;
        }
        HANDLE producer = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                                      FALSE, producer_pid);
        const bool trusted = producer && process_is_current_user_and_session(producer);
        if (producer) CloseHandle(producer);
        if (!trusted) {
            session.cancel();
            finished = true;
            DisconnectNamedPipe(pipe);
            return;
        }

        while (!token.stop_requested()) {
            std::array<std::byte, 4> prefix{};
            if (!read_exact(pipe, prefix)) break;
            const auto size = playback::decode_frame_size(prefix);
            if (!size) break;
            std::vector<std::byte> body(*size);
            if (!read_exact(pipe, body)) break;
            auto envelope = playback::decode_envelope(body);
            SecureZeroMemory(body.data(), body.size());
            if (!envelope) break;
            const auto command = envelope->command;
            auto response = session.process(*envelope, producer_pid, qpc_now());
            playback::clear_authentication_token(envelope->token);
            if (command == playback::ProducerCommand::begin &&
                response.status == playback::ProducerStatus::ok) {
                std::string error;
                if (!device.start(session.ring(), session.lease(), envelope_store, error)) {
                    session.mark_device_lost();
                    response.status = playback::ProducerStatus::device_unavailable;
                    response.receipt = session.receipt();
                }
            } else if (command == playback::ProducerCommand::finish &&
                       response.status == playback::ProducerStatus::ok) {
                device.finish_source();
                if (device.wait_for_drain(drain_timeout)) {
                    session.record_device_frames(device.device_frames());
                    session.mark_device_drained();
                    response.receipt = session.receipt();
                } else {
                    session.record_device_frames(device.device_frames());
                    if (device.failed()) {
                        session.mark_device_lost();
                        response.status = playback::ProducerStatus::device_unavailable;
                    } else {
                        session.cancel();
                        response.status = playback::ProducerStatus::drain_timeout;
                    }
                    response.receipt = session.receipt();
                }
            } else if (command == playback::ProducerCommand::cancel) {
                device.cancel();
                session.record_device_frames(device.device_frames());
                response.receipt = session.receipt();
            }
            const auto encoded = playback::encode_response(response);
            if (!encoded || !write_exact(pipe, *encoded)) break;
            if (response.receipt || response.status == playback::ProducerStatus::authentication_failed ||
                response.status == playback::ProducerStatus::producer_mismatch ||
                response.status == playback::ProducerStatus::identity_mismatch) break;
        }
        if (session.state() != playback::SessionState::drained &&
            session.state() != playback::SessionState::cancelled &&
            session.state() != playback::SessionState::failed) session.cancel();
        device.cancel();
        FlushFileBuffers(pipe);
        DisconnectNamedPipe(pipe);
        finished = true;
    }
};

PlaybackEndpoint::PlaybackEndpoint(playback::PlaybackLease lease,
                                   const std::uint32_t expected_producer_process_id,
                                   const std::uint64_t qpc_frequency,
                                   std::shared_ptr<VisualAudioEnvelopeStore> envelope_store)
    : impl_(std::make_unique<Impl>(std::move(lease), expected_producer_process_id,
                                  qpc_frequency, std::move(envelope_store))) {}
PlaybackEndpoint::~PlaybackEndpoint() { cancel(); }

bool PlaybackEndpoint::start(std::string& error) {
    if (impl_->pipe != INVALID_HANDLE_VALUE) return true;
    LocalSecurityDescriptor descriptor;
    SECURITY_ATTRIBUTES attributes{};
    if (!current_user_pipe_security(descriptor, attributes)) {
        error = "Build current-user playback pipe ACL failed";
        return false;
    }
    const std::wstring pipe_name(impl_->session.lease().producer_endpoint.begin(),
                                 impl_->session.lease().producer_endpoint.end());
    impl_->pipe = CreateNamedPipeW(pipe_name.c_str(),
        PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
        1, playback::maximum_wire_frame_bytes + 4, playback::maximum_wire_frame_bytes + 4,
        0, &attributes);
    if (impl_->pipe == INVALID_HANDLE_VALUE) {
        error = "Create one-time playback pipe failed";
        return false;
    }
    impl_->worker = std::jthread([state = impl_.get()](const std::stop_token token) {
        state->run(token);
    });
    return true;
}

void PlaybackEndpoint::cancel() noexcept {
    if (!impl_) return;
    if (impl_->worker.joinable()) impl_->worker.request_stop();
    impl_->device.cancel();
    if (impl_->pipe != INVALID_HANDLE_VALUE) {
        CancelIoEx(impl_->pipe, nullptr);
        DisconnectNamedPipe(impl_->pipe);
    }
    if (impl_->worker.joinable() && impl_->worker.get_id() != std::this_thread::get_id()) impl_->worker.join();
    if (impl_->pipe != INVALID_HANDLE_VALUE) CloseHandle(std::exchange(impl_->pipe, INVALID_HANDLE_VALUE));
    impl_->finished = true;
}

bool PlaybackEndpoint::finished() const noexcept { return impl_->finished.load(); }
const playback::PlaybackLease& PlaybackEndpoint::lease() const noexcept { return impl_->session.lease(); }

struct PlaybackService::Impl {
    explicit Impl(std::string session)
        : broker_session_id(std::move(session)),
          envelope_store(std::make_shared<VisualAudioEnvelopeStore>()) {}
    std::string broker_session_id;
    std::mutex mutex;
    std::vector<std::unique_ptr<PlaybackEndpoint>> endpoints;
    AudioOutputSelection selection;
    std::shared_ptr<VisualAudioEnvelopeStore> envelope_store;
};

PlaybackService::PlaybackService(std::string broker_session_id)
    : impl_(std::make_unique<Impl>(std::move(broker_session_id))) {}
PlaybackService::~PlaybackService() { cancel_all(); }

std::optional<playback::PlaybackLease> PlaybackService::allocate(
    const playback::AllocationRequest& request, const std::uint64_t now_qpc,
    const std::uint64_t frequency, std::string& error) {
    if (!playback::valid_allocation(request) || frequency == 0) {
        error = "Playback allocation is malformed or exceeds the bounded PCM policy";
        return std::nullopt;
    }
    reap_finished();
    std::scoped_lock lock(impl_->mutex);
    const auto selected = resolve_output(impl_->selection, error);
    if (!selected) return std::nullopt;
    if (impl_->endpoints.size() >= maximum_live_endpoints) {
        error = "Playback endpoint limit reached";
        return std::nullopt;
    }
    const auto random_id = random_hex(16);
    if (random_id.empty()) {
        error = "Generate playback stream identity failed";
        return std::nullopt;
    }
    playback::AuthenticationToken token{};
    if (!random_token(token)) {
        error = "Generate playback authentication token failed";
        return std::nullopt;
    }
    const std::string stream_id = "pcm-" + random_id;
    const std::string endpoint = R"(\\.\pipe\npc-media-playback-)" +
                                 impl_->broker_session_id + "-" + random_id;
    playback::PlaybackLease lease{
        playback::schema_version, stream_id, endpoint, token, request.session_id,
        request.turn_id, request.generation, request.sample_rate, request.channels,
        request.max_frames, playback::maximum_chunk_bytes,
        now_qpc + frequency * allocation_lifetime_seconds,
        selected->selection.mode, selected->resolved.endpoint_id,
        selected->resolved.generation};
    auto playback_endpoint = std::make_unique<PlaybackEndpoint>(
        lease, request.expected_producer_process_id, frequency, impl_->envelope_store);
    if (!playback_endpoint->start(error)) return std::nullopt;
    impl_->endpoints.push_back(std::move(playback_endpoint));
    return lease;
}

std::optional<AudioOutputSnapshot> PlaybackService::enumerate_audio_outputs(
    std::string& error) const {
    return enumerate_render_outputs(error);
}

std::optional<SelectedAudioOutput> PlaybackService::select_audio_output(
    const AudioOutputSelection& selection, std::string& error) {
    if (selection.mode != playback::AudioOutputSelectionMode::system_default &&
        selection.mode != playback::AudioOutputSelectionMode::endpoint_id) {
        error = "Audio output selection mode is invalid";
        return std::nullopt;
    }
    auto selected = resolve_output(selection, error);
    if (!selected) return std::nullopt;
    std::scoped_lock lock(impl_->mutex);
    if (impl_->selection.mode == selection.mode &&
        impl_->selection.endpoint_id == selection.endpoint_id) {
        return selected;
    }
    for (auto& endpoint : impl_->endpoints) endpoint->cancel();
    impl_->endpoints.clear();
    impl_->selection = selection;
    return selected;
}

std::optional<VisualAudioEnvelope> PlaybackService::query_visual_audio_envelope(
    const std::string_view session_id, const std::string_view turn_id,
    const std::uint64_t generation, const std::string_view stream_id,
    const std::string_view segment_id) const {
    if (session_id.empty() || turn_id.empty() || stream_id.empty() || segment_id.empty() ||
        generation == 0 || !impl_->envelope_store) {
        return std::nullopt;
    }
    std::scoped_lock lock(impl_->envelope_store->mutex);
    const auto& value = impl_->envelope_store->value;
    if (!value || value->session_id != session_id || value->turn_id != turn_id ||
        value->generation != generation || value->stream_id != stream_id ||
        value->segment_id != segment_id || value->cancelled) {
        return std::nullopt;
    }
    return value;
}

std::optional<SelectedAudioOutput> PlaybackService::selected_audio_output(
    std::string& error) const {
    AudioOutputSelection selection;
    {
        std::scoped_lock lock(impl_->mutex);
        selection = impl_->selection;
    }
    return resolve_output(selection, error);
}

void PlaybackService::cancel_before_generation(const std::uint64_t generation) noexcept {
    std::scoped_lock lock(impl_->mutex);
    for (auto& endpoint : impl_->endpoints) {
        if (endpoint->lease().generation < generation) endpoint->cancel();
    }
    std::erase_if(impl_->endpoints, [](const auto& endpoint) { return endpoint->finished(); });
}

void PlaybackService::cancel_all() noexcept {
    if (!impl_) return;
    std::scoped_lock lock(impl_->mutex);
    for (auto& endpoint : impl_->endpoints) endpoint->cancel();
    impl_->endpoints.clear();
}

void PlaybackService::reap_finished() noexcept {
    std::scoped_lock lock(impl_->mutex);
    std::erase_if(impl_->endpoints, [](const auto& endpoint) { return endpoint->finished(); });
}

} // namespace npc::media::windows

#endif
