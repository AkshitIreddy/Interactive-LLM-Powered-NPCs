#ifdef _WIN32

#include "windows_audio.hpp"

#include <Windows.h>
#include <audioclient.h>
#include <avrt.h>
#include <mmdeviceapi.h>
#include <wrl/client.h>

#include <algorithm>
#include <cstring>
#include <mutex>
#include <span>
#include <thread>
#include <utility>
#include <vector>

namespace npc::media::windows {

using Microsoft::WRL::ComPtr;

namespace {

[[nodiscard]] Failure audio_failure(const FailureDomain domain, const HRESULT value, const char* context) {
    const auto code = value == AUDCLNT_E_DEVICE_INVALIDATED ? FailureCode::device_removed
                                                             : FailureCode::backend_unavailable;
    return {domain, code, true,
            std::string(context) + " (HRESULT=" + std::to_string(static_cast<long>(value)) + ")"};
}

[[nodiscard]] PcmFormat describe_format(const WAVEFORMATEX& format) {
    PcmSampleKind kind = PcmSampleKind::signed_integer;
    if (format.wFormatTag == WAVE_FORMAT_IEEE_FLOAT) {
        kind = PcmSampleKind::floating_point;
    } else if (format.wFormatTag == WAVE_FORMAT_PCM && format.wBitsPerSample == 8) {
        kind = PcmSampleKind::unsigned_integer;
    } else if (format.wFormatTag == WAVE_FORMAT_EXTENSIBLE && format.cbSize >= 22) {
        const auto& extensible = reinterpret_cast<const WAVEFORMATEXTENSIBLE&>(format);
        if (extensible.SubFormat == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) {
            kind = PcmSampleKind::floating_point;
        }
    }
    return {format.nSamplesPerSec, format.nChannels, format.wBitsPerSample, format.nBlockAlign, kind};
}

} // namespace


struct EventDrivenAudio::Impl {
    struct Endpoint {
        ComPtr<IMMDevice> device;
        ComPtr<IAudioClient3> client;
        ComPtr<IAudioCaptureClient> capture;
        ComPtr<IAudioRenderClient> render;
        HANDLE ready_event{};
        UINT32 buffer_frames{};
        std::unique_ptr<SharedPcmRing> ring;
        std::vector<std::byte> silence_scratch;
        std::jthread thread;
    };

    ComPtr<IMMDeviceEnumerator> enumerator;
    Endpoint capture_endpoint;
    Endpoint render_endpoint;
    HANDLE shutdown_event{};
    std::mutex failure_mutex;
    std::optional<Failure> failure;

    void record_failure(Failure value) {
        std::scoped_lock lock(failure_mutex);
        if (!failure) {
            failure = std::move(value);
        }
    }

    bool open_endpoint(const EDataFlow flow, Endpoint& endpoint, Failure& out_failure) {
        HRESULT hr = enumerator->GetDefaultAudioEndpoint(flow, eConsole, &endpoint.device);
        if (FAILED(hr)) {
            out_failure = audio_failure(flow == eCapture ? FailureDomain::audio_capture
                                                         : FailureDomain::audio_render,
                                        hr, "Get default WASAPI endpoint failed");
            return false;
        }
        hr = endpoint.device->Activate(__uuidof(IAudioClient3), CLSCTX_INPROC_SERVER, nullptr,
                                       reinterpret_cast<void**>(endpoint.client.GetAddressOf()));
        if (FAILED(hr)) {
            out_failure = audio_failure(flow == eCapture ? FailureDomain::audio_capture
                                                         : FailureDomain::audio_render,
                                        hr, "Activate IAudioClient3 failed");
            return false;
        }

        WAVEFORMATEX* mix_format{};
        hr = endpoint.client->GetMixFormat(&mix_format);
        if (FAILED(hr) || !mix_format) {
            out_failure = audio_failure(flow == eCapture ? FailureDomain::audio_capture
                                                         : FailureDomain::audio_render,
                                        hr, "Get WASAPI mix format failed");
            return false;
        }
        const auto format = describe_format(*mix_format);
        UINT32 default_period{}, fundamental_period{}, minimum_period{}, maximum_period{};
        hr = endpoint.client->GetSharedModeEnginePeriod(mix_format, &default_period, &fundamental_period,
                                                        &minimum_period, &maximum_period);
        if (SUCCEEDED(hr)) {
            hr = endpoint.client->InitializeSharedAudioStream(
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                default_period, mix_format, nullptr);
        }
        CoTaskMemFree(mix_format);
        if (FAILED(hr)) {
            out_failure = audio_failure(flow == eCapture ? FailureDomain::audio_capture
                                                         : FailureDomain::audio_render,
                                        hr, "Initialize event-driven shared WASAPI stream failed");
            return false;
        }
        hr = endpoint.client->GetBufferSize(&endpoint.buffer_frames);
        if (FAILED(hr)) {
            out_failure = audio_failure(flow == eCapture ? FailureDomain::audio_capture
                                                         : FailureDomain::audio_render,
                                        hr, "Get WASAPI buffer size failed");
            return false;
        }
        endpoint.ready_event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!endpoint.ready_event) {
            out_failure = {flow == eCapture ? FailureDomain::audio_capture : FailureDomain::audio_render,
                           FailureCode::backend_unavailable, true, "Create WASAPI event failed"};
            return false;
        }
        hr = endpoint.client->SetEventHandle(endpoint.ready_event);
        if (FAILED(hr)) {
            out_failure = audio_failure(flow == eCapture ? FailureDomain::audio_capture
                                                         : FailureDomain::audio_render,
                                        hr, "Set WASAPI event handle failed");
            return false;
        }
        const auto capacity = std::max<UINT32>(endpoint.buffer_frames * 8, format.sample_rate / 2);
        endpoint.ring = std::make_unique<SharedPcmRing>(format, capacity);
        endpoint.silence_scratch.resize(static_cast<std::size_t>(endpoint.buffer_frames) * format.block_align);

