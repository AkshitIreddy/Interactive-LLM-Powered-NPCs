#ifdef _WIN32

#include "npc/media_broker/geometry.hpp"
#include "npc/media_broker/ipc.hpp"
#include "npc/media_broker/platform.hpp"
#include "windows_audio.hpp"
#include "windows_capture.hpp"
#include "windows_overlay.hpp"
#include "windows_service.hpp"

#include <Windows.h>
#include <d3d11.h>
#include <d3d11_1.h>
#include <dxgi1_2.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <optional>
#include <span>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;
using namespace npc::media;
using namespace npc::media::windows;

COLORREF target_color = RGB(18, 92, 112);

class ScopedComApartment final {
public:
    ScopedComApartment() noexcept
        : result_(CoInitializeEx(nullptr, COINIT_MULTITHREADED)) {}
    ~ScopedComApartment() {
        if (SUCCEEDED(result_)) CoUninitialize();
    }

    ScopedComApartment(const ScopedComApartment&) = delete;
    ScopedComApartment& operator=(const ScopedComApartment&) = delete;

    [[nodiscard]] bool initialized() const noexcept { return SUCCEEDED(result_); }

private:
    HRESULT result_{};
};

LRESULT CALLBACK test_window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_PAINT) {
        PAINTSTRUCT paint{};
        const HDC dc = BeginPaint(window, &paint);
        const HBRUSH brush = CreateSolidBrush(target_color);
        FillRect(dc, &paint.rcPaint, brush);
        DeleteObject(brush);
        EndPaint(window, &paint);
        return 0;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

[[nodiscard]] HWND create_test_window() {
    WNDCLASSEXW definition{};
    definition.cbSize = sizeof(definition);
    definition.lpfnWndProc = test_window_proc;
    definition.hInstance = GetModuleHandleW(nullptr);
    definition.lpszClassName = L"NpcMediaBrokerWgcSmokeTarget";
    definition.hbrBackground = reinterpret_cast<HBRUSH>(COLOR_WINDOW + 1);
    RegisterClassExW(&definition);
    POINT origin{100, 100};
    EnumDisplayMonitors(nullptr, nullptr,
        [](const HMONITOR monitor, HDC, LPRECT, const LPARAM data) -> BOOL {
            MONITORINFO info{sizeof(info)};
            if (GetMonitorInfoW(monitor, &info) && (info.dwFlags & MONITORINFOF_PRIMARY) == 0) {
                auto* selected = reinterpret_cast<POINT*>(data);
                selected->x = info.rcWork.left + 50;
                selected->y = info.rcWork.top + 50;
                return FALSE;
            }
            return TRUE;
        }, reinterpret_cast<LPARAM>(&origin));
    const HWND window = CreateWindowExW(WS_EX_NOACTIVATE, definition.lpszClassName,
                                        L"WGC smoke target",
                                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                                        origin.x, origin.y, 640, 360, nullptr, nullptr,
                                        definition.hInstance, nullptr);
    UpdateWindow(window);
    return window;
}

void pump_messages() {
    MSG message{};
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

struct TransparencyOverlayProvenance {
    HMONITOR target_monitor{};
    std::size_t visible_layered_transparent_windows{};
};

[[nodiscard]] std::size_t count_transparency_app_overlays(const HWND target_window) {
    TransparencyOverlayProvenance provenance{
        MonitorFromWindow(target_window, MONITOR_DEFAULTTONEAREST), 0};
    EnumWindows(
        [](const HWND window, const LPARAM data) -> BOOL {
            auto& provenance = *reinterpret_cast<TransparencyOverlayProvenance*>(data);
            const auto extended_style = static_cast<DWORD_PTR>(GetWindowLongPtrW(window, GWL_EXSTYLE));
            if (!IsWindowVisible(window) ||
                (extended_style & (WS_EX_LAYERED | WS_EX_TRANSPARENT)) !=
                    (WS_EX_LAYERED | WS_EX_TRANSPARENT) ||
                MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) != provenance.target_monitor) {
                return TRUE;
            }
            DWORD process_id{};
            GetWindowThreadProcessId(window, &process_id);
            const HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id);
            if (!process) return TRUE;
            std::wstring path(32768, L'\0');
            DWORD path_size = static_cast<DWORD>(path.size());
            const bool queried = QueryFullProcessImageNameW(process, 0, path.data(), &path_size) != FALSE;
            CloseHandle(process);
            if (!queried) return TRUE;
            path.resize(path_size);
            const auto separator = path.find_last_of(L"\\/");
            const auto* executable_name = path.c_str() +
                                          (separator == std::wstring::npos ? 0U : separator + 1U);
            if (_wcsicmp(executable_name, L"TransparencyApp.exe") == 0) {
                ++provenance.visible_layered_transparent_windows;
            }
            return TRUE;
        },
        reinterpret_cast<LPARAM>(&provenance));
    return provenance.visible_layered_transparent_windows;
}

[[nodiscard]] std::optional<std::uint32_t> read_center_bgra(ID3D11Device* device,
                                                            ID3D11DeviceContext* context,
                                                            ID3D11Texture2D* texture) {
    if (!device || !context || !texture) return std::nullopt;
    D3D11_TEXTURE2D_DESC description{};
    texture->GetDesc(&description);
    if (description.Width == 0 || description.Height == 0 || description.SampleDesc.Count != 1 ||
        description.Format != DXGI_FORMAT_B8G8R8A8_UNORM) {
        return std::nullopt;
    }
    description.BindFlags = 0;
    description.MiscFlags = 0;
    description.Usage = D3D11_USAGE_STAGING;
    description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    ComPtr<ID3D11Texture2D> staging;
    if (FAILED(device->CreateTexture2D(&description, nullptr, &staging))) return std::nullopt;
    context->CopyResource(staging.Get(), texture);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return std::nullopt;
    const auto* row = static_cast<const std::byte*>(mapped.pData) +
                      static_cast<std::size_t>(mapped.RowPitch) * (description.Height / 2U);
    std::uint32_t pixel{};
    std::memcpy(&pixel, row + static_cast<std::size_t>(description.Width / 2U) * 4U,
                sizeof(pixel));
    context->Unmap(staging.Get(), 0);
    return pixel;
}

[[nodiscard]] bool bgra_matches_color(const std::uint32_t pixel,
                                      const COLORREF expected,
                                      const std::uint8_t tolerance = 8U) noexcept {
    const auto within = [tolerance](const std::uint8_t actual, const std::uint8_t wanted) {
        return actual >= wanted ? actual - wanted <= tolerance : wanted - actual <= tolerance;
    };
    return within(static_cast<std::uint8_t>(pixel & 0xffU), GetBValue(expected)) &&
           within(static_cast<std::uint8_t>((pixel >> 8U) & 0xffU), GetGValue(expected)) &&
           within(static_cast<std::uint8_t>((pixel >> 16U) & 0xffU), GetRValue(expected));
}

[[nodiscard]] bool pipe_write(HANDLE pipe, std::span<const std::byte> bytes) {
    std::size_t offset{};
    while (offset < bytes.size()) {
        DWORD written{};
        if (!WriteFile(pipe, bytes.data() + offset, static_cast<DWORD>(bytes.size() - offset), &written, nullptr)) return false;
        offset += written;
    }
    return true;
}

[[nodiscard]] bool pipe_read(HANDLE pipe, std::span<std::byte> bytes) {
    std::size_t offset{};
    while (offset < bytes.size()) {
        DWORD read{};
        if (!ReadFile(pipe, bytes.data() + offset, static_cast<DWORD>(bytes.size() - offset), &read, nullptr)) return false;
        offset += read;
    }
    return true;
}

[[nodiscard]] std::uint64_t pack_file_time(const FILETIME value) noexcept {
    return (static_cast<std::uint64_t>(value.dwHighDateTime) << 32U) |
           static_cast<std::uint64_t>(value.dwLowDateTime);
}

[[nodiscard]] std::uint64_t pack_luid(const LUID value) noexcept {
    return (static_cast<std::uint64_t>(static_cast<std::uint32_t>(value.HighPart)) << 32U) |
           static_cast<std::uint64_t>(value.LowPart);
}

struct ResidualWorkerMetadata {
    std::uint64_t magic{};
    std::uint64_t source_handle_value{};
    std::uint64_t adapter_luid{};
};

inline constexpr std::uint64_t residual_worker_magic = 0x4e50434d4f555448ULL;

[[nodiscard]] int run_residual_worker(const HANDLE output_pipe,
                                      const HANDLE release_event,
                                      const UINT width,
                                      const UINT height) {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    const D3D_FEATURE_LEVEL levels[]{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
    D3D_FEATURE_LEVEL selected{};
    HRESULT hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr,
                                   D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                   levels, ARRAYSIZE(levels), D3D11_SDK_VERSION,
                                   &device, &selected, &context);
    if (FAILED(hr)) return 20;

    ComPtr<IDXGIDevice> dxgi_device;
    ComPtr<IDXGIAdapter> adapter;
    DXGI_ADAPTER_DESC adapter_description{};
    hr = device.As(&dxgi_device);
    if (SUCCEEDED(hr)) hr = dxgi_device->GetAdapter(&adapter);
    if (SUCCEEDED(hr)) hr = adapter->GetDesc(&adapter_description);
    if (FAILED(hr)) return 21;

    D3D11_TEXTURE2D_DESC description{};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_DEFAULT;
    description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    description.MiscFlags = D3D11_RESOURCE_MISC_SHARED_NTHANDLE |
                            D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX;
    ComPtr<ID3D11Texture2D> texture;
    hr = device->CreateTexture2D(&description, nullptr, &texture);
    if (FAILED(hr)) return 22;

    ComPtr<IDXGIResource1> resource;
    ComPtr<IDXGIKeyedMutex> mutex;
    hr = texture.As(&resource);
    if (SUCCEEDED(hr)) hr = texture.As(&mutex);
    HANDLE shared_handle{};
    if (SUCCEEDED(hr)) {
        hr = resource->CreateSharedHandle(nullptr,
                                          DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE,
                                          nullptr, &shared_handle);
    }
    if (FAILED(hr) || !shared_handle) return 23;

    hr = mutex->AcquireSync(0, 1000);
    if (hr != S_OK) {
        CloseHandle(shared_handle);
        return 24;
    }
    std::vector<std::uint32_t> pixels(static_cast<std::size_t>(width) * height, 0x80463010U);
    context->UpdateSubresource(texture.Get(), 0, nullptr, pixels.data(), width * 4U, 0);
    context->Flush();
    hr = mutex->ReleaseSync(1);
    if (FAILED(hr)) {
        CloseHandle(shared_handle);
        return 25;
    }

    const ResidualWorkerMetadata metadata{
        residual_worker_magic,
        static_cast<std::uint64_t>(reinterpret_cast<std::uintptr_t>(shared_handle)),
        pack_luid(adapter_description.AdapterLuid),
    };
    const bool sent = pipe_write(
        output_pipe,
        {reinterpret_cast<const std::byte*>(&metadata), sizeof(metadata)});
    if (sent) WaitForSingleObject(release_event, 10000);
    CloseHandle(shared_handle);
    CloseHandle(output_pipe);
    CloseHandle(release_event);
    return sent ? 0 : 26;
}

struct ResidualWorkerProcess {
    HANDLE process{};
    HANDLE release_event{};
    std::uint32_t process_id{};
    std::uint64_t creation_time{};
    std::string executable_name;
    ResidualWorkerMetadata metadata;
};

