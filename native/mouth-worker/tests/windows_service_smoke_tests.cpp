#include "npc/mouth_worker/service_protocol.hpp"
#include "npc/mouth_worker/windows_service.hpp"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <d3d11_1.h>
#include <dxgi1_2.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <limits>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <thread>
#include <utility>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;
using namespace npc::mouth;
using namespace npc::mouth::service;

class UniqueHandle final {
public:
    UniqueHandle() = default;
    explicit UniqueHandle(HANDLE value) : value_(value) {}
    ~UniqueHandle() { reset(); }
    UniqueHandle(const UniqueHandle&) = delete;
    UniqueHandle& operator=(const UniqueHandle&) = delete;
    UniqueHandle(UniqueHandle&& other) noexcept : value_(other.release()) {}
    UniqueHandle& operator=(UniqueHandle&& other) noexcept {
        if (this != &other) reset(other.release());
        return *this;
    }
    [[nodiscard]] HANDLE get() const noexcept { return value_; }
    [[nodiscard]] explicit operator bool() const noexcept {
        return value_ && value_ != INVALID_HANDLE_VALUE;
    }
    [[nodiscard]] HANDLE release() noexcept {
        const HANDLE result = value_;
        value_ = nullptr;
        return result;
    }
    void reset(HANDLE value = nullptr) noexcept {
        if (value_ && value_ != INVALID_HANDLE_VALUE) CloseHandle(value_);
        value_ = value;
    }

private:
    HANDLE value_{};
};

int failures = 0;

void expect(const bool condition, const std::string_view message) {
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        ++failures;
    }
}

[[nodiscard]] Nanoseconds monotonic_ns() {
    LARGE_INTEGER counter{};
    LARGE_INTEGER frequency{};
    if (!QueryPerformanceCounter(&counter) || !QueryPerformanceFrequency(&frequency)) return 0;
    return static_cast<Nanoseconds>(static_cast<long double>(counter.QuadPart) * 1'000'000'000.0L /
                                    static_cast<long double>(frequency.QuadPart));
}

[[nodiscard]] std::uint64_t qpc_now() {
    LARGE_INTEGER value{};
    return QueryPerformanceCounter(&value) && value.QuadPart > 0
        ? static_cast<std::uint64_t>(value.QuadPart)
        : 0U;
}

[[nodiscard]] std::uint64_t qpc_frequency() {
    LARGE_INTEGER value{};
    return QueryPerformanceFrequency(&value) && value.QuadPart > 0
        ? static_cast<std::uint64_t>(value.QuadPart)
        : 0U;
}

[[nodiscard]] std::uint64_t creation_time(const HANDLE process) {
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(process, &created, &exited, &kernel, &user)) return 0U;
    return (static_cast<std::uint64_t>(created.dwHighDateTime) << 32U) |
           static_cast<std::uint64_t>(created.dwLowDateTime);
}

[[nodiscard]] std::wstring quote(const std::wstring& value) {
    return L"\"" + value + L"\"";
}

[[nodiscard]] std::optional<std::string> narrow_ascii(const std::wstring_view value) {
    std::string result;
    result.reserve(value.size());
    for (const wchar_t character : value) {
        if (character < 0 || character > 0x7f) return std::nullopt;
        result.push_back(static_cast<char>(character));
    }
    return result;
}

[[nodiscard]] std::string nonce_hex(const SessionBindingV1& session) {
    constexpr char digits[] = "0123456789abcdef";
    std::string result;
    result.reserve(session.nonce.size() * 2U);
    for (const auto byte : session.nonce) {
        const auto value = std::to_integer<std::uint8_t>(byte);
        result.push_back(digits[value >> 4U]);
        result.push_back(digits[value & 0x0fU]);
    }
    return result;
}

[[nodiscard]] bool write_exact(const HANDLE pipe, const std::span<const std::byte> bytes) {
    std::size_t offset{};
    while (offset < bytes.size()) {
        DWORD transferred{};
        if (!WriteFile(pipe, bytes.data() + offset,
                       static_cast<DWORD>(bytes.size() - offset), &transferred, nullptr) ||
            transferred == 0U) return false;
        offset += transferred;
    }
    return true;
}

[[nodiscard]] bool read_exact(const HANDLE pipe, const std::span<std::byte> bytes) {
    std::size_t offset{};
    while (offset < bytes.size()) {
        DWORD transferred{};
        if (!ReadFile(pipe, bytes.data() + offset,
                      static_cast<DWORD>(bytes.size() - offset), &transferred, nullptr) ||
            transferred == 0U) return false;
        offset += transferred;
    }
    return true;
}

[[nodiscard]] std::optional<WorkerResponseV1> request(const HANDLE pipe,
                                                     const EnvelopeV1& envelope) {
    const auto encoded = encode_envelope(envelope);
    if (!encoded) return std::nullopt;
    const auto framed = frame_message(*encoded);
    if (!write_exact(pipe, framed)) return std::nullopt;
    std::array<std::byte, 4> prefix{};
    if (!read_exact(pipe, prefix)) return std::nullopt;
    const auto size = decode_frame_size(prefix);
    if (!size) return std::nullopt;
    std::vector<std::byte> response(*size);
    if (!read_exact(pipe, response)) return std::nullopt;
    return decode_response(response);
}