        if (flow == eCapture) {
            hr = endpoint.client->GetService(IID_PPV_ARGS(&endpoint.capture));
        } else {
            hr = endpoint.client->GetService(IID_PPV_ARGS(&endpoint.render));
        }
        if (FAILED(hr)) {
            out_failure = audio_failure(flow == eCapture ? FailureDomain::audio_capture
                                                         : FailureDomain::audio_render,
                                        hr, "Get WASAPI packet service failed");
            return false;
        }
        return true;
    }

    void capture_loop(std::stop_token token) {
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        DWORD task_index{};
        const HANDLE mmcss = AvSetMmThreadCharacteristicsW(L"Pro Audio", &task_index);
        const HANDLE waits[]{shutdown_event, capture_endpoint.ready_event};
        while (!token.stop_requested()) {
            const DWORD wait = WaitForMultipleObjects(2, waits, FALSE, INFINITE);
            if (wait != WAIT_OBJECT_0 + 1) {
                break;
            }
            UINT32 packet_frames{};
            HRESULT hr = capture_endpoint.capture->GetNextPacketSize(&packet_frames);
            while (SUCCEEDED(hr) && packet_frames > 0) {
                BYTE* data{};
                DWORD flags{};
                hr = capture_endpoint.capture->GetBuffer(&data, &packet_frames, &flags, nullptr, nullptr);
                if (FAILED(hr)) {
                    break;
                }
                const auto byte_count = static_cast<std::size_t>(packet_frames) *
                                        capture_endpoint.ring->format().block_align;
                const std::byte* source = reinterpret_cast<const std::byte*>(data);
                if ((flags & AUDCLNT_BUFFERFLAGS_SILENT) != 0 || !data) {
                    std::fill_n(endpoint_silence_data(), byte_count, std::byte{});
                    source = capture_endpoint.silence_scratch.data();
                }
                (void)capture_endpoint.ring->write({source, byte_count}, packet_frames);
                capture_endpoint.capture->ReleaseBuffer(packet_frames);
                hr = capture_endpoint.capture->GetNextPacketSize(&packet_frames);
            }
            if (FAILED(hr)) {
                record_failure(audio_failure(FailureDomain::audio_capture, hr,
                                             "WASAPI capture packet failed"));
                break;
            }
        }
        if (mmcss) {
            AvRevertMmThreadCharacteristics(mmcss);
        }
        CoUninitialize();
    }

    std::byte* endpoint_silence_data() noexcept {
        return capture_endpoint.silence_scratch.data();
    }