struct SharedSourceFixture {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11Device1> device1;
    ComPtr<ID3D11DeviceContext> context;
};

[[nodiscard]] std::optional<SharedSourceFixture> create_shared_source_fixture(
    const std::uint64_t packed_adapter_luid) {
    ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return std::nullopt;
    for (UINT index = 0U;; ++index) {
        ComPtr<IDXGIAdapter1> adapter;
        if (factory->EnumAdapters1(index, &adapter) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_ADAPTER_DESC1 description{};
        if (FAILED(adapter->GetDesc1(&description)) ||
            pack_luid(description.AdapterLuid) != packed_adapter_luid) {
            continue;
        }
        SharedSourceFixture result{};
        constexpr std::array levels{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
        D3D_FEATURE_LEVEL selected{};
        if (FAILED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
                                     D3D11_CREATE_DEVICE_BGRA_SUPPORT, levels.data(),
                                     static_cast<UINT>(levels.size()), D3D11_SDK_VERSION,
                                     &result.device, &selected, &result.context)) ||
            FAILED(result.device.As(&result.device1))) {
            return std::nullopt;
        }
        return result;
    }
    return std::nullopt;
}

[[nodiscard]] bool verify_exact_source_texture(const ResidualWorkerProcess& worker,
                                               const VisualSourceLease& lease) {
    HANDLE local_handle{};
    if (!DuplicateHandle(worker.process,
                         reinterpret_cast<HANDLE>(
                             static_cast<std::uintptr_t>(lease.worker_handle_value)),
                         GetCurrentProcess(), &local_handle, 0U, FALSE,
                         DUPLICATE_SAME_ACCESS | DUPLICATE_CLOSE_SOURCE) ||
        !local_handle) {
        return false;
    }
    const auto close_local = [&local_handle]() {
        if (local_handle) CloseHandle(local_handle);
        local_handle = nullptr;
    };
    auto d3d = create_shared_source_fixture(lease.adapter_luid);
    if (!d3d) {
        close_local();
        return false;
    }
    ComPtr<ID3D11Texture2D> texture;
    const HRESULT opened = d3d->device1->OpenSharedResource1(local_handle,
                                                             IID_PPV_ARGS(&texture));
    close_local();
    if (FAILED(opened)) return false;
    ComPtr<IDXGIKeyedMutex> mutex;
    if (FAILED(texture.As(&mutex)) ||
        mutex->AcquireSync(lease.keyed_mutex_acquire_key, 100U) != S_OK) {
        return false;
    }
    D3D11_TEXTURE2D_DESC description{};
    texture->GetDesc(&description);
    D3D11_TEXTURE2D_DESC staging_description = description;
    staging_description.Usage = D3D11_USAGE_STAGING;
    staging_description.BindFlags = 0U;
    staging_description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    staging_description.MiscFlags = 0U;
    ComPtr<ID3D11Texture2D> staging;
    const HRESULT staging_created = d3d->device->CreateTexture2D(
        &staging_description, nullptr, &staging);
    if (SUCCEEDED(staging_created)) {
        d3d->context->CopyResource(staging.Get(), texture.Get());
    }
    const HRESULT released = mutex->ReleaseSync(lease.keyed_mutex_release_key);
    if (FAILED(staging_created) || FAILED(released)) return false;
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(d3d->context->Map(staging.Get(), 0U, D3D11_MAP_READ, 0U, &mapped))) return false;
    std::uint64_t sampled_energy{};
    const auto row_step = (std::max)(1U, description.Height / 16U);
    const auto column_step = (std::max)(1U, description.Width / 16U);
    for (UINT row = 0U; row < description.Height; row += row_step) {
        const auto* pixels = static_cast<const std::uint8_t*>(mapped.pData) +
                             static_cast<std::size_t>(row) * mapped.RowPitch;
        for (UINT column = 0U; column < description.Width; column += column_step) {
            const auto offset = static_cast<std::size_t>(column) * 4U;
            sampled_energy += pixels[offset] + pixels[offset + 1U] + pixels[offset + 2U];
        }
    }
    d3d->context->Unmap(staging.Get(), 0U);
    return description.Width == lease.width && description.Height == lease.height &&
           description.Format == static_cast<DXGI_FORMAT>(lease.dxgi_format) &&
           (description.MiscFlags & D3D11_RESOURCE_MISC_SHARED_NTHANDLE) != 0U &&
           (description.MiscFlags & D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX) != 0U &&
           sampled_energy > 0U;
}

struct RemoteHandleReuseProbe {
    HANDLE local_event{};
    std::uint64_t remote_handle_value{};
};

void close_remote_probe(const ResidualWorkerProcess& worker,
                        RemoteHandleReuseProbe& probe) noexcept {
    if (probe.remote_handle_value != 0U) {
        HANDLE reclaimed{};
        if (DuplicateHandle(worker.process,
                            reinterpret_cast<HANDLE>(
                                static_cast<std::uintptr_t>(probe.remote_handle_value)),
                            GetCurrentProcess(), &reclaimed, 0U, FALSE,
                            DUPLICATE_SAME_ACCESS | DUPLICATE_CLOSE_SOURCE) && reclaimed) {
            CloseHandle(reclaimed);
        }
    }
    if (probe.local_event) CloseHandle(probe.local_event);
    probe = {};
}

[[nodiscard]] std::optional<RemoteHandleReuseProbe> force_remote_handle_reuse(
    const ResidualWorkerProcess& worker,
    const std::uint64_t released_handle_value) {
    std::vector<RemoteHandleReuseProbe> probes;
    probes.reserve(4096U);
    for (unsigned attempt = 0U; attempt < 4096U; ++attempt) {
        RemoteHandleReuseProbe probe{};
        probe.local_event = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        HANDLE remote{};
        if (!probe.local_event ||
            !DuplicateHandle(GetCurrentProcess(), probe.local_event, worker.process,
                             &remote, 0U, FALSE, DUPLICATE_SAME_ACCESS) || !remote) {
            if (probe.local_event) CloseHandle(probe.local_event);
            for (auto& retained : probes) close_remote_probe(worker, retained);
            return std::nullopt;
        }
        probe.remote_handle_value = reinterpret_cast<std::uint64_t>(remote);
        probes.push_back(probe);
        if (probe.remote_handle_value == released_handle_value) {
            probes.back() = {};
            for (auto& retained : probes) close_remote_probe(worker, retained);
            return probe;
        }
    }
    for (auto& retained : probes) close_remote_probe(worker, retained);
    return std::nullopt;
}

void close_residual_worker(ResidualWorkerProcess& worker, const bool request_exit = true) {
    if (request_exit && worker.release_event) SetEvent(worker.release_event);
    if (worker.process && WaitForSingleObject(worker.process, 5000) != WAIT_OBJECT_0) {
        TerminateProcess(worker.process, 1);
        WaitForSingleObject(worker.process, 5000);
    }
    if (worker.release_event) CloseHandle(worker.release_event);
    if (worker.process) CloseHandle(worker.process);
    worker = {};
}

[[nodiscard]] std::optional<ResidualWorkerProcess> launch_residual_worker(
    const std::wstring& executable,
    const UINT width,
    const UINT height) {
    SECURITY_ATTRIBUTES inheritable{sizeof(inheritable), nullptr, TRUE};
    HANDLE read_pipe{};
    HANDLE write_pipe{};
    if (!CreatePipe(&read_pipe, &write_pipe, &inheritable, 0) ||
        !SetHandleInformation(read_pipe, HANDLE_FLAG_INHERIT, 0)) {
        if (read_pipe) CloseHandle(read_pipe);
        if (write_pipe) CloseHandle(write_pipe);
        return std::nullopt;
    }
    HANDLE release_event = CreateEventW(&inheritable, TRUE, FALSE, nullptr);
    if (!release_event) {
        CloseHandle(read_pipe);
        CloseHandle(write_pipe);
        return std::nullopt;
    }

    std::wstring command = L"\"" + executable + L"\" --residual-worker " +
                           std::to_wstring(reinterpret_cast<std::uintptr_t>(write_pipe)) + L" " +
                           std::to_wstring(reinterpret_cast<std::uintptr_t>(release_event)) + L" " +
                           std::to_wstring(width) + L" " + std::to_wstring(height);
    STARTUPINFOW startup{sizeof(startup)};
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, TRUE,
                        CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process)) {
        CloseHandle(release_event);
        CloseHandle(read_pipe);
        CloseHandle(write_pipe);
        return std::nullopt;
    }
    CloseHandle(process.hThread);
    CloseHandle(write_pipe);

    ResidualWorkerMetadata metadata{};
    const bool received = pipe_read(
        read_pipe,
        {reinterpret_cast<std::byte*>(&metadata), sizeof(metadata)});
    CloseHandle(read_pipe);
    FILETIME created{}, exited{}, kernel{}, user{};
    const bool identified = received && metadata.magic == residual_worker_magic &&
                            metadata.source_handle_value != 0 && metadata.adapter_luid != 0 &&
                            GetProcessTimes(process.hProcess, &created, &exited, &kernel, &user);
    if (!identified) {
        ResidualWorkerProcess failed{process.hProcess, release_event};
        close_residual_worker(failed);
        return std::nullopt;
    }
    return ResidualWorkerProcess{
        process.hProcess,
        release_event,
        process.dwProcessId,
        pack_file_time(created),
        std::filesystem::path(executable).filename().string(),
        metadata,
    };
}

[[nodiscard]] std::optional<ipc::Response> transact(HANDLE pipe, const ipc::Envelope& envelope) {
    const auto encoded = ipc::encode_envelope(envelope);
    if (!encoded) return std::nullopt;
    const auto framed = ipc::frame_message(*encoded);
    if (!pipe_write(pipe, framed)) return std::nullopt;
    std::array<std::byte, 4> prefix{};
    if (!pipe_read(pipe, prefix)) return std::nullopt;
    const auto size = ipc::decode_frame_size(prefix);
    if (!size) return std::nullopt;
    std::vector<std::byte> response(*size);
    return pipe_read(pipe, response) ? ipc::decode_response(response) : std::nullopt;
}

[[nodiscard]] std::optional<playback::ProducerResponse> playback_transact(
    const HANDLE pipe, const playback::ProducerEnvelope& envelope) {
    const auto encoded = playback::encode_envelope(envelope);
    if (!encoded || !pipe_write(pipe, *encoded)) return std::nullopt;
    std::array<std::byte, 4> prefix{};
    if (!pipe_read(pipe, prefix)) return std::nullopt;
    const auto size = playback::decode_frame_size(prefix);
    if (!size) return std::nullopt;
    std::vector<std::byte> response(*size);
    return pipe_read(pipe, response) ? playback::decode_response(response) : std::nullopt;
}

[[nodiscard]] playback::ProducerEnvelope playback_request(
    const playback::PlaybackLease& lease,
    const playback::ProducerCommand command,
    const std::uint64_t sequence,
    std::vector<std::byte> payload = {}) {
    return {playback::schema_version, command, sequence,
            qpc_now() + qpc_frequency() * 3, lease.generation, lease.stream_id,
            lease.session_id, lease.turn_id, lease.one_time_token, std::move(payload)};
}