[[nodiscard]] EnvelopeV1 envelope(const SessionBindingV1& session,
                                  const CommandKind command,
                                  const std::uint64_t sequence,
                                  const std::uint64_t generation,
                                  std::vector<std::byte> payload = {}) {
    EnvelopeV1 value{};
    value.session = session;
    value.command = command;
    value.sequence = sequence;
    value.cancellation_generation = generation;
    value.deadline_ns = monotonic_ns() + 2'000'000'000;
    value.payload = std::move(payload);
    return value;
}

struct D3dFixture {
    ComPtr<IDXGIAdapter1> adapter;
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11Device1> device1;
    ComPtr<ID3D11DeviceContext> context;
    std::uint32_t luid_low{};
    std::int32_t luid_high{};
};

[[nodiscard]] std::optional<D3dFixture> create_d3d_fixture() {
    ComPtr<IDXGIFactory1> factory;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) return std::nullopt;
    for (UINT index = 0U;; ++index) {
        ComPtr<IDXGIAdapter1> adapter;
        if (factory->EnumAdapters1(index, &adapter) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_ADAPTER_DESC1 description{};
        if (FAILED(adapter->GetDesc1(&description)) ||
            (description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0U) continue;
        D3dFixture result{};
        constexpr std::array levels{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
        D3D_FEATURE_LEVEL level{};
        if (FAILED(D3D11CreateDevice(adapter.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr,
                                     D3D11_CREATE_DEVICE_BGRA_SUPPORT, levels.data(),
                                     static_cast<UINT>(levels.size()), D3D11_SDK_VERSION,
                                     &result.device, &level, &result.context)) ||
            FAILED(result.device.As(&result.device1))) continue;
        result.adapter = std::move(adapter);
        result.luid_low = description.AdapterLuid.LowPart;
        result.luid_high = description.AdapterLuid.HighPart;
        return result;
    }
    return std::nullopt;
}

[[nodiscard]] bool verify_residual_texture(D3dFixture& d3d,
                                           const HANDLE handle,
                                           const TextureLeaseDescriptor& lease) {
    ComPtr<ID3D11Texture2D> texture;
    if (FAILED(d3d.device1->OpenSharedResource1(handle, IID_PPV_ARGS(&texture)))) return false;
    ComPtr<IDXGIKeyedMutex> mutex;
    if (FAILED(texture.As(&mutex)) || mutex->AcquireSync(lease.keyed_mutex_acquire_key, 100U) != S_OK) {
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
    if (FAILED(d3d.device->CreateTexture2D(&staging_description, nullptr, &staging))) return false;
    d3d.context->CopyResource(staging.Get(), texture.Get());
    if (FAILED(mutex->ReleaseSync(lease.keyed_mutex_release_key))) return false;
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(d3d.context->Map(staging.Get(), 0U, D3D11_MAP_READ, 0U, &mapped))) return false;
    bool transparent = false;
    bool visible = false;
    for (std::uint32_t row = 0U; row < description.Height; ++row) {
        const auto* pixels = static_cast<const std::uint8_t*>(mapped.pData) +
                             static_cast<std::size_t>(row) * mapped.RowPitch;
        for (std::uint32_t column = 0U; column < description.Width; ++column) {
            const auto alpha = pixels[static_cast<std::size_t>(column) * 4U + 3U];
            transparent |= alpha == 0U;
            visible |= alpha > 0U;
        }
    }
    d3d.context->Unmap(staging.Get(), 0U);
    return description.Width == lease.width && description.Height == lease.height &&
           transparent && visible;
}

class FakeAdmittedLandmarkProvider final : public NativeLandmarkProviderV1 {
public:
    [[nodiscard]] bool load(const AdmittedLandmarkProviderLaunchV1&,
                            const std::uint64_t generation,
                            std::string&) override {
        generation_ = generation;
        return true;
    }

    [[nodiscard]] std::optional<OpenSeeFaceLandmarkPacketV1> infer(
        const LandmarkInferenceWorkV1& work,
        const Nanoseconds now_ns,
        std::string&) override {
        OpenSeeFaceLandmarkPacketV1 packet{};
        packet.provider_instance_id = 0xfaceU;
        packet.track = work.track;
        packet.frame = work.frame;
        packet.source_frame_qpc = work.source_frame_qpc;
        packet.qpc_frequency = work.qpc_frequency;
        packet.face_bounds = work.seed_face_bounds;
        for (auto& point : packet.landmarks) point = {0.50, 0.45, 0.96};
        packet.landmarks[48U] = {0.43, 0.60, 0.97};
        packet.landmarks[49U] = {0.46, 0.58, 0.97};
        packet.landmarks[50U] = {0.48, 0.57, 0.97};
        packet.landmarks[51U] = {0.50, 0.565, 0.97};
        packet.landmarks[52U] = {0.52, 0.57, 0.97};
        packet.landmarks[53U] = {0.54, 0.58, 0.97};
        packet.landmarks[54U] = {0.57, 0.60, 0.97};
        packet.landmarks[55U] = {0.54, 0.64, 0.97};
        packet.landmarks[56U] = {0.52, 0.655, 0.97};
        packet.landmarks[57U] = {0.50, 0.66, 0.97};
        packet.landmarks[58U] = {0.48, 0.655, 0.97};
        packet.landmarks[59U] = {0.46, 0.64, 0.97};
        packet.landmarks[58U] = {0.57, 0.60, 0.96};
        packet.landmarks[59U] = {0.54, 0.585, 0.96};
        packet.landmarks[60U] = {0.50, 0.578, 0.96};
        packet.landmarks[61U] = {0.46, 0.585, 0.96};
        packet.landmarks[62U] = {0.43, 0.60, 0.96};
        packet.landmarks[63U] = {0.46, 0.63, 0.96};
        packet.landmarks[64U] = {0.50, 0.642, 0.96};
        packet.landmarks[65U] = {0.54, 0.63, 0.96};
        packet.detector_confidence = 0.96;
        packet.landmark_confidence = 0.95;
        packet.visibility_ratio = 0.94;
        packet.measured_at_ns = now_ns;
        return packet;
    }

    [[nodiscard]] bool cancel_to(const std::uint64_t generation) noexcept override {
        if (generation <= generation_) return false;
        generation_ = generation;
        return true;
    }
    void unload() noexcept override { generation_ = 0U; }
    [[nodiscard]] bool loaded() const noexcept override { return generation_ != 0U; }

private:
    std::uint64_t generation_{};
};

[[nodiscard]] AdmittedLandmarkProviderLaunchV1 fake_admitted_launch() {
    AdmittedLandmarkProviderLaunchV1 value{};
    value.pack_id = std::string(admitted_openseeface_pack_id_v1);
    value.pack_revision = std::string(admitted_openseeface_revision_v1);
    value.artifact_root = L"C:\\npc-test-openseeface";
    value.detector_model = value.artifact_root / L"models\\mnv3_detection_opt.onnx";
    value.landmark_model = value.artifact_root / L"models\\lm_model1_opt.onnx";
    value.runtime_library = value.artifact_root / L"runtime\\onnxruntime.dll";
    value.runtime_shared_library =
        value.artifact_root / L"runtime\\onnxruntime_providers_shared.dll";
    value.detector_size_bytes = 568'302U;
    value.landmark_size_bytes = 4'842'329U;
    value.runtime_size_bytes = 14'854'688U;
    value.runtime_shared_size_bytes = 19'456U;
    value.detector_sha256.assign(64U, 'a');
    value.landmark_sha256.assign(64U, 'b');
    value.runtime_sha256.assign(64U, 'c');
    value.runtime_shared_sha256.assign(64U, 'e');
    value.measured_envelope_sha256.assign(64U, 'd');
    value.runtime_revision = std::string(admitted_openseeface_runtime_revision_v1);
    value.backend = std::string(admitted_openseeface_backend_v1);
    value.maximum_signal_rate_hz = 15U;
    value.inference_threads = 1U;
    value.exact_target_process_id = GetCurrentProcessId();
    return value;
}

[[nodiscard]] InstallCharacterMouthAtlasCommandV1 service_atlas(
    const std::uint64_t generation,
    const std::uint64_t actor_id) {
    InstallCharacterMouthAtlasCommandV1 command{};
    command.atlas.cancellation_generation = generation;
    command.atlas.actor_id = actor_id;
    command.atlas.identity_revision = 1U;
    const std::array visemes{
        Viseme::silence, Viseme::rounded, Viseme::open_vowel, Viseme::spread_vowel,
    };
    for (std::size_t state_index = 0U; state_index < visemes.size(); ++state_index) {
        MouthAtlasState state{};
        state.coefficients = coefficients_for_viseme(visemes[state_index]);
        state.appearance.width = 48U;
        state.appearance.height = 32U;
        state.appearance.stride_bytes = 48U * 4U;
        state.appearance.premultiplied_bgra.assign(48U * 32U * 4U, 0U);
        for (std::uint32_t y = 0U; y < state.appearance.height; ++y) {
            for (std::uint32_t x = 0U; x < state.appearance.width; ++x) {
                const double nx = (static_cast<double>(x) + 0.5) / 24.0 - 1.0;
                const double ny = (static_cast<double>(y) + 0.5) / 16.0 - 1.0;
                const double radius = std::sqrt(nx * nx + ny * ny);
                const auto alpha = static_cast<std::uint8_t>(std::lround(
                    std::clamp((1.0 - radius) / 0.22, 0.0, 1.0) * 255.0));
                const auto offset = (static_cast<std::size_t>(y) * 48U + x) * 4U;
                const std::array color{
                    static_cast<std::uint8_t>(30U + state_index * 12U),
                    static_cast<std::uint8_t>(58U + state_index * 18U),
                    static_cast<std::uint8_t>(100U + state_index * 28U),
                };
                for (std::size_t channel = 0U; channel < color.size(); ++channel) {
                    state.appearance.premultiplied_bgra[offset + channel] =
                        static_cast<std::uint8_t>(
                            (static_cast<std::uint32_t>(color[channel]) * alpha + 127U) / 255U);
                }
                state.appearance.premultiplied_bgra[offset + 3U] = alpha;
            }
        }
        command.atlas.states.push_back(std::move(state));
    }
    return command;
}

} // namespace

int wmain(const int argc, wchar_t** argv) {
    if (argc != 2) {
        std::cerr << "expected worker executable path\n";
        return 2;
    }
    SessionBindingV1 session{};
    for (std::size_t index = 0U; index < session.nonce.size(); ++index) {
        session.nonce[index] = static_cast<std::byte>(index + 17U);
    }
    session.session_id_high = 0x1001U;
    session.session_id_low = 0x1002U;
    const std::wstring worker_path = argv[1];
    const std::wstring pipe_name = L"\\\\.\\pipe\\npc-mouth-worker-smoke-" +
                                   std::to_wstring(GetCurrentProcessId()) + L"-" +
                                   std::to_wstring(GetTickCount64());
    const std::string nonce = nonce_hex(session);
    std::wstring command_line = quote(worker_path) + L" --pipe " + quote(pipe_name) +
        L" --nonce-hex " + std::wstring(nonce.begin(), nonce.end()) +
        L" --session-high " + std::to_wstring(session.session_id_high) +
        L" --session-low " + std::to_wstring(session.session_id_low) +
        L" --controller-pid " + std::to_wstring(GetCurrentProcessId()) +
        L" --generation 1";
    STARTUPINFOW startup{};
    startup.cb = sizeof(startup);
    PROCESS_INFORMATION process_info{};
    if (!CreateProcessW(worker_path.c_str(), command_line.data(), nullptr, nullptr, FALSE,
                        CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process_info)) {
        std::cerr << "failed to launch worker: " << GetLastError() << '\n';
        return 1;
    }
    UniqueHandle process(process_info.hProcess);
    UniqueHandle thread(process_info.hThread);
    bool clean_shutdown = false;
    const auto terminate_on_exit = [&] {
        if (!clean_shutdown && WaitForSingleObject(process.get(), 0U) == WAIT_TIMEOUT) {
            TerminateProcess(process.get(), 99U);
            WaitForSingleObject(process.get(), 5'000U);
        }
    };

    bool pipe_ready = false;
    const auto pipe_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(10);
    while (std::chrono::steady_clock::now() < pipe_deadline) {
        if (WaitNamedPipeW(pipe_name.c_str(), 100U)) {
            pipe_ready = true;
            break;
        }
        if (GetLastError() != ERROR_FILE_NOT_FOUND && GetLastError() != ERROR_SEM_TIMEOUT) break;
        Sleep(10U);
    }
    if (!pipe_ready) {
        expect(false, "worker named pipe becomes ready");
        DWORD early_exit = STILL_ACTIVE;
        GetExitCodeProcess(process.get(), &early_exit);
        std::cerr << "WaitNamedPipe error=" << GetLastError()
                  << " worker_exit=" << early_exit << '\n';
        terminate_on_exit();
        return 1;
    }
    UniqueHandle pipe(CreateFileW(pipe_name.c_str(), GENERIC_READ | GENERIC_WRITE, 0U, nullptr,
                                  OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr));
    expect(static_cast<bool>(pipe), "controller connects to local worker pipe");
    if (!pipe) {
        terminate_on_exit();
        return 1;
    }
    const auto health = request(pipe.get(), envelope(session, CommandKind::health, 1U, 1U));
    expect(health && health->status == StatusCode::ok && health->detail == "ready",
           "authenticated health request succeeds");

    auto d3d = create_d3d_fixture();
    expect(d3d.has_value(), "hardware D3D11 adapter is available");
    if (!d3d) {
        terminate_on_exit();
        return 1;
    }
    constexpr std::uint32_t width = 320U;
    constexpr std::uint32_t height = 180U;
    std::vector<std::uint8_t> pixels(width * height * 4U);
    for (std::uint32_t y = 0U; y < height; ++y) {
        for (std::uint32_t x = 0U; x < width; ++x) {
            const auto offset = (static_cast<std::size_t>(y) * width + x) * 4U;
            pixels[offset + 0U] = static_cast<std::uint8_t>(40U + x % 80U);
            pixels[offset + 1U] = static_cast<std::uint8_t>(80U + y % 80U);
            pixels[offset + 2U] = 170U;
            pixels[offset + 3U] = 255U;
        }
    }
    D3D11_TEXTURE2D_DESC source_description{};
    source_description.Width = width;
    source_description.Height = height;
    source_description.MipLevels = 1U;
    source_description.ArraySize = 1U;
    source_description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    source_description.SampleDesc.Count = 1U;
    source_description.Usage = D3D11_USAGE_DEFAULT;
    source_description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    source_description.MiscFlags = D3D11_RESOURCE_MISC_SHARED_NTHANDLE |
                                   D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX;
    ComPtr<ID3D11Texture2D> source_texture;
    expect(SUCCEEDED(d3d->device->CreateTexture2D(&source_description, nullptr, &source_texture)),
           "broker fixture creates shared source texture");
    ComPtr<IDXGIKeyedMutex> source_mutex;
    ComPtr<IDXGIResource1> source_resource;
    HANDLE source_handle{};
    expect(SUCCEEDED(source_texture.As(&source_mutex)) &&
               source_mutex->AcquireSync(0U, 100U) == S_OK,
           "broker fixture acquires source keyed mutex");
    d3d->context->UpdateSubresource(source_texture.Get(), 0U, nullptr, pixels.data(), width * 4U, 0U);
    expect(SUCCEEDED(source_mutex->ReleaseSync(1U)), "broker publishes current source frame");
    expect(SUCCEEDED(source_texture.As(&source_resource)) &&
               SUCCEEDED(source_resource->CreateSharedHandle(
                   nullptr, DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE,
                   nullptr, &source_handle)),
           "broker fixture creates source NT handle");
    UniqueHandle local_source_handle(source_handle);
    HANDLE worker_source_handle{};
    expect(DuplicateHandle(GetCurrentProcess(), local_source_handle.get(), process.get(),
                           &worker_source_handle, 0U, FALSE, DUPLICATE_SAME_ACCESS) != FALSE,
           "broker duplicates source handle only into exact worker process");

    std::array<wchar_t, 32'768> controller_path{};
    const DWORD controller_length = GetModuleFileNameW(nullptr, controller_path.data(),
                                                       static_cast<DWORD>(controller_path.size()));
    RenderCurrentFrameCommandV1 render{};
    render.request = {77U, session.session_id_high, session.session_id_low, 88U, 89U, 90U};
    render.source.schema_version = 1U;
    render.source.session = session;
    render.source.texture.schema_version = 1U;
    render.source.texture.transport = LeaseTransport::d3d11_shared_nt_handle;
    render.source.texture.lease_nonce_high = 0xabcU;
    render.source.texture.lease_nonce_low = 0xdefU;
    render.source.texture.owner_process_id = GetCurrentProcessId();
    render.source.texture.intended_consumer_process_id = process_info.dwProcessId;
    render.source.texture.native_handle_value = reinterpret_cast<std::uint64_t>(worker_source_handle);
    render.source.texture.adapter_luid_low = d3d->luid_low;
    render.source.texture.adapter_luid_high = d3d->luid_high;
    render.source.texture.keyed_mutex_acquire_key = 1U;
    render.source.texture.keyed_mutex_release_key = 2U;
    render.source.texture.width = width;
    render.source.texture.height = height;
    render.source.texture.stride_bytes = width * 4U;
    render.source.texture.format = PixelFormat::bgra8_unorm_premultiplied;
    render.source.broker_process_id = GetCurrentProcessId();
    render.source.broker_process_creation_time = creation_time(GetCurrentProcess());
    const auto basename = std::filesystem::path(
        std::wstring(controller_path.data(), controller_length)).filename().wstring();
    const auto basename_ascii = narrow_ascii(basename);
    expect(basename_ascii.has_value(), "test controller basename is ASCII");
    render.source.broker_executable_name = basename_ascii.value_or("invalid.exe");
    render.source.source_frame_qpc = qpc_now();
    render.source.qpc_frequency = qpc_frequency();
    const Nanoseconds captured = monotonic_ns();
    render.source.texture.expires_at_ns = captured + 500'000'000;
    render.track = {1U, 41U, 42U, 43U};
    render.frame = {51U, 52U, 53U, captured};
    render.landmarks.schema_version = 1U;
    render.landmarks.provider_instance_id = 54U;
    render.landmarks.track = render.track;
    render.landmarks.frame = render.frame;
    render.landmarks.source_frame_qpc = render.source.source_frame_qpc;
    render.landmarks.qpc_frequency = render.source.qpc_frequency;
    render.landmarks.face_bounds = {0.30, 0.16, 0.40, 0.68};
    for (auto& point : render.landmarks.landmarks) point = {0.50, 0.45, 0.96};
    render.landmarks.landmarks[48U] = {0.43, 0.60, 0.97};
    render.landmarks.landmarks[49U] = {0.46, 0.58, 0.97};
    render.landmarks.landmarks[50U] = {0.48, 0.57, 0.97};
    render.landmarks.landmarks[51U] = {0.50, 0.565, 0.97};
    render.landmarks.landmarks[52U] = {0.52, 0.57, 0.97};
    render.landmarks.landmarks[53U] = {0.54, 0.58, 0.97};
    render.landmarks.landmarks[54U] = {0.57, 0.60, 0.97};
    render.landmarks.landmarks[55U] = {0.54, 0.64, 0.97};
    render.landmarks.landmarks[56U] = {0.52, 0.655, 0.97};
    render.landmarks.landmarks[57U] = {0.50, 0.66, 0.97};
    render.landmarks.landmarks[58U] = {0.48, 0.655, 0.97};
    render.landmarks.landmarks[59U] = {0.46, 0.64, 0.97};
    render.landmarks.landmarks[58U] = {0.57, 0.60, 0.96};
    render.landmarks.landmarks[59U] = {0.54, 0.585, 0.96};
    render.landmarks.landmarks[60U] = {0.50, 0.578, 0.96};
    render.landmarks.landmarks[61U] = {0.46, 0.585, 0.96};
    render.landmarks.landmarks[62U] = {0.43, 0.60, 0.96};
    render.landmarks.landmarks[63U] = {0.46, 0.63, 0.96};
    render.landmarks.landmarks[64U] = {0.50, 0.642, 0.96};
    render.landmarks.landmarks[65U] = {0.54, 0.63, 0.96};
    render.landmarks.detector_confidence = 0.96;
    render.landmarks.landmark_confidence = 0.95;
    render.landmarks.visibility_ratio = 0.94;
    render.landmarks.measured_at_ns = captured;
    render.appearance = {1U, 41U, 1U, 101U, 102U, 103U, 104U,
                         0.94, 0.91, 0.01, true, true, false};
    render.resources = {1U, VisualPressure::nominal, 15U, true};
    render.drive.kind = DriveKind::timed_viseme;
    render.drive.clock = {1U, 90U, 0U, 800U, 0U, 24'000U, 1U, captured};
    render.drive.viseme = Viseme::open_vowel;
    render.drive.viseme_strength = 0.8;
    render.deadline_ns = captured + 500'000'000;
    const auto encoded_atlas = encode_character_mouth_atlas(service_atlas(1U, 41U));
    expect(encoded_atlas.has_value(), "bounded identity atlas encodes for service transport");
    const auto installed_atlas = request(pipe.get(), envelope(
        session, CommandKind::install_character_mouth_atlas, 2U, 1U,
        encoded_atlas.value_or(std::vector<std::byte>{})));
    expect(installed_atlas && installed_atlas->status == StatusCode::ok &&
               installed_atlas->detail == "character_mouth_atlas_ready",
           "authenticated service installs an exact actor-bound mouth atlas");
    const auto encoded_render = encode_render_command(render);
    auto rendered = request(pipe.get(), envelope(session,
        CommandKind::render_current_frame, 3U, 1U, *encoded_render));
    std::uint64_t next_sequence = 4U;
    if (rendered && rendered->status == StatusCode::ok && !rendered->residual &&
        rendered->receipt.worker_disposition == Disposition::bypass_stale_frame) {
        // First-use D3D adapter initialization may outlive the hard 80 ms frame
        // budget. The product retries only with a newly captured source frame;
        // it never presents the stale result.
        expect(source_mutex->AcquireSync(2U, 100U) == S_OK,
               "broker reacquires source texture for a new current frame");
        d3d->context->UpdateSubresource(source_texture.Get(), 0U, nullptr,
                                        pixels.data(), width * 4U, 0U);
        expect(SUCCEEDED(source_mutex->ReleaseSync(3U)),
               "broker publishes replacement current frame");
        HANDLE replacement_worker_handle{};
        expect(DuplicateHandle(GetCurrentProcess(), local_source_handle.get(), process.get(),
                               &replacement_worker_handle, 0U, FALSE,
                               DUPLICATE_SAME_ACCESS) != FALSE,
               "broker duplicates replacement source handle into worker");
        const Nanoseconds replacement_captured = monotonic_ns();
        render.source.texture.native_handle_value =
            reinterpret_cast<std::uint64_t>(replacement_worker_handle);
        render.source.texture.lease_nonce_high = 0xabdU;
        render.source.texture.keyed_mutex_acquire_key = 3U;
        render.source.texture.keyed_mutex_release_key = 4U;
        render.source.texture.expires_at_ns = replacement_captured + 500'000'000;
        render.source.source_frame_qpc = qpc_now();
        render.frame.sequence += 1U;
        render.frame.captured_at_ns = replacement_captured;
        render.landmarks.frame = render.frame;
        render.landmarks.source_frame_qpc = render.source.source_frame_qpc;
        render.landmarks.measured_at_ns = replacement_captured;
        render.drive.clock.playback_at_ns = replacement_captured;
        render.deadline_ns = replacement_captured + 500'000'000;
        rendered = request(pipe.get(), envelope(session, CommandKind::render_current_frame,
            next_sequence++, 1U, *encode_render_command(render)));
    }
    expect(rendered && rendered->status == StatusCode::ok && rendered->residual &&
               rendered->receipt.disposition == PresentationDisposition::residual_proposed,
           "authenticated service returns a worker-owned residual proposal");
    if (!rendered) {
        std::cerr << "render response missing\n";
    } else if (rendered->status != StatusCode::ok || !rendered->residual) {
        std::cerr << "render status=" << to_string(rendered->status)
                  << " detail=" << rendered->detail
                  << " receipt=" << npc::mouth::to_string(rendered->receipt.disposition)
                  << '\n';
    }
    if (rendered && rendered->residual) {
        expect(rendered->residual->schema_version == 3U &&
                   rendered->residual->audio_clock.segment_id == 90U &&
                   rendered->residual->audio_clock.sample_count == 800U &&
                   rendered->residual->audio_clock.playback_at_ns ==
                       render.frame.captured_at_ns,
               "service proposal binds the residual to the exact admitted audio interval");
        HANDLE local_residual_handle{};
        expect(DuplicateHandle(process.get(),
                               reinterpret_cast<HANDLE>(
                                   rendered->residual->residual.native_handle_value),
                               GetCurrentProcess(), &local_residual_handle, 0U, FALSE,
                               DUPLICATE_SAME_ACCESS) != FALSE,
               "broker duplicates residual only from exact worker process");
        UniqueHandle residual_handle(local_residual_handle);
        expect(verify_residual_texture(*d3d, residual_handle.get(),
                                       rendered->residual->residual),
               "broker opens, synchronizes, and reads a nonempty mouth-only residual");
        const AcknowledgeResidualCommandV1 ack{
            rendered->residual->residual.lease_nonce_high,
            rendered->residual->residual.lease_nonce_low,
            true,
        };
        const auto acknowledged = request(pipe.get(), envelope(session,
            CommandKind::acknowledge_residual, next_sequence++, 1U,
            encode_acknowledgement(ack)));
        expect(acknowledged && acknowledged->status == StatusCode::ok,
               "broker presentation acknowledgement releases worker lease");
        const auto replayed = request(pipe.get(), envelope(session,
            CommandKind::acknowledge_residual, next_sequence++, 1U,
            encode_acknowledgement(ack)));
        expect(replayed && replayed->status == StatusCode::payload_invalid,
               "residual acknowledgement replay is rejected");
    }
    const auto cancelled = request(pipe.get(), envelope(session,
        CommandKind::cancel_generation, next_sequence++, 1U,
        encode_cancel_command(CancelGenerationCommandV1{2U})));
    expect(cancelled && cancelled->status == StatusCode::ok &&
               cancelled->cancellation_generation == 2U,
           "generation cancellation advances and releases all residuals");
    const auto shutdown = request(pipe.get(), envelope(session,
        CommandKind::shutdown, next_sequence++, 2U));
    expect(shutdown && shutdown->status == StatusCode::ok, "authenticated shutdown succeeds");
    pipe.reset();
    expect(WaitForSingleObject(process.get(), 10'000U) == WAIT_OBJECT_0,
           "worker exits after clean shutdown");
    DWORD exit_code{};
    expect(GetExitCodeProcess(process.get(), &exit_code) && exit_code == 0U,
           "worker returns success after service lifecycle");
    clean_shutdown = true;
    terminate_on_exit();

    // Exercise the actual service command that owns native landmark inference.
    // A deterministic fake is injected at the provider ABI; no model/runtime
    // is loaded by this test.
    SessionBindingV1 native_session{};
    for (std::size_t index = 0U; index < native_session.nonce.size(); ++index) {
        native_session.nonce[index] = static_cast<std::byte>(index + 73U);
    }
    native_session.session_id_high = 0x2001U;
    native_session.session_id_low = 0x2002U;
    const std::wstring native_pipe_name = L"\\\\.\\pipe\\npc-mouth-worker-native-provider-" +
        std::to_wstring(GetCurrentProcessId()) + L"-" + std::to_wstring(GetTickCount64());
    WindowsServiceConfig native_config{};
    native_config.pipe_name = native_pipe_name;
    native_config.session = native_session;
    native_config.expected_controller_process_id = GetCurrentProcessId();
    native_config.initial_generation = 1U;
    int native_service_exit = -1;
    std::thread native_service([&] {
        native_service_exit = run_windows_service(
            native_config, std::make_unique<FakeAdmittedLandmarkProvider>());
    });
    bool native_pipe_ready = false;
    const auto native_pipe_deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(10);
    while (std::chrono::steady_clock::now() < native_pipe_deadline) {
        if (WaitNamedPipeW(native_pipe_name.c_str(), 100U)) {
            native_pipe_ready = true;
            break;
        }
        Sleep(10U);
    }
    expect(native_pipe_ready, "in-process admitted-provider service becomes ready");
    UniqueHandle native_pipe(CreateFileW(native_pipe_name.c_str(), GENERIC_READ | GENERIC_WRITE,
                                         0U, nullptr, OPEN_EXISTING,
                                         FILE_ATTRIBUTE_NORMAL, nullptr));
    expect(static_cast<bool>(native_pipe), "controller connects to admitted-provider service");
    std::uint64_t native_sequence = 1U;
    const auto native_health = request(native_pipe.get(), envelope(
        native_session, CommandKind::health, native_sequence++, 1U));
    expect(native_health && native_health->status == StatusCode::ok,
           "admitted-provider service authenticates health");
    auto self_test_launch = fake_admitted_launch();
    self_test_launch.schema_version = 2U;
    self_test_launch.exact_target_process_id = 0U;
    self_test_launch.provider_load_self_test = true;
    const auto self_test_payload = encode_provider_configuration(
        ConfigureAdmittedLandmarkProviderCommandV1{self_test_launch});
    expect(self_test_payload.has_value(),
           "setup provider-load payload carries no target authority");
    const auto self_test = request(native_pipe.get(), envelope(
        native_session, CommandKind::self_test_admitted_landmark_provider,
        native_sequence++, 1U, *self_test_payload));
    expect(self_test && self_test->status == StatusCode::ok &&
               self_test->detail ==
                   "admitted_landmark_provider_load_self_test_passed",
           "authenticated setup command loads and unloads the admitted provider");
    const auto setup_on_runtime_command = request(native_pipe.get(), envelope(
        native_session, CommandKind::configure_admitted_landmark_provider,
        native_sequence++, 1U, *self_test_payload));
    expect(setup_on_runtime_command &&
               setup_on_runtime_command->status == StatusCode::payload_invalid,
           "runtime configure command rejects setup-only provider purpose");
    const auto runtime_payload = encode_provider_configuration(
        ConfigureAdmittedLandmarkProviderCommandV1{fake_admitted_launch()});
    const auto runtime_on_setup_command = request(native_pipe.get(), envelope(
        native_session, CommandKind::self_test_admitted_landmark_provider,
        native_sequence++, 1U, *runtime_payload));
    expect(runtime_on_setup_command &&
               runtime_on_setup_command->status == StatusCode::payload_invalid,
           "setup command rejects runtime target authority");
    const auto configured = request(native_pipe.get(), envelope(
        native_session, CommandKind::configure_admitted_landmark_provider,
        native_sequence++, 1U,
        *runtime_payload));
    expect(configured && configured->status == StatusCode::ok &&
               configured->detail == "admitted_landmark_provider_ready",
           "authenticated controller configures the exact admitted provider authority");

    HANDLE native_source_handle{};
    expect(DuplicateHandle(GetCurrentProcess(), local_source_handle.get(), GetCurrentProcess(),
                           &native_source_handle, 0U, FALSE, DUPLICATE_SAME_ACCESS) != FALSE,
           "same-process service receives a distinct owned source handle");
    const Nanoseconds native_captured = monotonic_ns();
    RenderWithAdmittedLandmarksCommandV1 native_render{};
    native_render.request = {177U, native_session.session_id_high,
                             native_session.session_id_low, 188U, 189U, 190U};
    native_render.source = render.source;
    native_render.source.session = native_session;
    native_render.source.texture.lease_nonce_high = 0xbcdU;
    native_render.source.texture.lease_nonce_low = 0xef1U;
    native_render.source.texture.intended_consumer_process_id = GetCurrentProcessId();
    native_render.source.texture.native_handle_value =
        reinterpret_cast<std::uint64_t>(native_source_handle);
    native_render.source.texture.keyed_mutex_acquire_key =
        render.source.texture.keyed_mutex_release_key;
    native_render.source.texture.keyed_mutex_release_key =
        render.source.texture.keyed_mutex_release_key + 1U;
    native_render.source.texture.expires_at_ns = native_captured + 500'000'000;
    native_render.source.source_frame_qpc = qpc_now();
    native_render.track = {1U, 141U, 142U, 143U};
    native_render.frame = {151U, 152U, 153U, native_captured};
    native_render.seed_face_bounds = {0.30, 0.16, 0.40, 0.68};
    native_render.appearance = {1U, 141U, 1U, 201U, 202U, 203U, 204U,
                                0.94, 0.91, 0.01, true, true, false};
    native_render.resources = {1U, VisualPressure::nominal, 15U, true};
    native_render.drive = render.drive;
    native_render.drive.clock.stream_generation = 1U;
    native_render.drive.clock.segment_id = 190U;
    native_render.drive.clock.playback_at_ns = native_captured;
    native_render.deadline_ns = native_captured + 500'000'000;
    const auto native_rendered = request(native_pipe.get(), envelope(
        native_session, CommandKind::render_with_admitted_landmarks, native_sequence++, 1U,
        *encode_admitted_render_command(native_render)));
    expect(native_rendered && native_rendered->status == StatusCode::ok &&
               native_rendered->residual &&
               native_rendered->receipt.disposition == PresentationDisposition::residual_proposed,
           "service reads the exact lease, invokes admitted native provider, and proposes residual");
    if (!native_rendered) {
        std::cerr << "native provider response missing\n";
    } else if (native_rendered->status != StatusCode::ok || !native_rendered->residual) {
        std::cerr << "native provider status=" << to_string(native_rendered->status)
                  << " detail=" << native_rendered->detail
                  << " receipt="
                  << npc::mouth::to_string(native_rendered->receipt.disposition) << '\n';
    }
    if (native_rendered && native_rendered->residual) {
        expect(verify_residual_texture(*d3d,
                   reinterpret_cast<HANDLE>(
                       native_rendered->residual->residual.native_handle_value),
                   native_rendered->residual->residual),
               "native-provider service residual remains current-frame bounded");
        const AcknowledgeResidualCommandV1 native_ack{
            native_rendered->residual->residual.lease_nonce_high,
            native_rendered->residual->residual.lease_nonce_low,
            true,
        };
        const auto acknowledged = request(native_pipe.get(), envelope(
            native_session, CommandKind::acknowledge_residual, native_sequence++, 1U,
            encode_acknowledgement(native_ack)));
        expect(acknowledged && acknowledged->status == StatusCode::ok,
               "native-provider residual is explicitly acknowledged");
    }
    const auto native_shutdown = request(native_pipe.get(), envelope(
        native_session, CommandKind::shutdown, native_sequence++, 1U));
    expect(native_shutdown && native_shutdown->status == StatusCode::ok,
           "native-provider service shuts down cleanly");
    native_pipe.reset();
    native_service.join();
    expect(native_service_exit == 0, "native-provider service unloads provider on shutdown");

    if (failures != 0) {
        std::cerr << failures << " Windows service smoke assertion(s) failed\n";
        return 1;
    }
    std::cout << "PASS: authenticated cross-process D3D mouth-worker service lifecycle\n";
    return 0;
}