    void render_loop(std::stop_token token) {
        CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        DWORD task_index{};
        const HANDLE mmcss = AvSetMmThreadCharacteristicsW(L"Pro Audio", &task_index);
        const HANDLE waits[]{shutdown_event, render_endpoint.ready_event};
        while (!token.stop_requested()) {
            const DWORD wait = WaitForMultipleObjects(2, waits, FALSE, INFINITE);
            if (wait != WAIT_OBJECT_0 + 1) {
                break;
            }
            UINT32 padding{};
            HRESULT hr = render_endpoint.client->GetCurrentPadding(&padding);
            const UINT32 writable = SUCCEEDED(hr) && padding < render_endpoint.buffer_frames
                                        ? render_endpoint.buffer_frames - padding
                                        : 0;
            BYTE* destination{};
            if (writable > 0 && SUCCEEDED(hr)) {
                hr = render_endpoint.render->GetBuffer(writable, &destination);
            }
            if (writable > 0 && SUCCEEDED(hr)) {
                const auto bytes = static_cast<std::size_t>(writable) *
                                   render_endpoint.ring->format().block_align;
                std::memset(destination, 0, bytes);
                (void)render_endpoint.ring->read(
                    {reinterpret_cast<std::byte*>(destination), bytes}, writable);
                hr = render_endpoint.render->ReleaseBuffer(writable, 0);
            }
            if (FAILED(hr)) {
                record_failure(audio_failure(FailureDomain::audio_render, hr,
                                             "WASAPI render packet failed"));
                break;
            }
        }
        if (mmcss) {
            AvRevertMmThreadCharacteristics(mmcss);
        }
        CoUninitialize();
    }

    void close_endpoint(Endpoint& endpoint) noexcept {
        if (endpoint.thread.joinable()) {
            endpoint.thread.request_stop();
        }
        if (shutdown_event) {
            SetEvent(shutdown_event);
        }
        if (endpoint.thread.joinable()) {
            endpoint.thread.join();
        }
        if (endpoint.client) {
            endpoint.client->Stop();
        }
        if (endpoint.ready_event) {
            CloseHandle(endpoint.ready_event);
        }
        endpoint = {};
    }
};

EventDrivenAudio::EventDrivenAudio() : impl_(std::make_unique<Impl>()) {}
EventDrivenAudio::~EventDrivenAudio() { stop(); }

bool EventDrivenAudio::start(Failure& failure) {
    stop();
    HRESULT hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_INPROC_SERVER,
                                  IID_PPV_ARGS(&impl_->enumerator));
    if (FAILED(hr)) {
        failure = audio_failure(FailureDomain::audio_capture, hr,
                                "Create WASAPI device enumerator failed");
        return false;
    }
    impl_->shutdown_event = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (!impl_->shutdown_event) {
        failure = {FailureDomain::audio_capture, FailureCode::backend_unavailable, true,
                   "Create audio shutdown event failed"};
        stop();
        return false;
    }
    if (!impl_->open_endpoint(eCapture, impl_->capture_endpoint, failure) ||
        !impl_->open_endpoint(eRender, impl_->render_endpoint, failure)) {
        stop();
        return false;
    }
    hr = impl_->capture_endpoint.client->Start();
    if (SUCCEEDED(hr)) {
        hr = impl_->render_endpoint.client->Start();
    }
    if (FAILED(hr)) {
        failure = audio_failure(FailureDomain::audio_capture, hr, "Start WASAPI stream failed");
        stop();
        return false;
    }
    impl_->capture_endpoint.thread = std::jthread([this](std::stop_token token) { impl_->capture_loop(token); });
    impl_->render_endpoint.thread = std::jthread([this](std::stop_token token) { impl_->render_loop(token); });
    return true;
}

void EventDrivenAudio::stop() noexcept {
    if (!impl_) {
        return;
    }
    impl_->close_endpoint(impl_->capture_endpoint);
    impl_->close_endpoint(impl_->render_endpoint);
    if (impl_->shutdown_event) {
        CloseHandle(impl_->shutdown_event);
        impl_->shutdown_event = nullptr;
    }
    impl_->enumerator.Reset();
}

bool EventDrivenAudio::restart(Failure& failure) {
    stop();
    return start(failure);
}

SharedPcmRing* EventDrivenAudio::capture_ring() noexcept { return impl_->capture_endpoint.ring.get(); }
SharedPcmRing* EventDrivenAudio::render_ring() noexcept { return impl_->render_endpoint.ring.get(); }

std::optional<Failure> EventDrivenAudio::take_failure() {
    std::scoped_lock lock(impl_->failure_mutex);
    auto result = std::move(impl_->failure);
    impl_->failure.reset();
    return result;
}

} // namespace npc::media::windows

#endif