[[nodiscard]] std::vector<std::byte> quiet_mono_sine(
    const std::uint32_t frames, const std::uint32_t sample_rate) {
    constexpr double tau = 6.28318530717958647692;
    std::vector<std::byte> result(static_cast<std::size_t>(frames) * sizeof(std::int16_t));
    for (std::uint32_t frame = 0; frame < frames; ++frame) {
        const auto sample = static_cast<std::int16_t>(800.0 * std::sin(
            tau * 220.0 * static_cast<double>(frame) / static_cast<double>(sample_rate)));
        result[static_cast<std::size_t>(frame) * 2] =
            static_cast<std::byte>(static_cast<std::uint16_t>(sample) & 0xffU);
        result[static_cast<std::size_t>(frame) * 2 + 1] =
            static_cast<std::byte>((static_cast<std::uint16_t>(sample) >> 8U) & 0xffU);
    }
    return result;
}

template <typename T>
[[nodiscard]] std::optional<T> read_little(const std::span<const std::byte> payload,
                                           const std::size_t offset) {
    static_assert(std::is_unsigned_v<T>);
    if (offset > payload.size() || payload.size() - offset < sizeof(T)) return std::nullopt;
    T value{};
    for (std::size_t index = 0; index < sizeof(T); ++index) {
        value |= static_cast<T>(std::to_integer<std::uint8_t>(payload[offset + index])) << (index * 8U);
    }
    return value;
}

struct CaptureDiagnosticsEvidence {
    BrokerState state{BrokerState::stopped};
    CaptureBackend capture_backend{CaptureBackend::none};
    TargetState target_state{TargetState::none};
    std::uint64_t frames_received{};
    std::uint64_t frames_presented{};
    std::uint64_t patches_presented{};
};

struct NativeCaptureEvidence {
    std::uint32_t process_id{};
    std::uint64_t native_window{};
    std::uint64_t device_generation{};
    std::uint64_t geometry_epoch{};
    std::uint64_t latest_sequence{};
    std::uint64_t latest_qpc{};
    std::uint64_t initial_hash{};
    std::uint64_t latest_hash{};
    std::uint64_t hash_changes{};
    std::uint64_t geometry_changes{};
    SizeI size_px;
    bool overlay_capture_excluded{};
    bool overlay_visuals_allowed{};
    CapturePixelSource pixel_source{CapturePixelSource::unavailable};
    CapturePixelScope pixel_scope{CapturePixelScope::unavailable};
    bool external_display_overlay_pixels_excluded{};
    bool desktop_luminance_excluded_from_pixel_evidence{};
    bool external_display_overlays_may_change_perceived_brightness{};
    std::string executable_name;
};

[[nodiscard]] std::optional<NativeCaptureEvidence> decode_native_capture_evidence(
    const std::span<const std::byte> payload) {
    const auto schema = read_little<std::uint32_t>(payload, 0);
    const auto process_id = read_little<std::uint32_t>(payload, 4);
    const auto native_window = read_little<std::uint64_t>(payload, 8);
    const auto device_generation = read_little<std::uint64_t>(payload, 16);
    const auto geometry_epoch = read_little<std::uint64_t>(payload, 24);
    const auto latest_sequence = read_little<std::uint64_t>(payload, 32);
    const auto latest_qpc = read_little<std::uint64_t>(payload, 40);
    const auto initial_hash = read_little<std::uint64_t>(payload, 48);
    const auto latest_hash = read_little<std::uint64_t>(payload, 56);
    const auto hash_changes = read_little<std::uint64_t>(payload, 64);
    const auto geometry_changes = read_little<std::uint64_t>(payload, 72);
    const auto width = read_little<std::uint32_t>(payload, 88);
    const auto height = read_little<std::uint32_t>(payload, 92);
    const auto excluded = read_little<std::uint32_t>(payload, 96);
    const auto visuals = read_little<std::uint32_t>(payload, 100);
    const auto name_size = read_little<std::uint32_t>(payload, 104);
    const auto pixel_source = read_little<std::uint32_t>(payload, 108);
    const auto pixel_scope = read_little<std::uint32_t>(payload, 112);
    const auto external_overlay_excluded = read_little<std::uint32_t>(payload, 116);
    const auto desktop_luminance_excluded = read_little<std::uint32_t>(payload, 120);
    const auto perceived_brightness_caveat = read_little<std::uint32_t>(payload, 124);
    if (!schema || *schema != 3 || !process_id || !native_window || !device_generation ||
        !geometry_epoch ||
        !latest_sequence || !latest_qpc || !initial_hash || !latest_hash || !hash_changes ||
        !geometry_changes || !width || !height || !excluded || !visuals || !name_size ||
        !pixel_source || !pixel_scope || !external_overlay_excluded || !desktop_luminance_excluded ||
        !perceived_brightness_caveat ||
        *pixel_source < static_cast<std::uint32_t>(CapturePixelSource::windows_graphics_capture_texture) ||
        *pixel_source > static_cast<std::uint32_t>(CapturePixelSource::desktop_duplication_texture) ||
        *pixel_scope < static_cast<std::uint32_t>(CapturePixelScope::exact_selected_window) ||
        *pixel_scope > static_cast<std::uint32_t>(CapturePixelScope::full_display_output) ||
        *external_overlay_excluded > 1U || *desktop_luminance_excluded > 1U ||
        *perceived_brightness_caveat != 1U ||
        *name_size > 260 || payload.size() != 128 + *name_size) {
        return std::nullopt;
    }
    std::string executable_name;
    executable_name.reserve(*name_size);
    for (std::size_t index = 0; index < *name_size; ++index) {
        executable_name.push_back(static_cast<char>(std::to_integer<unsigned char>(payload[128 + index])));
    }
    return NativeCaptureEvidence{*process_id, *native_window, *device_generation, *geometry_epoch,
                                 *latest_sequence, *latest_qpc, *initial_hash, *latest_hash,
                                 *hash_changes, *geometry_changes,
                                 {static_cast<std::int32_t>(*width), static_cast<std::int32_t>(*height)},
                                 *excluded != 0, *visuals != 0,
                                 static_cast<CapturePixelSource>(*pixel_source),
                                 static_cast<CapturePixelScope>(*pixel_scope),
                                 *external_overlay_excluded != 0, *desktop_luminance_excluded != 0,
                                 *perceived_brightness_caveat != 0,
                                 std::move(executable_name)};
}

[[nodiscard]] std::optional<CaptureDiagnosticsEvidence> decode_capture_evidence(
    const std::span<const std::byte> payload) {
    const auto state = read_little<std::uint32_t>(payload, 0);
    const auto capture_backend = read_little<std::uint32_t>(payload, 4);
    const auto target_state = read_little<std::uint32_t>(payload, 20);
    const auto frames_received = read_little<std::uint64_t>(payload, 48);
    const auto frames_presented = read_little<std::uint64_t>(payload, 56);
    const auto patches_presented = read_little<std::uint64_t>(payload, 88);
    if (!state || !capture_backend || !target_state || !frames_received || !frames_presented ||
        !patches_presented ||
        *state > static_cast<std::uint32_t>(BrokerState::failed) ||
        *capture_backend > static_cast<std::uint32_t>(CaptureBackend::desktop_duplication) ||
        *target_state > static_cast<std::uint32_t>(TargetState::unsupported)) {
        return std::nullopt;
    }
    return CaptureDiagnosticsEvidence{
        static_cast<BrokerState>(*state),
        static_cast<CaptureBackend>(*capture_backend),
        static_cast<TargetState>(*target_state),
        *frames_received,
        *frames_presented,
        *patches_presented,
    };
}

struct ChildService {
    PROCESS_INFORMATION process{};
    HANDLE job{};
    HANDLE pipe{INVALID_HANDLE_VALUE};
    std::string session;
    std::array<std::byte, ipc::launch_nonce_bytes> nonce{};
};

[[nodiscard]] std::optional<ChildService> launch_service(const std::wstring& executable,
                                                         const std::uint32_t suffix) {
    ChildService child;
    child.session = "service-smoke-" + std::to_string(GetCurrentProcessId()) + "-" + std::to_string(suffix);
    std::string nonce_hex;
    constexpr char hex[] = "0123456789abcdef";
    for (std::size_t index = 0; index < child.nonce.size(); ++index) {
        const auto value = static_cast<std::uint8_t>((index + suffix) & 0xffU);
        child.nonce[index] = static_cast<std::byte>(value);
        nonce_hex.push_back(hex[value >> 4U]);
        nonce_hex.push_back(hex[value & 0x0fU]);
    }
    std::wstring command = L"\"" + executable + L"\" --parent-pid=" + std::to_wstring(GetCurrentProcessId()) +
                           L" --session=" + std::wstring(child.session.begin(), child.session.end()) +
                           L" --nonce=" + std::wstring(nonce_hex.begin(), nonce_hex.end());
    STARTUPINFOW startup{sizeof(startup)};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE,
                        CREATE_SUSPENDED | CREATE_NO_WINDOW, nullptr, nullptr, &startup, &child.process)) return std::nullopt;
    child.job = CreateJobObjectW(nullptr, nullptr);
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (!child.job || !SetInformationJobObject(child.job, JobObjectExtendedLimitInformation, &limits, sizeof(limits)) ||
        !AssignProcessToJobObject(child.job, child.process.hProcess)) {
        TerminateProcess(child.process.hProcess, 1);
        return std::nullopt;
    }
    ResumeThread(child.process.hThread);
    CloseHandle(child.process.hThread);
    child.process.hThread = nullptr;
    const std::wstring pipe_name = L"\\\\.\\pipe\\npc-media-broker-" +
                                   std::wstring(child.session.begin(), child.session.end());
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(8);
    DWORD last_pipe_error{};
    while (std::chrono::steady_clock::now() < deadline) {
        child.pipe = CreateFileW(pipe_name.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                                 OPEN_EXISTING, 0, nullptr);
        if (child.pipe != INVALID_HANDLE_VALUE) return child;
        last_pipe_error = GetLastError();
        if (WaitForSingleObject(child.process.hProcess, 0) == WAIT_OBJECT_0) {
            DWORD exit_code{};
            GetExitCodeProcess(child.process.hProcess, &exit_code);
            std::cerr << "media service exited before control-pipe connection: exit=" << exit_code
                      << " pipe_error=" << last_pipe_error << '\n';
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    if (WaitForSingleObject(child.process.hProcess, 0) != WAIT_OBJECT_0) {
        std::cerr << "media service control-pipe connection timed out: pipe_error="
                  << last_pipe_error << '\n';
    }
    TerminateProcess(child.process.hProcess, 1);
    return std::nullopt;
}

void close_child(ChildService& child) {
    if (child.pipe != INVALID_HANDLE_VALUE) CloseHandle(child.pipe);
    if (child.process.hProcess) CloseHandle(child.process.hProcess);
    if (child.job) CloseHandle(child.job);
    child = {};
}

[[nodiscard]] bool authenticated_playback_smoke(ChildService& child,
                                                ipc::Envelope& request,
                                                const std::uint64_t frequency) {
    constexpr std::uint32_t sample_rate = 24'000;
    constexpr std::uint64_t frames = 2'400;
    const auto control = [&](const ipc::CommandKind kind, const ipc::Command& command)
        -> std::optional<ipc::Response> {
        const auto payload = ipc::encode_command(kind, command);
        if (!payload) return std::nullopt;
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = kind;
        request.payload = *payload;
        return transact(child.pipe, request);
    };
    const auto outputs_response = control(
        ipc::CommandKind::enumerate_audio_outputs, ipc::EnumerateAudioOutputsCommand{});
    const auto outputs = outputs_response && outputs_response->status == ipc::StatusCode::ok
        ? ipc::decode_audio_output_snapshot(outputs_response->payload)
        : std::nullopt;
    if (!outputs || outputs->catalog_generation == 0 || outputs->endpoints.empty()) {
        std::cerr << "authenticated audio-output enumeration failed\n";
        return false;
    }
    const auto default_output = std::find_if(
        outputs->endpoints.begin(), outputs->endpoints.end(), [](const auto& endpoint) {
            return endpoint.system_default && endpoint.state == ipc::AudioOutputState::active;
        });
    if (default_output == outputs->endpoints.end()) return false;
    const auto default_selection = control(
        ipc::CommandKind::select_audio_output,
        ipc::SelectAudioOutputCommand{playback::AudioOutputSelectionMode::system_default, {}});
    if (!default_selection || default_selection->status != ipc::StatusCode::ok) return false;
    const auto explicit_selection = control(
        ipc::CommandKind::select_audio_output,
        ipc::SelectAudioOutputCommand{playback::AudioOutputSelectionMode::endpoint_id,
                                      default_output->endpoint_id});
    const auto selected = explicit_selection &&
                                  explicit_selection->status == ipc::StatusCode::ok
        ? ipc::decode_selected_audio_output(explicit_selection->payload)
        : std::nullopt;
    if (!selected || selected->requested_endpoint_id != default_output->endpoint_id ||
        selected->resolved.generation != default_output->generation) return false;
    const auto selected_query = control(
        ipc::CommandKind::selected_audio_output, ipc::SelectedAudioOutputCommand{});
    const auto selected_again = selected_query && selected_query->status == ipc::StatusCode::ok
        ? ipc::decode_selected_audio_output(selected_query->payload)
        : std::nullopt;
    if (!selected_again || selected_again->resolved.endpoint_id != default_output->endpoint_id) {
        return false;
    }
    const auto allocate = [&](const std::string& turn_id,
                              const std::uint64_t generation)
        -> std::optional<playback::PlaybackLease> {
        const auto payload = ipc::encode_command(
            ipc::CommandKind::allocate_playback_stream,
            ipc::AllocatePlaybackStreamCommand{child.session, turn_id, generation, sample_rate,
                                                1, frames, GetCurrentProcessId()});
        if (!payload) return std::nullopt;
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::allocate_playback_stream;
        request.payload = *payload;
        const auto response = transact(child.pipe, request);
        if (!response || response->status != ipc::StatusCode::ok) return std::nullopt;
        return ipc::decode_playback_lease(response->payload);
    };
    const auto connect = [](const playback::PlaybackLease& lease) {
        const std::wstring endpoint(lease.producer_endpoint.begin(), lease.producer_endpoint.end());
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
        HANDLE pipe = INVALID_HANDLE_VALUE;
        while (std::chrono::steady_clock::now() < deadline) {
            pipe = CreateFileW(endpoint.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                               OPEN_EXISTING, 0, nullptr);
            if (pipe != INVALID_HANDLE_VALUE) break;
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
        return pipe;
    };

    const auto lease = allocate("authenticated-pcm-finish", 1);
    if (!lease || lease->session_id != child.session || lease->sample_rate != sample_rate ||
        lease->channels != 1 || lease->max_frames != frames ||
        lease->max_chunk_bytes != playback::maximum_chunk_bytes ||
        lease->output_selection_mode != playback::AudioOutputSelectionMode::endpoint_id ||
        lease->output_endpoint_id != default_output->endpoint_id ||
        lease->output_endpoint_generation != default_output->generation) {
        std::cerr << "broker playback allocation response was unavailable or unbound\n";
        return false;
    }
    HANDLE producer = connect(*lease);
    if (producer == INVALID_HANDLE_VALUE) {
        std::cerr << "broker-issued playback endpoint was unavailable\n";
        return false;
    }
    const auto begin = playback_transact(
        producer, playback_request(*lease, playback::ProducerCommand::begin, 1));
    const auto chunk = begin && begin->status == playback::ProducerStatus::ok
        ? playback_transact(producer, playback_request(
              *lease, playback::ProducerCommand::chunk, 2,
              quiet_mono_sine(static_cast<std::uint32_t>(frames), sample_rate)))
        : std::nullopt;
    const auto finish = chunk && chunk->status == playback::ProducerStatus::ok &&
                                chunk->accepted_source_frames == frames
        ? playback_transact(producer,
                            playback_request(*lease, playback::ProducerCommand::finish, 3))
        : std::nullopt;
    CloseHandle(producer);
    if (!finish || finish->status != playback::ProducerStatus::ok || !finish->receipt) {
        std::cerr << "authenticated broker playback Begin/Chunk/Finish failed\n";
        return false;
    }
    const auto& receipt = *finish->receipt;
    if (receipt.stream_id != lease->stream_id || receipt.session_id != child.session ||
        receipt.turn_id != lease->turn_id || receipt.generation != lease->generation ||
        receipt.source_frames != frames || receipt.device_frames != frames ||
        receipt.source_duration_micros != 100'000 || !receipt.source_submission_complete ||
        !receipt.endpoint_drain_complete || receipt.cancelled ||
        receipt.output_selection_mode != lease->output_selection_mode ||
        receipt.output_endpoint_id != lease->output_endpoint_id ||
        receipt.output_endpoint_generation != lease->output_endpoint_generation) {
        std::cerr << "authenticated broker playback receipt was not exact: source="
                  << receipt.source_frames << " device=" << receipt.device_frames
                  << " duration_us=" << receipt.source_duration_micros
                  << " source_complete=" << receipt.source_submission_complete
                  << " drained=" << receipt.endpoint_drain_complete
                  << " cancelled=" << receipt.cancelled << '\n';
        return false;
    }

    const auto cancelled_lease = allocate("authenticated-pcm-cancel", 2);
    if (!cancelled_lease) return false;
    producer = connect(*cancelled_lease);
    if (producer == INVALID_HANDLE_VALUE) return false;
    const auto cancel_begin = playback_transact(
        producer, playback_request(*cancelled_lease, playback::ProducerCommand::begin, 1));
    const auto cancelled = cancel_begin && cancel_begin->status == playback::ProducerStatus::ok
        ? playback_transact(producer,
                            playback_request(*cancelled_lease, playback::ProducerCommand::cancel, 2))
        : std::nullopt;
    CloseHandle(producer);
    if (!cancelled || cancelled->status != playback::ProducerStatus::cancelled ||
        !cancelled->receipt || !cancelled->receipt->cancelled ||
        cancelled->receipt->source_submission_complete ||
        cancelled->receipt->endpoint_drain_complete) {
        std::cerr << "authenticated broker playback Cancel receipt failed\n";
        return false;
    }

    const auto cancel_payload = ipc::encode_command(
        ipc::CommandKind::cancel_playback, ipc::CancelPlaybackCommand{});
    if (!cancel_payload) return false;
    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::cancel_playback;
    request.payload = *cancel_payload;
    const auto cancel_response = transact(child.pipe, request);
    return cancel_response && cancel_response->status == ipc::StatusCode::ok;
}

[[nodiscard]] bool visual_worker_crash_preserves_live_playback(
    ChildService& child,
    ipc::Envelope& request,
    const std::uint64_t frequency,
    ResidualWorkerProcess& visual_worker) {
    constexpr std::uint32_t sample_rate = 24'000U;
    constexpr std::uint64_t frames = 2'400U;
    const auto payload = ipc::encode_command(
        ipc::CommandKind::allocate_playback_stream,
        ipc::AllocatePlaybackStreamCommand{
            child.session,
            "visual-worker-crash-audio-survives",
            3U,
            sample_rate,
            1U,
            frames,
            GetCurrentProcessId(),
        });
    if (!payload) return false;
    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2U;
    request.command = ipc::CommandKind::allocate_playback_stream;
    request.payload = *payload;
    const auto allocation = transact(child.pipe, request);
    const auto lease = allocation && allocation->status == ipc::StatusCode::ok
        ? ipc::decode_playback_lease(allocation->payload)
        : std::nullopt;
    if (!lease) return false;
    const std::wstring endpoint(lease->producer_endpoint.begin(), lease->producer_endpoint.end());
    HANDLE producer = INVALID_HANDLE_VALUE;
    const auto connect_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (producer == INVALID_HANDLE_VALUE &&
           std::chrono::steady_clock::now() < connect_deadline) {
        producer = CreateFileW(endpoint.c_str(), GENERIC_READ | GENERIC_WRITE, 0U, nullptr,
                               OPEN_EXISTING, 0U, nullptr);
        if (producer == INVALID_HANDLE_VALUE) std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (producer == INVALID_HANDLE_VALUE) return false;
    const auto begin = playback_transact(
        producer, playback_request(*lease, playback::ProducerCommand::begin, 1U));
    const auto chunk = begin && begin->status == playback::ProducerStatus::ok
        ? playback_transact(producer, playback_request(
              *lease, playback::ProducerCommand::chunk, 2U,
              quiet_mono_sine(static_cast<std::uint32_t>(frames), sample_rate)))
        : std::nullopt;
    if (!chunk || chunk->status != playback::ProducerStatus::ok ||
        chunk->accepted_source_frames != frames) {
        CloseHandle(producer);
        return false;
    }

    // This models the optional mouth worker dying after it consumed its exact
    // source lease but while the independently leased PCM stream is live. No
    // global Control Cancel or CancelPlayback command is sent.
    close_residual_worker(visual_worker, false);
    const auto finish = playback_transact(
        producer, playback_request(*lease, playback::ProducerCommand::finish, 3U));
    CloseHandle(producer);
    if (!finish || finish->status != playback::ProducerStatus::ok || !finish->receipt) return false;
    const auto& receipt = *finish->receipt;
    return receipt.stream_id == lease->stream_id && receipt.session_id == child.session &&
           receipt.turn_id == lease->turn_id && receipt.generation == lease->generation &&
           receipt.source_frames == frames && receipt.device_frames == frames &&
           receipt.source_duration_micros == 100'000U &&
           receipt.source_submission_complete && receipt.endpoint_drain_complete &&
           !receipt.cancelled;
}

[[nodiscard]] bool service_lifecycle_smoke(const std::wstring& executable, const HWND target_window) {
    {
        STARTUPINFOW startup{sizeof(startup)};
        PROCESS_INFORMATION malformed{};
        std::wstring command = L"\"" + executable + L"\"";
        if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE,
                            CREATE_NO_WINDOW, nullptr, nullptr, &startup, &malformed)) return false;
        CloseHandle(malformed.hThread);
        const bool rejected = WaitForSingleObject(malformed.hProcess, 5000) == WAIT_OBJECT_0;
        DWORD exit_code{};
        GetExitCodeProcess(malformed.hProcess, &exit_code);
        CloseHandle(malformed.hProcess);
        if (!rejected || exit_code == 0) {
            std::cerr << "malformed service launch was not rejected: waited=" << rejected
                      << " exit=" << exit_code << '\n';
            return false;
        }
    }

    auto child_value = launch_service(executable, 1);
    if (!child_value) {
        std::cerr << "authenticated service launch failed before health exchange\n";
        return false;
    }
    auto child = std::move(*child_value);
    const auto fail_child = [&child](const std::string_view stage) {
        std::cerr << "authenticated service protocol failed at " << stage << '\n';
        close_child(child);
        return false;
    };
    const auto frequency = qpc_frequency();
    const auto now = qpc_now();
    const auto empty = ipc::encode_command(ipc::CommandKind::health, ipc::HealthCommand{});
    ipc::Envelope request{ipc::protocol_version, child.nonce, child.session, 1,
                          now + frequency * 2, 0, ipc::CommandKind::health, *empty};
    request.nonce[0] ^= std::byte{0xff};
    const auto rejected = transact(child.pipe, request);
    if (!rejected || rejected->status != ipc::StatusCode::authentication_failed) {
        return fail_child("nonce rejection");
    }
    request.nonce = child.nonce;
    request.sequence = 2;
    request.deadline_qpc = qpc_now() + frequency * 2;
    const auto health = transact(child.pipe, request);
    if (!health || health->status != ipc::StatusCode::ok) return fail_child("authenticated health");
    if (!authenticated_playback_smoke(child, request, frequency)) {
        return fail_child("authenticated playback allocation and drain");
    }

    const auto inspected = inspect_target(target_window, GetCurrentProcessId());
    if (!inspected.valid_window || !inspected.process_id_matches ||
        !inspected.inspection_complete || inspected.process_name.empty()) {
        std::cerr << "synthetic target preflight incomplete: valid=" << inspected.valid_window
                  << " pid_match=" << inspected.process_id_matches
                  << " inspection_complete=" << inspected.inspection_complete
                  << " process_name_empty=" << inspected.process_name.empty() << '\n';
        return fail_child("synthetic target preflight");
    }
    const auto blocked_select = [&](const std::uint32_t process_id,
                                    std::vector<std::string> allowed,
                                    const TargetBlockReason expected_reason,
                                    const std::string_view stage) {
        const auto payload = ipc::encode_command(
            ipc::CommandKind::select_target,
            ipc::SelectTargetCommand{reinterpret_cast<std::uintptr_t>(target_window),
                                     process_id, std::move(allowed)});
        if (!payload) return false;
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::select_target;
        request.payload = *payload;
        const auto response = transact(child.pipe, request);
        const auto reason = response ? read_little<std::uint32_t>(response->payload, 0) : std::nullopt;
        if (!response || response->status != ipc::StatusCode::target_blocked || !reason ||
            *reason != static_cast<std::uint32_t>(expected_reason) ||
            response->cancellation_generation <= request.cancellation_generation) {
            std::cerr << "blocked SelectTarget failed at " << stage << '\n';
            return false;
        }
        request.cancellation_generation = response->cancellation_generation;
        return true;
    };
    if (!blocked_select(GetCurrentProcessId() + 1, {inspected.process_name},
                        TargetBlockReason::process_mismatch, "exact PID rejection")) {
        return fail_child("exact PID rejection");
    }
    if (!blocked_select(GetCurrentProcessId(), {"definitely-not-the-owner.exe"},
                        TargetBlockReason::not_allowlisted, "exact executable rejection")) {
        return fail_child("exact executable rejection");
    }

    const auto select_payload = ipc::encode_command(
        ipc::CommandKind::select_target,
        ipc::SelectTargetCommand{reinterpret_cast<std::uintptr_t>(target_window),
                                 GetCurrentProcessId(),
                                 {inspected.process_name}});
    if (!select_payload) return fail_child("SelectTarget encoding");
    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::select_target;
    request.payload = *select_payload;
    const auto selected = transact(child.pipe, request);
    if (!selected || selected->status != ipc::StatusCode::ok ||
        selected->cancellation_generation <= request.cancellation_generation) {
        if (selected) {
            std::cerr << "SelectTarget response: status=" << ipc::to_string(selected->status)
                      << " request_generation=" << request.cancellation_generation
                      << " response_generation=" << selected->cancellation_generation;
            if (const auto reason = read_little<std::uint32_t>(selected->payload, 0)) {
                std::cerr << " block_reason=" << *reason;
            }
            std::cerr << '\n';
        }
        return fail_child("SelectTarget response");
    }
    request.cancellation_generation = selected->cancellation_generation;

    const auto diagnostics_payload = ipc::encode_command(ipc::CommandKind::diagnostics,
                                                         ipc::DiagnosticsCommand{});
    const auto native_evidence_payload = ipc::encode_command(
        ipc::CommandKind::capture_evidence, ipc::CaptureEvidenceCommand{});
    std::optional<CaptureDiagnosticsEvidence> capture_evidence;
    std::optional<NativeCaptureEvidence> native_evidence;
    const auto capture_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(8);
    while (std::chrono::steady_clock::now() < capture_deadline) {
        target_color = target_color == RGB(18, 92, 112) ? RGB(116, 34, 72) : RGB(18, 92, 112);
        InvalidateRect(target_window, nullptr, FALSE);
        UpdateWindow(target_window);
        pump_messages();
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::diagnostics;
        request.payload = *diagnostics_payload;
        const auto diagnostics = transact(child.pipe, request);
        if (!diagnostics || diagnostics->status != ipc::StatusCode::ok) {
            return fail_child("capture diagnostics exchange");
        }
        capture_evidence = decode_capture_evidence(diagnostics->payload);
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::capture_evidence;
        request.payload = *native_evidence_payload;
        const auto evidence_response = transact(child.pipe, request);
        native_evidence = evidence_response && evidence_response->status == ipc::StatusCode::ok
                              ? decode_native_capture_evidence(evidence_response->payload)
                              : std::nullopt;
        if (capture_evidence && capture_evidence->state == BrokerState::capturing_primary &&
            capture_evidence->capture_backend == CaptureBackend::windows_graphics_capture &&
            capture_evidence->target_state == TargetState::selected &&
            capture_evidence->frames_received >= 2 && capture_evidence->frames_presented >= 1 &&
            native_evidence && native_evidence->process_id == GetCurrentProcessId() &&
            native_evidence->native_window == reinterpret_cast<std::uintptr_t>(target_window) &&
            native_evidence->executable_name == inspected.process_name &&
            native_evidence->device_generation > 0 && native_evidence->geometry_epoch > 0 &&
            native_evidence->latest_sequence > 1 &&
            native_evidence->latest_qpc > 0 && native_evidence->initial_hash > 0 &&
            native_evidence->latest_hash > 0 && native_evidence->hash_changes > 0 &&
            native_evidence->geometry_changes > 0 && native_evidence->size_px.valid() &&
            native_evidence->overlay_capture_excluded && native_evidence->overlay_visuals_allowed &&
            native_evidence->pixel_source == CapturePixelSource::windows_graphics_capture_texture &&
            native_evidence->pixel_scope == CapturePixelScope::exact_selected_window &&
            native_evidence->external_display_overlay_pixels_excluded &&
            native_evidence->desktop_luminance_excluded_from_pixel_evidence) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    if (!capture_evidence || capture_evidence->state != BrokerState::capturing_primary ||
        capture_evidence->capture_backend != CaptureBackend::windows_graphics_capture ||
        capture_evidence->target_state != TargetState::selected ||
        capture_evidence->frames_received < 2 || capture_evidence->frames_presented < 1 ||
        !native_evidence || native_evidence->process_id != GetCurrentProcessId() ||
        native_evidence->native_window != reinterpret_cast<std::uintptr_t>(target_window) ||
        native_evidence->executable_name != inspected.process_name ||
        native_evidence->device_generation == 0 || native_evidence->geometry_epoch == 0 ||
        native_evidence->latest_sequence <= 1 || native_evidence->latest_qpc == 0 ||
        native_evidence->initial_hash == 0 || native_evidence->latest_hash == 0 ||
        native_evidence->hash_changes == 0 || native_evidence->geometry_changes == 0 ||
        !native_evidence->size_px.valid() || !native_evidence->overlay_capture_excluded ||
        !native_evidence->overlay_visuals_allowed ||
        native_evidence->pixel_source != CapturePixelSource::windows_graphics_capture_texture ||
        native_evidence->pixel_scope != CapturePixelScope::exact_selected_window ||
        !native_evidence->external_display_overlay_pixels_excluded ||
        !native_evidence->desktop_luminance_excluded_from_pixel_evidence) {
        if (capture_evidence) {
            std::cerr << "capture evidence timeout: state="
                      << static_cast<std::uint32_t>(capture_evidence->state)
                      << " backend=" << static_cast<std::uint32_t>(capture_evidence->capture_backend)
                      << " target=" << static_cast<std::uint32_t>(capture_evidence->target_state)
                      << " received=" << capture_evidence->frames_received
                      << " presented=" << capture_evidence->frames_presented << '\n';
        }
        if (native_evidence) {
            std::cerr << "native capture evidence: pid=" << native_evidence->process_id
                      << " hwnd=" << native_evidence->native_window
                      << " exe=" << native_evidence->executable_name
                      << " device_generation=" << native_evidence->device_generation
                      << " geometry_epoch=" << native_evidence->geometry_epoch
                      << " geometry_changes=" << native_evidence->geometry_changes
                      << " sequence=" << native_evidence->latest_sequence
                      << " qpc=" << native_evidence->latest_qpc
                      << " initial_hash=" << native_evidence->initial_hash
                      << " latest_hash=" << native_evidence->latest_hash
                      << " hash_changes=" << native_evidence->hash_changes
                      << " size=" << native_evidence->size_px.width << 'x' << native_evidence->size_px.height
                      << " overlay_excluded=" << native_evidence->overlay_capture_excluded
                      << " visuals=" << native_evidence->overlay_visuals_allowed << '\n';
        } else {
            std::cerr << "native capture evidence payload was unavailable or malformed\n";
        }
        return fail_child("primary WGC frame evidence");
    }

    wchar_t smoke_test_path[32768]{};
    if (GetModuleFileNameW(nullptr, smoke_test_path, ARRAYSIZE(smoke_test_path)) == 0) {
        return fail_child("residual worker executable path");
    }
    constexpr std::uint32_t residual_width = 96;
    constexpr std::uint32_t residual_height = 48;
    auto residual_worker = launch_residual_worker(smoke_test_path, residual_width, residual_height);
    if (!residual_worker) return fail_child("production residual worker launch");

    std::optional<VisualSourceLease> source_lease;
    std::optional<RemoteHandleReuseProbe> source_handle_reuse;
    ipc::StatusCode last_source_status{ipc::StatusCode::internal_error};
    std::optional<std::uint32_t> last_source_failure;
    const auto source_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    while (!source_lease && std::chrono::steady_clock::now() < source_deadline) {
        target_color = target_color == RGB(18, 92, 112) ? RGB(116, 34, 72) : RGB(18, 92, 112);
        InvalidateRect(target_window, nullptr, FALSE);
        UpdateWindow(target_window);
        pump_messages();
        std::this_thread::sleep_for(std::chrono::milliseconds(20));

        const auto source_payload = ipc::encode_command(
            ipc::CommandKind::allocate_visual_source,
            ipc::AllocateVisualSourceCommand{
                .worker_process_id = residual_worker->process_id,
                .worker_process_creation_time = residual_worker->creation_time,
                .worker_executable_name = residual_worker->executable_name,
                .actor_id = 5,
                .track_id = 6,
                .track_epoch = 7,
            });
        if (!source_payload) {
            close_residual_worker(*residual_worker);
            return fail_child("visual source allocation encoding");
        }
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::allocate_visual_source;
        request.payload = *source_payload;
        const auto source_response = transact(child.pipe, request);
        if (!source_response || source_response->status != ipc::StatusCode::ok) {
            if (source_response) {
                last_source_status = source_response->status;
                last_source_failure = read_little<std::uint32_t>(source_response->payload, 0U);
            }
            continue;
        }
        auto decoded = ipc::decode_visual_source_lease(source_response->payload);
        if (!decoded || decoded->worker_process_id != residual_worker->process_id ||
            decoded->worker_process_creation_time != residual_worker->creation_time ||
            decoded->worker_executable_name != residual_worker->executable_name ||
            decoded->cancellation_generation != request.cancellation_generation ||
            decoded->source_geometry_epoch == 0U ||
            decoded->source_frame_sequence == 0U || decoded->source_frame_qpc == 0U ||
            decoded->actor_id != 5U || decoded->track_id != 6U || decoded->track_epoch != 7U ||
            decoded->expires_qpc <= qpc_now() || decoded->qpc_frequency != frequency ||
            !verify_exact_source_texture(*residual_worker, *decoded)) {
            close_residual_worker(*residual_worker);
            return fail_child("exact current WGC source texture lease");
        }
        source_handle_reuse = force_remote_handle_reuse(
            *residual_worker, decoded->worker_handle_value);
        if (!source_handle_reuse) {
            close_residual_worker(*residual_worker);
            return fail_child("worker source-handle reuse regression fixture");
        }
        source_lease = std::move(decoded);
    }
    if (!source_lease) {
        std::cerr << "visual source allocation timeout: status="
                  << ipc::to_string(last_source_status);
        if (last_source_failure) std::cerr << " failure=" << *last_source_failure;
        std::cerr << '\n';
        close_residual_worker(*residual_worker);
        return fail_child("fresh WGC source lease allocation");
    }
    std::cout << "authenticated broker duplicated an exact current WGC source texture into the "
                 "bound worker process and the lease reopened with keyed-mutex ownership\n";
    const auto release_payload = ipc::encode_command(
        ipc::CommandKind::release_visual_source,
        ipc::ReleaseVisualSourceCommand{
            .worker_process_id = residual_worker->process_id,
            .lease_nonce_high = source_lease->lease_nonce_high,
            .lease_nonce_low = source_lease->lease_nonce_low,
        });
    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::release_visual_source;
    request.payload = release_payload ? *release_payload : std::vector<std::byte>{};
    const auto source_released = release_payload ? transact(child.pipe, request) : std::nullopt;
    if (!source_released || source_released->status != ipc::StatusCode::ok) {
        if (source_handle_reuse) close_remote_probe(*residual_worker, *source_handle_reuse);
        close_residual_worker(*residual_worker);
        return fail_child("visual source transfer acknowledgement");
    }
    HANDLE reopened_probe{};
    const bool probe_survived = source_handle_reuse &&
        SetEvent(source_handle_reuse->local_event) &&
        DuplicateHandle(residual_worker->process,
                        reinterpret_cast<HANDLE>(static_cast<std::uintptr_t>(
                            source_handle_reuse->remote_handle_value)),
                        GetCurrentProcess(), &reopened_probe, 0U, FALSE,
                        DUPLICATE_SAME_ACCESS) &&
        reopened_probe && WaitForSingleObject(reopened_probe, 0U) == WAIT_OBJECT_0;
    if (reopened_probe) CloseHandle(reopened_probe);
    if (source_handle_reuse) close_remote_probe(*residual_worker, *source_handle_reuse);
    if (!probe_survived) {
        close_residual_worker(*residual_worker);
        return fail_child("source-handle reuse survived lease release");
    }
    if (!visual_worker_crash_preserves_live_playback(
            child, request, frequency, *residual_worker)) {
        close_residual_worker(*residual_worker);
        return fail_child("visual worker crash preserved independent PCM drain");
    }
    residual_worker = launch_residual_worker(smoke_test_path, residual_width, residual_height);
    if (!residual_worker) return fail_child("residual worker restart after visual-only crash");
    std::cout << "optional visual worker crash left live PCM leased, submitted, and endpoint-drained "
                 "without global broker cancellation\n";

    bool residual_presented{};
    ipc::StatusCode last_patch_status{ipc::StatusCode::internal_error};
    std::optional<std::uint32_t> last_contract;
    const auto residual_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    std::uint64_t lease_nonce = 0x8000;
    while (!residual_presented && std::chrono::steady_clock::now() < residual_deadline) {
        target_color = target_color == RGB(18, 92, 112) ? RGB(116, 34, 72) : RGB(18, 92, 112);
        InvalidateRect(target_window, nullptr, FALSE);
        UpdateWindow(target_window);
        pump_messages();
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::capture_evidence;
        request.payload = *native_evidence_payload;
        const auto fresh_response = transact(child.pipe, request);
        const auto fresh = fresh_response && fresh_response->status == ipc::StatusCode::ok
                               ? decode_native_capture_evidence(fresh_response->payload)
                               : std::nullopt;
        if (!fresh || fresh->device_generation == 0 || fresh->latest_sequence == 0 ||
            fresh->latest_qpc == 0 ||
            fresh->geometry_epoch == 0 || fresh->size_px.width <= static_cast<std::int32_t>(residual_width) ||
            fresh->size_px.height <= static_cast<std::int32_t>(residual_height) ||
            fresh->pixel_source != CapturePixelSource::windows_graphics_capture_texture ||
            fresh->pixel_scope != CapturePixelScope::exact_selected_window ||
            !fresh->external_display_overlay_pixels_excluded ||
            !fresh->desktop_luminance_excluded_from_pixel_evidence) {
            close_residual_worker(*residual_worker);
            return fail_child("fresh residual frame identity");
        }
        const auto residual_left = (fresh->size_px.width - static_cast<std::int32_t>(residual_width)) / 2;
        const auto residual_top = (fresh->size_px.height - static_cast<std::int32_t>(residual_height)) / 2;
        const RectF bounds{
            static_cast<double>(residual_left) / static_cast<double>(fresh->size_px.width),
            static_cast<double>(residual_top) / static_cast<double>(fresh->size_px.height),
            static_cast<double>(residual_left + static_cast<std::int32_t>(residual_width)) /
                static_cast<double>(fresh->size_px.width),
            static_cast<double>(residual_top + static_cast<std::int32_t>(residual_height)) /
                static_cast<double>(fresh->size_px.height),
        };

        const auto measured_qpc = qpc_now();
        const auto occlusion_payload = ipc::encode_command(
            ipc::CommandKind::submit_occlusion,
            ipc::SubmitOcclusionCommand{
                .face_confidence = 0.99,
                .landmark_confidence = 0.99,
                .visibility_ratio = 0.99,
                .mouth_occluded = false,
                .measured_qpc = measured_qpc,
                .source_frame_sequence = fresh->latest_sequence,
                .source_device_generation = fresh->device_generation,
                .source_geometry_epoch = fresh->geometry_epoch,
                .source_frame_qpc = fresh->latest_qpc,
                .actor_id = 5,
                .track_id = 6,
                .track_epoch = 7,
            });
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::submit_occlusion;
        request.payload = *occlusion_payload;
        const auto occlusion_response = transact(child.pipe, request);
        if (!occlusion_response || occlusion_response->status != ipc::StatusCode::ok) continue;

        const auto produced_qpc = qpc_now();
        const ipc::SharedTextureDescriptor texture{
            .schema_version = 1,
            .session_nonce = child.nonce,
            .session_id = child.session,
            .lease_nonce_high = ++lease_nonce,
            .lease_nonce_low = lease_nonce ^ 0x5a5a5a5aULL,
            .worker_process_id = residual_worker->process_id,
            .worker_process_creation_time = residual_worker->creation_time,
            .worker_executable_name = residual_worker->executable_name,
            .source_process_handle_value = residual_worker->metadata.source_handle_value,
            .adapter_luid = residual_worker->metadata.adapter_luid,
            .keyed_mutex_acquire_key = 1,
            .keyed_mutex_release_key = 2,
            .width = residual_width,
            .height = residual_height,
            .stride_bytes = residual_width * 4U,
            .dxgi_format = DXGI_FORMAT_B8G8R8A8_UNORM,
            .alpha_mode = 1,
            .expires_qpc = produced_qpc + frequency * 80U / 1000U,
        };
        const auto patch_payload = ipc::encode_command(
            ipc::CommandKind::submit_patch,
            ipc::SubmitPatchCommand{
                .source_frame_sequence = fresh->latest_sequence,
                .cancellation_generation = request.cancellation_generation,
                .left = bounds.left,
                .top = bounds.top,
                .right = bounds.right,
                .bottom = bounds.bottom,
                .confidence = 0.99,
                .produced_qpc = produced_qpc,
                .shared_texture = texture,
                .source_device_generation = fresh->device_generation,
                .source_frame_qpc = fresh->latest_qpc,
                .source_geometry_epoch = fresh->geometry_epoch,
                .actor_id = 5,
                .track_id = 6,
                .track_epoch = 7,
            });
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::submit_patch;
        request.payload = *patch_payload;
        const auto patch_response = transact(child.pipe, request);
        if (!patch_response) continue;
        last_patch_status = patch_response->status;
        last_contract = read_little<std::uint32_t>(patch_response->payload, 0);
        const auto presentation = read_little<std::uint32_t>(patch_response->payload, 4);
        residual_presented = patch_response->status == ipc::StatusCode::ok &&
                             presentation && *presentation == 1;
    }
    close_residual_worker(*residual_worker);
    if (!residual_presented) {
        std::cerr << "production residual IPC did not present: status="
                  << ipc::to_string(last_patch_status);
        if (last_contract) std::cerr << " contract=" << *last_contract;
        std::cerr << '\n';
        return fail_child("authenticated residual import/presentation");
    }
    std::cout << "authenticated nested session/nonce and typed actor/track/frame lease "
                 "presented through the broker service\n";

    const auto clear_payload = ipc::encode_command(ipc::CommandKind::clear_target,
                                                   ipc::ClearTargetCommand{});
    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::clear_target;
    request.payload = *clear_payload;
    const auto cleared = transact(child.pipe, request);
    if (!cleared || cleared->status != ipc::StatusCode::ok ||
        cleared->cancellation_generation <= request.cancellation_generation) {
        return fail_child("ClearTarget response");
    }
    request.cancellation_generation = cleared->cancellation_generation;

    // Frames may legitimately arrive between the final active diagnostics
    // response and processing ClearTarget. Prove capture stopped by observing
    // the cleared state and stable counters after that command boundary.
    constexpr unsigned required_stable_observations = 5;
    unsigned stable_observations{};
    std::optional<CaptureDiagnosticsEvidence> cleared_evidence;
    const auto clear_observation_deadline =
        std::chrono::steady_clock::now() + std::chrono::milliseconds(500);
    while (std::chrono::steady_clock::now() < clear_observation_deadline) {
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::diagnostics;
        request.payload = *diagnostics_payload;
        const auto after_clear = transact(child.pipe, request);
        const auto observation =
            after_clear ? decode_capture_evidence(after_clear->payload) : std::nullopt;
        if (!after_clear || after_clear->status != ipc::StatusCode::ok || !observation ||
            observation->state != BrokerState::awaiting_target ||
            observation->capture_backend != CaptureBackend::none ||
            observation->target_state != TargetState::none) {
            std::cerr << "post-ClearTarget observation: response=" << static_cast<bool>(after_clear);
            if (after_clear) {
                std::cerr << " status=" << ipc::to_string(after_clear->status)
                          << " generation=" << after_clear->cancellation_generation
                          << " payload_bytes=" << after_clear->payload.size();
            }
            if (observation) {
                std::cerr << " state=" << static_cast<std::uint32_t>(observation->state)
                          << " backend=" << static_cast<std::uint32_t>(observation->capture_backend)
                          << " target=" << static_cast<std::uint32_t>(observation->target_state);
            }
            std::cerr << '\n';
            return fail_child("post-ClearTarget state");
        }
        if (cleared_evidence &&
            observation->frames_received == cleared_evidence->frames_received &&
            observation->frames_presented == cleared_evidence->frames_presented) {
            ++stable_observations;
        } else {
            stable_observations = 1;
        }
        cleared_evidence = observation;
        if (stable_observations >= required_stable_observations) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(25));
    }
    if (!cleared_evidence || stable_observations < required_stable_observations) {
        if (cleared_evidence) {
            std::cerr << "post-ClearTarget counters did not stabilize: received="
                      << cleared_evidence->frames_received
                      << " presented=" << cleared_evidence->frames_presented
                      << " stable_observations=" << stable_observations << '\n';
        }
        return fail_child("post-ClearTarget capture quiescence");
    }

    const auto shutdown_payload = ipc::encode_command(ipc::CommandKind::shutdown, ipc::ShutdownCommand{});
    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::shutdown;
    request.payload = *shutdown_payload;
    const auto shutdown = transact(child.pipe, request);
    const bool exited = shutdown && shutdown->status == ipc::StatusCode::ok &&
                        WaitForSingleObject(child.process.hProcess, 5000) == WAIT_OBJECT_0;
    close_child(child);
    if (!exited) {
        std::cerr << "authenticated service did not exit after Shutdown\n";
        return false;
    }
    std::cout << "authenticated SelectTarget observed " << capture_evidence->frames_received
              << " WGC frames (" << capture_evidence->frames_presented
              << " presented), sequence=" << native_evidence->latest_sequence
              << " qpc=" << native_evidence->latest_qpc
              << " hash_changes=" << native_evidence->hash_changes
              << " geometry_epoch=" << native_evidence->geometry_epoch
              << "; ClearTarget stopped capture\n";

    auto disconnect_value = launch_service(executable, 2);
    if (!disconnect_value) {
        std::cerr << "disconnect service launch failed\n";
        return false;
    }
    auto disconnect = std::move(*disconnect_value);
    CloseHandle(disconnect.pipe);
    disconnect.pipe = INVALID_HANDLE_VALUE;
    const bool disconnected_exit = WaitForSingleObject(disconnect.process.hProcess, 5000) == WAIT_OBJECT_0;
    close_child(disconnect);
    if (!disconnected_exit) std::cerr << "service did not exit after control-pipe disconnect\n";
    return disconnected_exit;
}

[[nodiscard]] TargetGeometry geometry_for(HWND window, SizeI captured_size) {
    RECT client{};
    GetClientRect(window, &client);
    POINT origin{};
    ClientToScreen(window, &origin);
    const RectI client_bounds{origin.x, origin.y,
                              origin.x + client.right, origin.y + client.bottom};
    const HMONITOR monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
    MONITORINFO info{sizeof(info)};
    GetMonitorInfoW(monitor, &info);
    const UINT dpi = GetDpiForWindow(window);
    return {
        client_bounds,
        client_bounds,
        client_bounds,
        captured_size,
        {"smoke-monitor",
         {info.rcMonitor.left, info.rcMonitor.top, info.rcMonitor.right, info.rcMonitor.bottom},
         {info.rcWork.left, info.rcWork.top, info.rcWork.right, info.rcWork.bottom},
         dpi == 0 ? 96U : dpi,
         dpi == 0 ? 96U : dpi,
         ColorSpace::sdr_srgb,
         DisplayRotation::identity,
         80.0},
        false,
    };
}

[[nodiscard]] bool shared_residual_lifecycle_smoke(const std::wstring& test_executable,
                                                   const HWND target_window,
                                                   const std::string& target_executable_name) {
    auto platform = create_windows_media_platform();
    std::optional<TargetGeometry> observed_geometry;
    std::optional<FrameDescriptor> observed_frame;
    std::optional<Failure> asynchronous_failure;
    platform->set_callbacks({
        .on_frame = [&](FrameDescriptor frame) { observed_frame = std::move(frame); },
        .on_failure = [&](Failure failure) { asynchronous_failure = std::move(failure); },
        .on_target_state = [&](const TargetState state, std::optional<TargetGeometry> geometry) {
            if (state == TargetState::selected && geometry) observed_geometry = std::move(geometry);
        },
    });
    const GameTarget target{reinterpret_cast<std::uintptr_t>(target_window),
                            GetCurrentProcessId(), target_executable_name,
                            "shared residual smoke target"};
    Failure failure;
    if (!platform->validate_target(target, failure)) {
        std::cerr << "shared residual target validation failed: " << failure.message << '\n';
        return false;
    }
    if (!platform->start_capture(target, CaptureBackend::windows_graphics_capture, failure)) {
        std::cerr << "shared residual platform capture startup failed: " << failure.message << '\n';
        return false;
    }

    bool overlay_started{};
    const auto capture_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while ((!overlay_started || !observed_frame) &&
           std::chrono::steady_clock::now() < capture_deadline) {
        pump_messages();
        platform->poll();
        if (!overlay_started && observed_geometry) {
            overlay_started = platform->start_overlay(*observed_geometry, failure);
            if (!overlay_started) break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!overlay_started || !observed_geometry || !observed_frame || asynchronous_failure ||
        !platform->overlay_capture_excluded()) {
        std::cerr << "shared residual platform did not establish WGC/capture-excluded overlay";
        if (!failure.message.empty()) std::cerr << ": " << failure.message;
        if (asynchronous_failure) std::cerr << ": " << asynchronous_failure->message;
        std::cerr << '\n';
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }

    constexpr UINT residual_width = 96;
    constexpr UINT residual_height = 48;
    if (observed_frame->content_size_px.width <= static_cast<std::int32_t>(residual_width) ||
        observed_frame->content_size_px.height <= static_cast<std::int32_t>(residual_height)) {
        std::cerr << "captured target is too small for the shared residual proof\n";
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    const auto residual_left = (observed_frame->content_size_px.width -
                                static_cast<std::int32_t>(residual_width)) / 2;
    const auto residual_top = (observed_frame->content_size_px.height -
                               static_cast<std::int32_t>(residual_height)) / 2;
    const RectF normalized_bounds{
        static_cast<double>(residual_left) /
            static_cast<double>(observed_frame->content_size_px.width),
        static_cast<double>(residual_top) /
            static_cast<double>(observed_frame->content_size_px.height),
        static_cast<double>(residual_left + static_cast<std::int32_t>(residual_width)) /
            static_cast<double>(observed_frame->content_size_px.width),
        static_cast<double>(residual_top + static_cast<std::int32_t>(residual_height)) /
            static_cast<double>(observed_frame->content_size_px.height),
    };
    const auto mapped_geometry = calculate_overlay_geometry(*observed_geometry);
    const auto patch_bounds = mapped_geometry
                                  ? map_normalized_source_rect(normalized_bounds, *mapped_geometry)
                                  : std::nullopt;
    if (!mapped_geometry || !patch_bounds || patch_bounds->width() != residual_width ||
        patch_bounds->height() != residual_height) {
        std::cerr << "shared residual exact normalized mapping failed\n";
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }

    std::uint64_t nonce_counter = 0x1000;
    const auto make_lease = [&](const ResidualWorkerProcess& worker) {
        const auto produced_qpc = qpc_now();
        return SharedResidualLease{
            .schema_version = 1,
            .worker_process_id = worker.process_id,
            .worker_process_creation_time = worker.creation_time,
            .worker_executable_name = worker.executable_name,
            .source_handle_value = worker.metadata.source_handle_value,
            .lease_nonce_high = ++nonce_counter,
            .lease_nonce_low = nonce_counter ^ 0xa5a5a5a5ULL,
            .adapter_luid = worker.metadata.adapter_luid,
            .keyed_mutex_acquire_key = 1,
            .keyed_mutex_release_key = 2,
            .width = residual_width,
            .height = residual_height,
            .stride_bytes = residual_width * 4U,
            .dxgi_format = DXGI_FORMAT_B8G8R8A8_UNORM,
            .alpha_mode = 1,
            .expires_qpc = produced_qpc + qpc_frequency() / 2U,
            .cancellation_generation = 3,
            .source_device_generation = observed_frame->device_generation,
            .source_geometry_epoch = observed_frame->geometry_epoch,
            .source_frame_sequence = observed_frame->sequence,
            .source_frame_qpc = observed_frame->captured_qpc,
            .produced_qpc = produced_qpc,
            .actor_id = 5,
            .track_id = 6,
            .track_epoch = 7,
            .normalized_bounds = normalized_bounds,
        };
    };
    const auto import = [&](const SharedResidualLease& lease, std::uintptr_t& texture) {
        failure = {};
        return platform->import_shared_residual(lease, texture, failure);
    };

    auto presentation_worker = launch_residual_worker(test_executable, residual_width, residual_height);
    if (!presentation_worker) {
        std::cerr << "shared residual D3D worker failed to launch\n";
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    auto wrong_adapter_lease = make_lease(*presentation_worker);
    wrong_adapter_lease.adapter_luid ^= 1U;
    std::uintptr_t imported_texture{};
    if (import(wrong_adapter_lease, imported_texture) || imported_texture != 0 ||
        platform->shared_residual_active() || failure.code != FailureCode::backend_unavailable) {
        std::cerr << "adapter-LUID mismatch did not fail closed\n";
        close_residual_worker(*presentation_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    auto non_texture_handle_lease = make_lease(*presentation_worker);
    non_texture_handle_lease.source_handle_value = static_cast<std::uint64_t>(
        reinterpret_cast<std::uintptr_t>(presentation_worker->release_event));
    imported_texture = 0;
    if (import(non_texture_handle_lease, imported_texture) || imported_texture != 0 ||
        platform->shared_residual_active() || failure.code != FailureCode::access_denied) {
        std::cerr << "worker-owned non-texture handle was accepted as a shared texture\n";
        close_residual_worker(*presentation_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    const auto presentation_lease = make_lease(*presentation_worker);
    imported_texture = 0;
    if (!import(presentation_lease, imported_texture) || imported_texture == 0 ||
        !platform->shared_residual_active()) {
        std::cerr << "broker failed to duplicate/open the worker-owned shared texture: "
                  << failure.message << '\n';
        close_residual_worker(*presentation_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    const MouthPatch patch{
        .source_frame_sequence = observed_frame->sequence,
        .cancellation_generation = presentation_lease.cancellation_generation,
        .normalized_bounds = normalized_bounds,
        .confidence = 0.99,
        .produced_at = std::chrono::steady_clock::now(),
        .native_texture = imported_texture,
        .source_device_generation = observed_frame->device_generation,
        .source_frame_captured_at = observed_frame->captured_at,
        .source_geometry_epoch = observed_frame->geometry_epoch,
        .source_frame_qpc = observed_frame->captured_qpc,
        .actor_id = presentation_lease.actor_id,
        .selected_track_id = presentation_lease.track_id,
        .track_epoch = presentation_lease.track_epoch,
    };
    platform->present_patch(*observed_frame, patch, *patch_bounds, *mapped_geometry);
    if (platform->shared_residual_active()) {
        std::cerr << "presented residual retained its one-shot lease\n";
        close_residual_worker(*presentation_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    imported_texture = 0;
    if (import(presentation_lease, imported_texture) || imported_texture != 0 ||
        failure.code != FailureCode::access_denied) {
        std::cerr << "consumed residual lease nonce was replayable\n";
        close_residual_worker(*presentation_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    close_residual_worker(*presentation_worker);

    auto cancellation_worker = launch_residual_worker(test_executable, residual_width, residual_height);
    auto cancellation_lease = cancellation_worker ? std::optional{make_lease(*cancellation_worker)}
                                                   : std::nullopt;
    imported_texture = 0;
    if (!cancellation_worker || !cancellation_lease ||
        !import(*cancellation_lease, imported_texture)) {
        std::cerr << "residual cancellation fixture import failed: " << failure.message << '\n';
        if (cancellation_worker) close_residual_worker(*cancellation_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    platform->suppress_residual();
    if (platform->shared_residual_active()) {
        std::cerr << "cancellation/suppression retained the worker lease\n";
        close_residual_worker(*cancellation_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    close_residual_worker(*cancellation_worker);

    auto exiting_worker = launch_residual_worker(test_executable, residual_width, residual_height);
    auto exiting_lease = exiting_worker ? std::optional{make_lease(*exiting_worker)} : std::nullopt;
    imported_texture = 0;
    if (!exiting_worker || !exiting_lease || !import(*exiting_lease, imported_texture)) {
        std::cerr << "worker-exit fixture import failed: " << failure.message << '\n';
        if (exiting_worker) close_residual_worker(*exiting_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    close_residual_worker(*exiting_worker);
    platform->poll();
    if (platform->shared_residual_active() || asynchronous_failure) {
        std::cerr << "dead residual worker lease survived platform polling";
        if (asynchronous_failure) std::cerr << ": " << asynchronous_failure->message;
        std::cerr << '\n';
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }

    auto reset_worker = launch_residual_worker(test_executable, residual_width, residual_height);
    auto reset_lease = reset_worker ? std::optional{make_lease(*reset_worker)} : std::nullopt;
    imported_texture = 0;
    if (!reset_worker || !reset_lease || !import(*reset_lease, imported_texture)) {
        std::cerr << "device-reset fixture import failed: " << failure.message << '\n';
        if (reset_worker) close_residual_worker(*reset_worker);
        platform->stop_overlay();
        platform->stop_capture();
        return false;
    }
    if (!platform->recreate_graphics_device(1, failure) || platform->shared_residual_active()) {
        std::cerr << "graphics reset retained a shared residual lease: " << failure.message << '\n';
        close_residual_worker(*reset_worker);
        return false;
    }
    close_residual_worker(*reset_worker);
    std::cout << "worker-owned shared NT handle imported/presented once; replay, cancellation, "
                 "worker exit, and device reset released fail-closed\n";
    return true;
}

} // namespace

int main(const int argc, char** argv) {
    if (argc == 6 && std::string_view{argv[1]} == "--residual-worker") {
        const auto output_pipe = static_cast<std::uintptr_t>(std::strtoull(argv[2], nullptr, 10));
        const auto release_event = static_cast<std::uintptr_t>(std::strtoull(argv[3], nullptr, 10));
        const auto width = static_cast<unsigned long>(std::strtoul(argv[4], nullptr, 10));
        const auto height = static_cast<unsigned long>(std::strtoul(argv[5], nullptr, 10));
        if (output_pipe == 0 || release_event == 0 || width == 0 || height == 0 ||
            width > 4096 || height > 4096) {
            return EXIT_FAILURE;
        }
        return run_residual_worker(reinterpret_cast<HANDLE>(output_pipe),
                                   reinterpret_cast<HANDLE>(release_event),
                                   static_cast<UINT>(width), static_cast<UINT>(height));
    }
    if (argc == 2 && std::string_view{argv[1]} == "--service-only") {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        const HWND target_window = create_test_window();
        if (!target_window) {
            std::cerr << "failed to create service protocol WGC target window\n";
            return EXIT_FAILURE;
        }
        wchar_t test_path[32768]{};
        if (GetModuleFileNameW(nullptr, test_path, ARRAYSIZE(test_path)) == 0) {
            DestroyWindow(target_window);
            return EXIT_FAILURE;
        }
        const auto service_path =
            (std::filesystem::path(test_path).parent_path() / L"npc-media-broker.exe").wstring();
        const bool passed = service_lifecycle_smoke(service_path, target_window);
        DestroyWindow(target_window);
        if (!passed) {
            std::cerr << "authenticated named-pipe service lifecycle smoke failed\n";
            return EXIT_FAILURE;
        }
        std::cout << "authenticated named-pipe service lifecycle smoke passed\n";
        return EXIT_SUCCESS;
    }
    ScopedComApartment apartment;
    if (!apartment.initialized()) {
        std::cerr << "failed to initialize the WGC smoke thread as COM MTA\n";
        return EXIT_FAILURE;
    }
    const bool direct_shared_only =
        argc == 2 && std::string_view{argv[1]} == "--direct-shared-only";
    if (argc != 1 && !direct_shared_only) {
        std::cerr << "unsupported smoke-test argument\n";
        return EXIT_FAILURE;
    }
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    const HWND window = create_test_window();
    if (!window) {
        std::cerr << "failed to create WGC smoke target window\n";
        return EXIT_FAILURE;
    }
    const auto inspected = inspect_target(window, GetCurrentProcessId());
    const auto transparency_overlay_count = count_transparency_app_overlays(window);
    const auto local_policy = evaluate_target_policy(inspected, {inspected.process_name});
    if (!local_policy.capture_allowed) {
        std::cerr << "same-user/session HWND inspection was unexpectedly blocked: "
                  << to_string(local_policy.reason) << '\n';
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const auto mismatched = inspect_target(window, GetCurrentProcessId() + 1);
    if (evaluate_target_policy(mismatched, {mismatched.process_name}).reason != TargetBlockReason::process_mismatch) {
        std::cerr << "HWND/PID mismatch did not fail closed\n";
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    D3D_FEATURE_LEVEL selected{};
    const D3D_FEATURE_LEVEL levels[]{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
    HRESULT hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr,
                                   D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                   levels, ARRAYSIZE(levels), D3D11_SDK_VERSION,
                                   &device, &selected, &context);
    if (FAILED(hr)) {
        std::cerr << "failed to create D3D11 smoke device\n";
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    Failure failure;
    GraphicsCapture capture;
    if (!capture.start(window, device.Get(), failure)) {
        std::cerr << failure.message << '\n';
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    std::optional<OwnedCaptureFrame> captured;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (!captured && std::chrono::steady_clock::now() < deadline) {
        pump_messages();
        captured = capture.take_latest();
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!captured || !captured->texture || !captured->size_px.valid()) {
        std::cerr << "WGC free-threaded pool produced no frame\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    const auto initial_size = captured->size_px;
    if (captured->captured_qpc == 0 || captured->content_hash == 0) {
        std::cerr << "WGC frame omitted QPC or content fingerprint evidence\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const auto initial_hash = captured->content_hash;
    const auto initial_qpc = captured->captured_qpc;
    target_color = RGB(116, 34, 72);
    InvalidateRect(window, nullptr, FALSE);
    UpdateWindow(window);
    std::optional<OwnedCaptureFrame> changed;
    const auto change_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (std::chrono::steady_clock::now() < change_deadline) {
        pump_messages();
        if (auto candidate = capture.take_latest(); candidate && candidate->sequence > captured->sequence &&
            candidate->captured_qpc > initial_qpc && candidate->content_hash != 0 &&
            candidate->content_hash != initial_hash) {
            changed = std::move(candidate);
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!changed) {
        std::cerr << "WGC content fingerprint did not change with deterministic target pixels\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    captured = std::move(changed);
    const auto exact_hwnd_center = read_center_bgra(device.Get(), context.Get(), captured->texture.Get());
    if (!exact_hwnd_center || !bgra_matches_color(*exact_hwnd_center, target_color)) {
        std::cerr << "exact-HWND WGC included external display composition or returned unexpected target pixels\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    std::cout << "environment provenance: " << transparency_overlay_count
              << " visible TransparencyApp layered/click-through overlay window(s) on the target monitor; "
                 "exact-game-HWND WGC retained the deterministic target pixel. Full-display capture was not tested.\n";
    const auto initial_sequence = captured->sequence;
    RECT before_resize{};
    GetWindowRect(window, &before_resize);
    SetWindowPos(window, nullptr, before_resize.left + 40, before_resize.top + 30,
                 800, 450, SWP_NOACTIVATE | SWP_NOZORDER);
    InvalidateRect(window, nullptr, TRUE);
    std::optional<OwnedCaptureFrame> resized;
    const auto resize_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (std::chrono::steady_clock::now() < resize_deadline) {
        pump_messages();
        if (auto candidate = capture.take_latest();
            candidate && candidate->size_px.valid() && candidate->size_px != initial_size) {
            resized = std::move(candidate);
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!resized || resized->sequence <= initial_sequence) {
        std::cerr << "WGC frame pool did not recover with an advancing frame sequence\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    captured = std::move(resized);

    const auto target = geometry_for(window, captured->size_px);
    const auto mapped = calculate_overlay_geometry(target);
    ResidualOverlay overlay;
    if (!mapped || !overlay.start(device.Get(), *mapped, failure)) {
        std::cerr << (mapped ? failure.message : "overlay geometry failed") << '\n';
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    if (!overlay.capture_excluded()) {
        std::cerr << "DirectComposition overlay is not excluded from capture\n";
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    constexpr UINT residual_width = 96;
    constexpr UINT residual_height = 48;
    std::vector<std::uint32_t> premultiplied(residual_width * residual_height, 0x80463010U);
    D3D11_TEXTURE2D_DESC texture_description{};
    texture_description.Width = residual_width;
    texture_description.Height = residual_height;
    texture_description.MipLevels = 1;
    texture_description.ArraySize = 1;
    texture_description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    texture_description.SampleDesc.Count = 1;
    texture_description.Usage = D3D11_USAGE_IMMUTABLE;
    texture_description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    const D3D11_SUBRESOURCE_DATA texture_data{premultiplied.data(), residual_width * 4, 0};
    ComPtr<ID3D11Texture2D> residual;
    hr = device->CreateTexture2D(&texture_description, &texture_data, &residual);
    const RectI patch{mapped->clipped_desktop_bounds_px.left + 20,
                      mapped->clipped_desktop_bounds_px.top + 20,
                      mapped->clipped_desktop_bounds_px.left + 20 + static_cast<int>(residual_width),
                      mapped->clipped_desktop_bounds_px.top + 20 + static_cast<int>(residual_height)};
    if (FAILED(hr) || !overlay.present(residual.Get(), patch, failure)) {
        std::cerr << (FAILED(hr) ? "residual texture creation failed" : failure.message) << '\n';
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    overlay.hide();

    EventDrivenAudio audio;
    if (!audio.start(failure) || !audio.capture_ring() || !audio.render_ring()) {
        std::cerr << failure.message << '\n';
        audio.stop();
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
    audio.stop();
    capture.stop();
    overlay.stop();
    wchar_t test_path[32768]{};
    if (GetModuleFileNameW(nullptr, test_path, ARRAYSIZE(test_path)) == 0) {
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    if (!shared_residual_lifecycle_smoke(test_path, window, inspected.process_name)) {
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    if (direct_shared_only) {
        DestroyWindow(window);
        return EXIT_SUCCESS;
    }
    const auto service_path = (std::filesystem::path(test_path).parent_path() / L"npc-media-broker.exe").wstring();
    const bool service_passed = service_lifecycle_smoke(service_path, window);
    DestroyWindow(window);
    if (!service_passed) {
        std::cerr << "authenticated named-pipe service lifecycle smoke failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "WGC advancing sequence/QPC/content-hash evidence, capture-excluded overlay, "
                 "and event-driven WASAPI smoke passed\n";
    return EXIT_SUCCESS;
}

#endif
