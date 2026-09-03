#ifdef _WIN32

#include "npc/media_broker/platform.hpp"

#include "npc/media_broker/geometry.hpp"
#include "windows_audio.hpp"
#include "windows_capture.hpp"
#include "windows_identity_reference.hpp"
#include "windows_manual_actor_picker.hpp"
#include "windows_overlay.hpp"

#include <Windows.h>
#include <bcrypt.h>
#include <d3d11.h>
#include <d3d11_1.h>
#include <dwmapi.h>
#include <dxgi1_6.h>
#include <sddl.h>
#include <wrl/client.h>

#include <atomic>
#include <algorithm>
#include <array>
#include <bit>
#include <cctype>
#include <cwchar>
#include <cstring>
#include <deque>
#include <iomanip>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <string_view>
#include <utility>

namespace npc::media {

using Microsoft::WRL::ComPtr;

namespace {

struct HandleCloser {
    void operator()(void* handle) const noexcept {
        if (handle && handle != INVALID_HANDLE_VALUE) {
            CloseHandle(handle);
        }
    }
};

using UniqueHandle = std::unique_ptr<void, HandleCloser>;

[[nodiscard]] std::uint64_t pack_file_time(const FILETIME value) noexcept {
    return (static_cast<std::uint64_t>(value.dwHighDateTime) << 32U) |
           static_cast<std::uint64_t>(value.dwLowDateTime);
}

[[nodiscard]] std::uint64_t pack_luid(const LUID value) noexcept {
    return (static_cast<std::uint64_t>(static_cast<std::uint32_t>(value.HighPart)) << 32U) |
           static_cast<std::uint64_t>(value.LowPart);
}

[[nodiscard]] std::string utf8_from_wide(const std::wstring_view value) {
    if (value.empty()) return {};
    const auto size = WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()),
                                          nullptr, 0, nullptr, nullptr);
    if (size <= 0) return {};
    std::string result(static_cast<std::size_t>(size), '\0');
    WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()),
                        result.data(), size, nullptr, nullptr);
    return result;
}

[[nodiscard]] std::string process_basename(const DWORD process_id) {
    const HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id);
    if (!process) return {};
    std::wstring path(32768, L'\0');
    DWORD size = static_cast<DWORD>(path.size());
    const bool queried = QueryFullProcessImageNameW(process, 0, path.data(), &size) != FALSE;
    CloseHandle(process);
    if (!queried || size == 0) return {};
    path.resize(size);
    const auto separator = path.find_last_of(L"\\/");
    const auto name = separator == std::wstring::npos ? std::wstring_view{path}
                                                       : std::wstring_view{path}.substr(separator + 1);
    return utf8_from_wide(name);
}

[[nodiscard]] std::uint64_t process_creation_time(const HANDLE process) noexcept {
    FILETIME created{}, exited{}, kernel{}, user{};
    return GetProcessTimes(process, &created, &exited, &kernel, &user)
        ? pack_file_time(created)
        : 0U;
}

[[nodiscard]] bool random_lease_nonce(std::uint64_t& high,
                                     std::uint64_t& low) noexcept {
    std::array<std::byte, 16> bytes{};
    if (BCryptGenRandom(nullptr, reinterpret_cast<PUCHAR>(bytes.data()),
                        static_cast<ULONG>(bytes.size()),
                        BCRYPT_USE_SYSTEM_PREFERRED_RNG) != 0) {
        return false;
    }
    std::memcpy(&high, bytes.data(), sizeof(high));
    std::memcpy(&low, bytes.data() + sizeof(high), sizeof(low));
    if (high == 0U && low == 0U) low = 1U;
    return true;
}

[[nodiscard]] std::string hex_nonce(const std::uint64_t high, const std::uint64_t low) {
    std::ostringstream stream;
    stream << std::hex << std::setfill('0') << std::setw(16) << high << std::setw(16) << low;
    return stream.str();
}

[[nodiscard]] std::string sha256_hex(const std::span<const std::byte> bytes) {
    BCRYPT_ALG_HANDLE algorithm{};
    BCRYPT_HASH_HANDLE hash{};
    std::array<std::byte, 32> digest{};
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) != 0) {
        return {};
    }
    const auto close_algorithm = [&] { BCryptCloseAlgorithmProvider(algorithm, 0); };
    DWORD object_length{}, returned{};
    if (BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                          reinterpret_cast<PUCHAR>(&object_length), sizeof(object_length),
                          &returned, 0) != 0 || object_length == 0U) {
        close_algorithm();
        return {};
    }
    std::vector<std::byte> object(object_length);
    if (BCryptCreateHash(algorithm, &hash, reinterpret_cast<PUCHAR>(object.data()),
                         object_length, nullptr, 0, 0) != 0 ||
        BCryptHashData(hash, const_cast<PUCHAR>(reinterpret_cast<const UCHAR*>(bytes.data())),
                       static_cast<ULONG>(bytes.size()), 0) != 0 ||
        BCryptFinishHash(hash, reinterpret_cast<PUCHAR>(digest.data()),
                         static_cast<ULONG>(digest.size()), 0) != 0) {
        if (hash) BCryptDestroyHash(hash);
        close_algorithm();
        return {};
    }
    BCryptDestroyHash(hash);
    close_algorithm();
    std::ostringstream stream;
    stream << std::hex << std::setfill('0');
    for (const auto value : digest) stream << std::setw(2) << std::to_integer<unsigned>(value);
    return stream.str();
}

[[nodiscard]] std::wstring wide_from_utf8(const std::string_view value) {
    if (value.empty()) return {};
    const auto count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                           static_cast<int>(value.size()), nullptr, 0);
    if (count <= 0) return {};
    std::wstring result(static_cast<std::size_t>(count), L'\0');
    return MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                               static_cast<int>(value.size()), result.data(), count) == count
        ? result : std::wstring{};
}

[[nodiscard]] bool same_user_and_session(const HANDLE process) {
    DWORD current_session{}, worker_session{};
    if (!ProcessIdToSessionId(GetCurrentProcessId(), &current_session) ||
        !ProcessIdToSessionId(GetProcessId(process), &worker_session) ||
        current_session != worker_session) {
        return false;
    }
    HANDLE current_token_raw{}, worker_token_raw{};
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &current_token_raw) ||
        !OpenProcessToken(process, TOKEN_QUERY, &worker_token_raw)) {
        if (current_token_raw) CloseHandle(current_token_raw);
        if (worker_token_raw) CloseHandle(worker_token_raw);
        return false;
    }
    UniqueHandle current_token(current_token_raw);
    UniqueHandle worker_token(worker_token_raw);
    const auto read_sid = [](const HANDLE token, std::vector<std::byte>& storage) -> PSID {
        DWORD needed{};
        GetTokenInformation(token, TokenUser, nullptr, 0, &needed);
        if (needed == 0U) return nullptr;
        storage.resize(needed);
        return GetTokenInformation(token, TokenUser, storage.data(), needed, &needed)
            ? reinterpret_cast<TOKEN_USER*>(storage.data())->User.Sid : nullptr;
    };
    std::vector<std::byte> current_storage, worker_storage;
    const auto current_sid = read_sid(current_token.get(), current_storage);
    const auto worker_sid = read_sid(worker_token.get(), worker_storage);
    return current_sid && worker_sid && EqualSid(current_sid, worker_sid);
}

struct LocalSecurityDescriptor {
    PSECURITY_DESCRIPTOR descriptor{};
    SECURITY_ATTRIBUTES attributes{sizeof(SECURITY_ATTRIBUTES), nullptr, FALSE};
    ~LocalSecurityDescriptor() { if (descriptor) LocalFree(descriptor); }
};

[[nodiscard]] bool read_only_same_user_mapping_security(LocalSecurityDescriptor& security) {
    HANDLE token_raw{};
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token_raw)) return false;
    UniqueHandle token(token_raw);
    DWORD needed{};
    GetTokenInformation(token.get(), TokenUser, nullptr, 0, &needed);
    if (needed == 0U) return false;
    std::vector<std::byte> storage(needed);
    if (!GetTokenInformation(token.get(), TokenUser, storage.data(), needed, &needed)) return false;
    LPWSTR sid_text{};
    const auto sid = reinterpret_cast<TOKEN_USER*>(storage.data())->User.Sid;
    if (!ConvertSidToStringSidW(sid, &sid_text)) return false;
    const std::wstring sddl = std::wstring{L"D:P(A;;GR;;;"} + sid_text + L")";
    LocalFree(sid_text);
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.c_str(), SDDL_REVISION_1, &security.descriptor, nullptr)) {
        return false;
    }
    security.attributes.lpSecurityDescriptor = security.descriptor;
    return true;
}

[[nodiscard]] std::uint64_t captured_unix_ms(const std::uint64_t frame_qpc,
                                             const std::uint64_t now_qpc,
                                             const std::uint64_t frequency) noexcept {
    FILETIME file_time{};
    GetSystemTimeAsFileTime(&file_time);
    constexpr std::uint64_t windows_to_unix_100ns = 116444736000000000ULL;
    const auto now_100ns = pack_file_time(file_time);
    if (now_100ns <= windows_to_unix_100ns || frame_qpc > now_qpc || frequency == 0U) return 0U;
    const auto age_ms = (now_qpc - frame_qpc) * 1000U / frequency;
    const auto now_ms = (now_100ns - windows_to_unix_100ns) / 10000U;
    return age_ms <= now_ms ? now_ms - age_ms : 0U;
}

[[nodiscard]] bool equal_ascii_case_insensitive(const std::string_view left,
                                                const std::string_view right) noexcept {
    return left.size() == right.size() && std::equal(left.begin(), left.end(), right.begin(),
        [](const char a, const char b) {
            const auto lower = [](const unsigned char value) {
                return value >= 'A' && value <= 'Z' ? static_cast<unsigned char>(value + ('a' - 'A')) : value;
            };
            return lower(static_cast<unsigned char>(a)) == lower(static_cast<unsigned char>(b));
        });
}

[[nodiscard]] std::string manual_actor_candidate_digest(
    const std::span<const ManualActorCandidate> candidates) {
    std::vector<std::byte> canonical;
    canonical.reserve(candidates.size() * 56U);
    const auto append_u64 = [&](const std::uint64_t value) {
        for (unsigned shift = 0; shift < 64U; shift += 8U) {
            canonical.push_back(static_cast<std::byte>((value >> shift) & 0xffU));
        }
    };
    for (const auto& candidate : candidates) {
        append_u64(candidate.actor_id);
        append_u64(candidate.track_id);
        append_u64(candidate.track_epoch);
        append_u64(std::bit_cast<std::uint64_t>(candidate.normalized_bounds.left));
        append_u64(std::bit_cast<std::uint64_t>(candidate.normalized_bounds.top));
        append_u64(std::bit_cast<std::uint64_t>(candidate.normalized_bounds.right));
        append_u64(std::bit_cast<std::uint64_t>(candidate.normalized_bounds.bottom));
    }
    return sha256_hex(canonical);
}

[[nodiscard]] bool same_material_geometry(const TargetGeometry& left,
                                          const TargetGeometry& right) noexcept {
    return left.window_bounds_px == right.window_bounds_px &&
           left.client_bounds_px == right.client_bounds_px &&
           left.captured_desktop_bounds_px == right.captured_desktop_bounds_px &&
           left.captured_content_px == right.captured_content_px &&
           left.monitor.stable_id == right.monitor.stable_id &&
           left.monitor.desktop_bounds_px == right.monitor.desktop_bounds_px &&
           left.monitor.work_area_px == right.monitor.work_area_px &&
           left.monitor.dpi_x == right.monitor.dpi_x && left.monitor.dpi_y == right.monitor.dpi_y &&
           left.monitor.color_space == right.monitor.color_space &&
           left.monitor.rotation == right.monitor.rotation && left.minimized == right.minimized;
}

[[nodiscard]] Failure hresult_failure(const FailureDomain domain,
                                      const FailureCode code,
                                      const HRESULT value,
                                      const char* context,
                                      const bool retryable = true) {
    return {domain, code, retryable,
            std::string(context) + " (HRESULT=" + std::to_string(static_cast<long>(value)) + ")"};
}

[[nodiscard]] TargetGeometry query_target_geometry(const HWND window) {
    RECT window_rect{};
    if (FAILED(DwmGetWindowAttribute(window, DWMWA_EXTENDED_FRAME_BOUNDS, &window_rect, sizeof(window_rect)))) {
        GetWindowRect(window, &window_rect);
    }

    RECT client{};
    GetClientRect(window, &client);
    POINT client_origin{client.left, client.top};
    ClientToScreen(window, &client_origin);
    const RectI client_bounds{
        client_origin.x,
        client_origin.y,
        client_origin.x + (client.right - client.left),
        client_origin.y + (client.bottom - client.top),
    };

    const HMONITOR monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
    MONITORINFOEXW info{};
    info.cbSize = sizeof(info);
    GetMonitorInfoW(monitor, &info);

    const UINT dpi = GetDpiForWindow(window);
    MonitorInfo monitor_info{
        utf8_from_wide(info.szDevice),
        {info.rcMonitor.left, info.rcMonitor.top, info.rcMonitor.right, info.rcMonitor.bottom},
        {info.rcWork.left, info.rcWork.top, info.rcWork.right, info.rcWork.bottom},
        dpi == 0 ? 96U : dpi,
        dpi == 0 ? 96U : dpi,
        ColorSpace::unknown,
        DisplayRotation::identity,
        80.0,
    };

    return {
        {window_rect.left, window_rect.top, window_rect.right, window_rect.bottom},
        client_bounds,
        client_bounds,
        {client_bounds.width(), client_bounds.height()},
        std::move(monitor_info),
        IsIconic(window) != FALSE,
    };
}

class WindowsMediaPlatform final : public IMediaPlatform {
public:
    struct ImportedResidual {
        SharedResidualLease lease;
        UniqueHandle worker_process;
        ComPtr<ID3D11Texture2D> texture;
        ComPtr<IDXGIKeyedMutex> keyed_mutex;
    };

    struct SeenLeaseNonce {
        std::uint64_t high{};
        std::uint64_t low{};
        std::uint64_t expires_qpc{};
    };

    struct ExportedVisualSource {
        UniqueHandle worker_process;
        std::uint32_t worker_process_id{};
        std::uint64_t lease_nonce_high{};
        std::uint64_t lease_nonce_low{};
        std::uint64_t expires_qpc{};
        std::uint64_t cancellation_generation{};
        std::uint64_t source_device_generation{};
        std::uint64_t source_geometry_epoch{};
        std::uint64_t source_frame_sequence{};
        std::uint64_t source_frame_qpc{};
        std::uint64_t actor_id{};
        std::uint64_t track_id{};
        std::uint64_t track_epoch{};
    };

    struct ExportedIdentityFrame {
        UniqueHandle mapping;
        UniqueHandle worker_process;
        std::uint32_t worker_process_id{};
        std::string lease_id;
        std::string lease_nonce;
        std::uint64_t expires_qpc{};
        bool reference_import{};
    };

    WindowsMediaPlatform() {
        const HRESULT result = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
        com_initialized_here_ = SUCCEEDED(result);
    }

    ~WindowsMediaPlatform() override {
        unregister_ptt_hotkey();
        shutdown_audio();
        stop_overlay();
        stop_capture();
        if (com_initialized_here_) {
            CoUninitialize();
        }
    }

    void set_callbacks(PlatformCallbacks callbacks) override { callbacks_ = std::move(callbacks); }

    bool validate_target(const GameTarget& target, Failure& failure) override {
        const auto window = reinterpret_cast<HWND>(target.native_window);
        DWORD actual_process{};
        if (!target.valid() || !IsWindow(window) || GetWindowThreadProcessId(window, &actual_process) == 0 ||
            actual_process != target.process_id) {
            failure = {FailureDomain::target, FailureCode::target_lost, false,
                       "HWND is missing, stale, or owned by a different process"};
            return false;
        }
        const auto actual_executable = process_basename(actual_process);
        if (actual_executable.empty() ||
            !equal_ascii_case_insensitive(actual_executable, target.executable_name)) {
            failure = {FailureDomain::target, FailureCode::target_lost, false,
                       "Selected executable name does not match the HWND owner"};
            return false;
        }
        // Serialize replacement of the native target bind against the final
        // picker guard/receipt transaction before publishing the new HWND.
        cancel_manual_actor_picker();
        target_window_ = window;
        return true;
    }

    bool start_capture(const GameTarget& target, const CaptureBackend backend, Failure& failure) override {
        stop_capture();
        target_window_ = reinterpret_cast<HWND>(target.native_window);
        last_target_geometry_.reset();
        last_reported_state_ = TargetState::none;
        if (!ensure_d3d_device(failure)) {
            return false;
        }

        if (backend == CaptureBackend::windows_graphics_capture) {
            if (!graphics_capture_.start(target_window_, d3d_device_.Get(), failure)) {
                return false;
            }
            wgc_suspended_for_minimize_ = false;
            capture_backend_ = backend;
            auto geometry = query_target_geometry(target_window_);
            geometry.captured_desktop_bounds_px = geometry.window_bounds_px;
            geometry.captured_content_px = {geometry.window_bounds_px.width(),
                                            geometry.window_bounds_px.height()};
            publish_geometry(std::move(geometry));
            return true;
        }

        if (backend != CaptureBackend::desktop_duplication) {
            failure = {FailureDomain::capture, FailureCode::backend_unavailable, false,
                       "No capture backend was selected"};
            return false;
        }

        ComPtr<IDXGIDevice> dxgi_device;
        HRESULT hr = d3d_device_.As(&dxgi_device);
        if (FAILED(hr)) {
            failure = hresult_failure(FailureDomain::capture, FailureCode::backend_unavailable, hr,
                                      "Query IDXGIDevice failed");
            return false;
        }
        ComPtr<IDXGIAdapter> adapter;
        dxgi_device->GetAdapter(&adapter);

        auto geometry = query_target_geometry(target_window_);
        for (UINT index = 0;; ++index) {
            ComPtr<IDXGIOutput> output;
            if (adapter->EnumOutputs(index, &output) == DXGI_ERROR_NOT_FOUND) {
                break;
            }
            DXGI_OUTPUT_DESC description{};
            output->GetDesc(&description);
            const RectI output_bounds{description.DesktopCoordinates.left,
                                      description.DesktopCoordinates.top,
                                      description.DesktopCoordinates.right,
                                      description.DesktopCoordinates.bottom};
            if (!intersect(output_bounds, geometry.client_bounds_px).valid()) {
                continue;
            }
            ComPtr<IDXGIOutput1> output1;
            if (SUCCEEDED(output.As(&output1))) {
                hr = output1->DuplicateOutput(d3d_device_.Get(), &duplication_);
                if (SUCCEEDED(hr)) {
                    geometry.captured_desktop_bounds_px = output_bounds;
                    geometry.captured_content_px = {output_bounds.width(), output_bounds.height()};
                    duplication_bounds_ = output_bounds;
                    duplication_monitor_id_ = geometry.monitor.stable_id;
                    capture_backend_ = backend;
                    publish_geometry(std::move(geometry));
                    return true;
                }
            }
        }
        failure = {FailureDomain::capture, FailureCode::backend_unavailable, true,
                   "Desktop Duplication could not open the output containing the target"};
        return false;
    }

    void stop_capture() noexcept override {
        cancel_manual_actor_picker();
        cancel_visual_source_leases();
        cancel_identity_frame_leases();
        release_shared_residual();
        graphics_capture_.stop();
        wgc_suspended_for_minimize_ = false;
        duplication_.Reset();
        duplication_bounds_ = {};
        duplication_monitor_id_.clear();
        latest_frame_texture_.Reset();
        capture_backend_ = CaptureBackend::none;
        target_window_ = nullptr;
        last_target_geometry_.reset();
        last_reported_state_ = TargetState::none;
        latest_frame_sequence_ = 0;
        latest_frame_qpc_ = 0;
        advancing_frame_verified_ = false;
        publish_picker_binding_snapshot();
    }

    bool start_overlay(const TargetGeometry& geometry, Failure& failure) override {
        if (capture_backend_ == CaptureBackend::desktop_duplication) {
            failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                       "Residual overlay is disabled during Desktop Duplication capture"};
            return false;
        }
        const auto mapped = calculate_overlay_geometry(geometry);
        if (!mapped) {
            failure = {FailureDomain::overlay, FailureCode::invalid_geometry, true,
                       "DirectComposition overlay received invalid target geometry"};
            return false;
        }
        if (!ensure_d3d_device(failure)) {
            return false;
        }
        if (!residual_overlay_.start(d3d_device_.Get(), *mapped, failure)) {
            return false;
        }
        overlay_geometry_ = geometry;
        return true;
    }

    void stop_overlay() noexcept override {
        release_shared_residual();
        residual_overlay_.stop();
        overlay_geometry_.reset();
    }

    bool overlay_capture_excluded() const noexcept override {
        return residual_overlay_.capture_excluded();
    }

    bool allocate_visual_source(const VisualSourceLeaseRequest& request,
                                VisualSourceLease& lease,
                                Failure& failure) override {
        lease = {};
        LARGE_INTEGER now_counter{};
        LARGE_INTEGER frequency{};
        if (!QueryPerformanceCounter(&now_counter) || !QueryPerformanceFrequency(&frequency) ||
            now_counter.QuadPart <= 0 || frequency.QuadPart <= 0) {
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Visual source leasing could not read the monotonic clock"};
            return false;
        }
        const auto now_qpc = static_cast<std::uint64_t>(now_counter.QuadPart);
        cleanup_visual_source_leases(now_qpc);
        if (capture_backend_ != CaptureBackend::windows_graphics_capture || !latest_frame_texture_ ||
            latest_frame_sequence_ == 0U || latest_frame_qpc_ == 0U || geometry_epoch_ == 0U ||
            !d3d_device_ || !d3d_context_) {
            failure = {FailureDomain::capture, FailureCode::unsupported_path, true,
                       "Visual source leasing requires a current Windows Graphics Capture frame"};
            return false;
        }
        if (latest_frame_qpc_ > now_qpc ||
            now_qpc - latest_frame_qpc_ >
                static_cast<std::uint64_t>(frequency.QuadPart) *
                    visual_capture_freshness_ms / 1000U) {
            failure = {FailureDomain::capture, FailureCode::timeout, true,
                       "Latest WGC frame is too old for a visual source lease"};
            return false;
        }
        DWORD target_process_id{};
        if (target_window_) GetWindowThreadProcessId(target_window_, &target_process_id);
        if (request.cancellation_generation == 0U || request.worker_process_id == 0U ||
            request.worker_process_id == GetCurrentProcessId() ||
            request.worker_process_id == target_process_id ||
            request.worker_process_creation_time == 0U || request.worker_executable_name.empty() ||
            request.worker_executable_name.find('/') != std::string::npos ||
            request.worker_executable_name.find('\\') != std::string::npos ||
            request.actor_id == 0U || request.track_id == 0U || request.track_epoch == 0U) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Visual source worker or typed actor binding is invalid"};
            return false;
        }
        constexpr std::size_t maximum_live_source_leases = 8U;
        if (exported_visual_sources_.size() >= maximum_live_source_leases) {
            failure = {FailureDomain::capture, FailureCode::timeout, true,
                       "Visual source lease pool is full until old one-frame leases expire"};
            return false;
        }
        UniqueHandle worker_process(OpenProcess(
            PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
            FALSE, request.worker_process_id));
        if (!worker_process || WaitForSingleObject(worker_process.get(), 0U) == WAIT_OBJECT_0 ||
            process_creation_time(worker_process.get()) != request.worker_process_creation_time ||
            !equal_ascii_case_insensitive(process_basename(request.worker_process_id),
                                           request.worker_executable_name)) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Visual source worker PID, creation time, or executable identity changed"};
            return false;
        }

        D3D11_TEXTURE2D_DESC source_description{};
        latest_frame_texture_->GetDesc(&source_description);
        if (source_description.Width == 0U || source_description.Height == 0U ||
            source_description.Format != DXGI_FORMAT_B8G8R8A8_UNORM ||
            source_description.ArraySize != 1U || source_description.MipLevels != 1U ||
            source_description.SampleDesc.Count != 1U) {
            failure = {FailureDomain::capture, FailureCode::invalid_geometry, false,
                       "Current WGC frame format cannot be leased to the mouth worker"};
            return false;
        }
        D3D11_TEXTURE2D_DESC shared_description = source_description;
        shared_description.Usage = D3D11_USAGE_DEFAULT;
        shared_description.CPUAccessFlags = 0U;
        shared_description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        shared_description.MiscFlags = D3D11_RESOURCE_MISC_SHARED_NTHANDLE |
                                       D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX;
        ComPtr<ID3D11Texture2D> shared_texture;
        HRESULT hr = d3d_device_->CreateTexture2D(&shared_description, nullptr, &shared_texture);
        ComPtr<IDXGIKeyedMutex> keyed_mutex;
        if (SUCCEEDED(hr)) hr = shared_texture.As(&keyed_mutex);
        if (FAILED(hr) || !shared_texture || !keyed_mutex || keyed_mutex->AcquireSync(0U, 0U) != S_OK) {
            failure = hresult_failure(FailureDomain::capture, FailureCode::backend_unavailable, hr,
                                      "Visual source shared texture allocation failed", true);
            return false;
        }
        d3d_context_->CopyResource(shared_texture.Get(), latest_frame_texture_.Get());
        hr = keyed_mutex->ReleaseSync(1U);
        if (FAILED(hr)) {
            failure = hresult_failure(FailureDomain::capture, FailureCode::backend_unavailable, hr,
                                      "Visual source keyed mutex publish failed", true);
            return false;
        }

        ComPtr<IDXGIResource1> shared_resource;
        HANDLE broker_shared_handle{};
        hr = shared_texture.As(&shared_resource);
        if (SUCCEEDED(hr)) {
            hr = shared_resource->CreateSharedHandle(
                nullptr, DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE,
                nullptr, &broker_shared_handle);
        }
        UniqueHandle local_shared_handle(broker_shared_handle);
        if (FAILED(hr) || !local_shared_handle) {
            failure = hresult_failure(FailureDomain::capture, FailureCode::backend_unavailable, hr,
                                      "Visual source NT handle creation failed", true);
            return false;
        }
        HANDLE worker_handle{};
        if (!DuplicateHandle(GetCurrentProcess(), local_shared_handle.get(), worker_process.get(),
                             &worker_handle, 0U, FALSE, DUPLICATE_SAME_ACCESS) || !worker_handle) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Broker could not duplicate the source texture into the exact worker"};
            return false;
        }

        ComPtr<IDXGIDevice> dxgi_device;
        ComPtr<IDXGIAdapter> adapter;
        DXGI_ADAPTER_DESC adapter_description{};
        hr = d3d_device_.As(&dxgi_device);
        if (SUCCEEDED(hr)) hr = dxgi_device->GetAdapter(&adapter);
        if (SUCCEEDED(hr)) hr = adapter->GetDesc(&adapter_description);
        std::uint64_t nonce_high{};
        std::uint64_t nonce_low{};
        if (FAILED(hr) || !random_lease_nonce(nonce_high, nonce_low)) {
            close_remote_visual_source(worker_process.get(),
                                       reinterpret_cast<std::uint64_t>(worker_handle));
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Visual source adapter identity or secure lease nonce is unavailable"};
            return false;
        }

        // The frame must be fresh when leased. The mouth worker independently
        // enforces its request deadline; the longer lease is only the bounded
        // end-to-end source/evidence/import/presentation authority window.
        const auto expiry = now_qpc + static_cast<std::uint64_t>(frequency.QuadPart) *
                                          visual_presentation_deadline_ms / 1000U;
        constexpr std::uint64_t acknowledgement_retention_ms = 2000U;
        const auto acknowledgement_expiry = now_qpc +
            static_cast<std::uint64_t>(frequency.QuadPart) * acknowledgement_retention_ms / 1000U;
        const auto broker_creation = process_creation_time(GetCurrentProcess());
        const auto broker_name = process_basename(GetCurrentProcessId());
        if (broker_creation == 0U || broker_name.empty()) {
            close_remote_visual_source(worker_process.get(),
                                       reinterpret_cast<std::uint64_t>(worker_handle));
            failure = {FailureDomain::capture, FailureCode::internal_error, false,
                       "Broker process identity could not be attested for the worker"};
            return false;
        }
        lease = {
            1U,
            GetCurrentProcessId(),
            broker_creation,
            broker_name,
            request.worker_process_id,
            request.worker_process_creation_time,
            request.worker_executable_name,
            reinterpret_cast<std::uint64_t>(worker_handle),
            nonce_high,
            nonce_low,
            pack_luid(adapter_description.AdapterLuid),
            1U,
            2U,
            shared_description.Width,
            shared_description.Height,
            shared_description.Width * 4U,
            static_cast<std::uint32_t>(shared_description.Format),
            1U,
            expiry,
            static_cast<std::uint64_t>(frequency.QuadPart),
            request.cancellation_generation,
            device_generation_,
            geometry_epoch_,
            latest_frame_sequence_,
            latest_frame_qpc_,
            request.actor_id,
            request.track_id,
            request.track_epoch,
        };
        // DuplicateHandle transferred ownership of this remote handle to the
        // worker. From this point onward the broker retains only the lease
        // identity. It must never close the numeric value remotely: the worker
        // closes it immediately after import and Windows may reuse that value.
        exported_visual_sources_.push_back({
            std::move(worker_process), request.worker_process_id, nonce_high, nonce_low,
            acknowledgement_expiry, request.cancellation_generation, device_generation_,
            geometry_epoch_, latest_frame_sequence_, latest_frame_qpc_, request.actor_id,
            request.track_id, request.track_epoch});
        return true;
    }

    bool release_visual_source(const std::uint32_t worker_process_id,
                               const std::uint64_t lease_nonce_high,
                               const std::uint64_t lease_nonce_low) noexcept override {
        const auto match = [=](const ExportedVisualSource& source) {
            return source.worker_process_id == worker_process_id &&
                   source.lease_nonce_high == lease_nonce_high &&
                   source.lease_nonce_low == lease_nonce_low;
        };
        const auto iterator = std::find_if(exported_visual_sources_.begin(),
                                           exported_visual_sources_.end(), match);
        if (iterator == exported_visual_sources_.end()) return false;
        exported_visual_sources_.erase(iterator);
        return true;
    }

    void cancel_visual_source_leases() noexcept override {
        // Remote source handles were transferred to the worker and may already
        // have been closed/reused. Cancellation revokes lease identity only.
        exported_visual_sources_.clear();
    }

    bool allocate_identity_frame(const IdentityFrameLeaseRequest& request,
                                 IdentityFrameLease& lease,
                                 Failure& failure) override {
        lease = {};
        LARGE_INTEGER counter{}, frequency{};
        if (!QueryPerformanceCounter(&counter) || !QueryPerformanceFrequency(&frequency) ||
            counter.QuadPart <= 0 || frequency.QuadPart <= 0) {
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity frame leasing could not read the monotonic clock"};
            return false;
        }
        const auto now_qpc = static_cast<std::uint64_t>(counter.QuadPart);
        cleanup_identity_frame_leases(now_qpc);
        if (!exported_identity_frames_.empty()) {
            failure = {FailureDomain::capture, FailureCode::timeout, true,
                       "Identity frame queue is full until the active request releases"};
            return false;
        }
        if (capture_backend_ != CaptureBackend::windows_graphics_capture || !latest_frame_texture_ ||
            !d3d_device_ || !d3d_context_ || !target_window_ || latest_frame_sequence_ == 0U ||
            latest_frame_qpc_ == 0U || device_generation_ == 0U || geometry_epoch_ == 0U ||
            !advancing_frame_verified_ || !residual_overlay_.capture_excluded() ||
            request.capture_session_id.empty() || request.capture_session_id.size() > 64U ||
            request.cancellation_generation == 0U) {
            failure = {FailureDomain::capture, FailureCode::unsupported_path, false,
                       "Identity inference requires advancing WGC, capture-excluded overlay, and a bound session"};
            return false;
        }
        constexpr std::uint64_t maximum_source_age_ms = 120U;
        if (latest_frame_qpc_ > now_qpc || now_qpc - latest_frame_qpc_ >
                static_cast<std::uint64_t>(frequency.QuadPart) * maximum_source_age_ms / 1000U) {
            failure = {FailureDomain::capture, FailureCode::timeout, true,
                       "Latest WGC frame is too old for identity inference"};
            return false;
        }
        DWORD target_process_id{};
        GetWindowThreadProcessId(target_window_, &target_process_id);
        if (request.worker_process_id == 0U || request.worker_process_id == GetCurrentProcessId() ||
            request.worker_process_id == target_process_id ||
            request.worker_process_creation_time == 0U || request.worker_executable_name.empty() ||
            request.worker_executable_name.find('/') != std::string::npos ||
            request.worker_executable_name.find('\\') != std::string::npos ||
            request.crop_px.left < 0 || request.crop_px.top < 0 || !request.crop_px.valid()) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity worker or crop binding is invalid"};
            return false;
        }
        UniqueHandle worker_process(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                                                FALSE, request.worker_process_id));
        if (!worker_process || WaitForSingleObject(worker_process.get(), 0U) == WAIT_OBJECT_0 ||
            process_creation_time(worker_process.get()) != request.worker_process_creation_time ||
            !equal_ascii_case_insensitive(process_basename(request.worker_process_id),
                                           request.worker_executable_name) ||
            !same_user_and_session(worker_process.get())) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity worker PID, creation, executable, user, or session changed"};
            return false;
        }
        D3D11_TEXTURE2D_DESC source{};
        latest_frame_texture_->GetDesc(&source);
        if (source.Format != DXGI_FORMAT_B8G8R8A8_UNORM || source.ArraySize != 1U ||
            source.MipLevels != 1U || source.SampleDesc.Count != 1U ||
            request.crop_px.right > static_cast<std::int32_t>(source.Width) ||
            request.crop_px.bottom > static_cast<std::int32_t>(source.Height)) {
            failure = {FailureDomain::capture, FailureCode::invalid_geometry, false,
                       "Identity crop does not fit the exact WGC BGRA8 frame"};
            return false;
        }
        const auto width = static_cast<std::uint32_t>(request.crop_px.width());
        const auto height = static_cast<std::uint32_t>(request.crop_px.height());
        const auto stride = width * 4U;
        const auto byte_length = static_cast<std::uint64_t>(stride) * height;
        if (width > 8192U || height > 8192U || byte_length == 0U ||
            byte_length > 64U * 1024U * 1024U) {
            failure = {FailureDomain::capture, FailureCode::invalid_geometry, false,
                       "Identity crop exceeds its 64 MiB and 8192-pixel bounds"};
            return false;
        }
        D3D11_TEXTURE2D_DESC staging_description{};
        staging_description.Width = width;
        staging_description.Height = height;
        staging_description.MipLevels = 1U;
        staging_description.ArraySize = 1U;
        staging_description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        staging_description.SampleDesc.Count = 1U;
        staging_description.Usage = D3D11_USAGE_STAGING;
        staging_description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        ComPtr<ID3D11Texture2D> staging;
        HRESULT hr = d3d_device_->CreateTexture2D(&staging_description, nullptr, &staging);
        if (FAILED(hr)) {
            failure = hresult_failure(FailureDomain::capture, FailureCode::backend_unavailable,
                                      hr, "Identity staging texture allocation failed", true);
            return false;
        }
        const D3D11_BOX source_box{static_cast<UINT>(request.crop_px.left),
                                   static_cast<UINT>(request.crop_px.top), 0U,
                                   static_cast<UINT>(request.crop_px.right),
                                   static_cast<UINT>(request.crop_px.bottom), 1U};
        d3d_context_->CopySubresourceRegion(staging.Get(), 0U, 0U, 0U, 0U,
                                            latest_frame_texture_.Get(), 0U, &source_box);
        D3D11_MAPPED_SUBRESOURCE mapped{};
        hr = d3d_context_->Map(staging.Get(), 0U, D3D11_MAP_READ, 0U, &mapped);
        if (FAILED(hr) || mapped.RowPitch < stride || !mapped.pData) {
            failure = hresult_failure(FailureDomain::capture, FailureCode::backend_unavailable,
                                      hr, "Identity staging texture readback failed", true);
            return false;
        }
        std::uint64_t id_high{}, id_low{}, nonce_high{}, nonce_low{};
        if (!random_lease_nonce(id_high, id_low) || !random_lease_nonce(nonce_high, nonce_low)) {
            d3d_context_->Unmap(staging.Get(), 0U);
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity lease identifiers could not be generated"};
            return false;
        }
        const auto lease_id = hex_nonce(id_high, id_low);
        const auto lease_nonce = hex_nonce(nonce_high, nonce_low);
        const std::string binding = lease_id + std::string(1, '\0') + lease_nonce;
        const auto mapping_name = "Local\\npc.identity." + sha256_hex({
            reinterpret_cast<const std::byte*>(binding.data()), binding.size()});
        const auto mapping_name_wide = wide_from_utf8(mapping_name);
        LocalSecurityDescriptor security;
        if (mapping_name.size() != 83U || mapping_name_wide.empty() ||
            !read_only_same_user_mapping_security(security)) {
            d3d_context_->Unmap(staging.Get(), 0U);
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity mapping name or same-user read-only ACL could not be created"};
            return false;
        }
        UniqueHandle mapping(CreateFileMappingW(INVALID_HANDLE_VALUE, &security.attributes,
                                                PAGE_READWRITE,
                                                static_cast<DWORD>(byte_length >> 32U),
                                                static_cast<DWORD>(byte_length & 0xffffffffU),
                                                mapping_name_wide.c_str()));
        if (!mapping || GetLastError() == ERROR_ALREADY_EXISTS) {
            d3d_context_->Unmap(staging.Get(), 0U);
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity mapping creation was denied or collided"};
            return false;
        }
        void* view = MapViewOfFile(mapping.get(), FILE_MAP_WRITE, 0U, 0U,
                                   static_cast<SIZE_T>(byte_length));
        if (!view) {
            d3d_context_->Unmap(staging.Get(), 0U);
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity mapping could not be initialized"};
            return false;
        }
        auto* destination = static_cast<std::byte*>(view);
        const auto* source_bytes = static_cast<const std::byte*>(mapped.pData);
        for (std::uint32_t row = 0; row < height; ++row) {
            std::memcpy(destination + static_cast<std::size_t>(row) * stride,
                        source_bytes + static_cast<std::size_t>(row) * mapped.RowPitch, stride);
        }
        d3d_context_->Unmap(staging.Get(), 0U);
        const auto digest = sha256_hex({destination, static_cast<std::size_t>(byte_length)});
        UnmapViewOfFile(view);
        if (digest.size() != 64U) {
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity crop digest could not be computed"};
            return false;
        }
        constexpr std::uint64_t lease_ms = 500U;
        const auto expiry = now_qpc + static_cast<std::uint64_t>(frequency.QuadPart) * lease_ms / 1000U;
        const auto selected_name = process_basename(target_process_id);
        const auto captured_ms = captured_unix_ms(latest_frame_qpc_, now_qpc,
                                                   static_cast<std::uint64_t>(frequency.QuadPart));
        if (selected_name.empty() || captured_ms == 0U) {
            failure = {FailureDomain::capture, FailureCode::internal_error, false,
                       "Identity frame target or wall-clock provenance is unavailable"};
            return false;
        }
        lease = {1U, request.worker_process_id, request.worker_process_creation_time,
                 request.worker_executable_name, lease_id, mapping_name, lease_nonce,
                 byte_length, width, height, stride, "b8g8r8a8_unorm", digest, expiry,
                 static_cast<std::uint64_t>(frequency.QuadPart), request.cancellation_generation,
                 request.capture_session_id, target_process_id,
                 reinterpret_cast<std::uint64_t>(target_window_), selected_name,
                 device_generation_, geometry_epoch_, latest_frame_sequence_, latest_frame_qpc_,
                 captured_ms, true, true, false, false, request.crop_px,
                 {static_cast<std::int32_t>(source.Width), static_cast<std::int32_t>(source.Height)}};
        exported_identity_frames_.push_back({std::move(mapping), std::move(worker_process),
                                             request.worker_process_id, lease_id, lease_nonce,
                                             expiry, false});
        return true;
    }

    bool release_identity_frame(const std::uint32_t worker_process_id,
                                const std::string_view lease_id,
                                const std::string_view lease_nonce) noexcept override {
        const auto iterator = std::find_if(exported_identity_frames_.begin(),
                                           exported_identity_frames_.end(),
            [&](const ExportedIdentityFrame& frame) {
                return !frame.reference_import &&
                       frame.worker_process_id == worker_process_id &&
                       frame.lease_id == lease_id && frame.lease_nonce == lease_nonce;
            });
        if (iterator == exported_identity_frames_.end()) return false;
        exported_identity_frames_.erase(iterator);
        return true;
    }

    void cancel_identity_frame_leases() noexcept override {
        // Closing the sole broker mapping handle destroys the raw crop as soon
        // as the worker closes its read-only view. No pixels are retained.
        exported_identity_frames_.clear();
    }

    bool allocate_identity_reference_import(const IdentityReferenceImportRequest& request,
                                            IdentityReferenceImportLease& lease,
                                            Failure& failure) override {
        lease = {};
        LARGE_INTEGER counter{}, frequency{};
        if (!QueryPerformanceCounter(&counter) || !QueryPerformanceFrequency(&frequency) ||
            counter.QuadPart <= 0 || frequency.QuadPart <= 0) {
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity reference import could not read the monotonic clock"};
            return false;
        }
        const auto now_qpc = static_cast<std::uint64_t>(counter.QuadPart);
        cleanup_identity_frame_leases(now_qpc);
        const auto valid_identifier = [](const std::string_view value, const std::size_t maximum) {
            return !value.empty() && value.size() <= maximum &&
                std::all_of(value.begin(), value.end(), [](const unsigned char byte) {
                    return std::isalnum(byte) != 0 || byte == '-' || byte == '_' || byte == '.';
                });
        };
        const bool private_rights = request.source_class == IdentityReferenceSourceClass::user_private &&
            request.explicit_user_consent && !request.owner_user_id.empty() &&
            request.owner_user_id.size() <= 128U && request.original_work_license.empty();
        const bool original_rights =
            request.source_class == IdentityReferenceSourceClass::original_synthetic &&
            request.owner_user_id.empty() && !request.original_work_license.empty() &&
            request.original_work_license.size() <= 512U;
        if (!exported_identity_frames_.empty() ||
            capture_backend_ != CaptureBackend::windows_graphics_capture || !target_window_ ||
            device_generation_ == 0U || geometry_epoch_ == 0U ||
            !residual_overlay_.capture_excluded() || request.capture_session_id.empty() ||
            request.capture_session_id.size() > 64U || request.cancellation_generation == 0U ||
            request.worker_process_id == 0U || request.worker_process_id == GetCurrentProcessId() ||
            request.worker_process_creation_time == 0U || request.worker_executable_name.empty() ||
            request.worker_executable_name.size() > 260U ||
            !valid_identifier(request.picker_consent_token, 128U) ||
            !valid_identifier(request.game_profile_id, 128U) ||
            !valid_identifier(request.character_id, 128U) ||
            !valid_identifier(request.subject_id, 128U) ||
            request.subject_id != request.character_id ||
            !valid_identifier(request.reference_id, 128U) ||
            request.subject_display_name.empty() || request.subject_display_name.size() > 256U ||
            !request.local_only || request.imported_at_unix_ms == 0U ||
            (!private_rights && !original_rights)) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity reference target, worker, consent, or rights binding is invalid"};
            return false;
        }
        DWORD target_process_id{};
        GetWindowThreadProcessId(target_window_, &target_process_id);
        if (request.worker_process_id == target_process_id) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity reference worker cannot be the selected target"};
            return false;
        }
        UniqueHandle worker_process(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                                                FALSE, request.worker_process_id));
        if (!worker_process || WaitForSingleObject(worker_process.get(), 0U) == WAIT_OBJECT_0 ||
            process_creation_time(worker_process.get()) != request.worker_process_creation_time ||
            !equal_ascii_case_insensitive(process_basename(request.worker_process_id),
                                           request.worker_executable_name) ||
            !same_user_and_session(worker_process.get())) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity reference worker attestation changed"};
            return false;
        }
        windows::DecodedIdentityReference decoded;
        if (!windows::pick_and_decode_identity_reference(decoded, failure)) return false;
        if (!IsWindow(target_window_) || WaitForSingleObject(worker_process.get(), 0U) == WAIT_OBJECT_0) {
            failure = {FailureDomain::capture, FailureCode::target_lost, false,
                       "Identity reference target or worker exited during selection"};
            return false;
        }
        const auto byte_length = static_cast<std::uint64_t>(decoded.stride_bytes) * decoded.height;
        if (decoded.width == 0U || decoded.height == 0U || decoded.width > 8192U ||
            decoded.height > 8192U || decoded.stride_bytes != decoded.width * 4U ||
            byte_length == 0U || byte_length > 64U * 1024U * 1024U ||
            decoded.bgra.size() != byte_length || decoded.source_asset_sha256.size() != 64U) {
            failure = {FailureDomain::capture, FailureCode::invalid_geometry, false,
                       "Identity reference normalized pixels are out of bounds"};
            return false;
        }
        std::uint64_t id_high{}, id_low{}, nonce_high{}, nonce_low{};
        if (!random_lease_nonce(id_high, id_low) || !random_lease_nonce(nonce_high, nonce_low)) {
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity reference lease identifiers could not be generated"};
            return false;
        }
        const auto lease_id = hex_nonce(id_high, id_low);
        const auto lease_nonce = hex_nonce(nonce_high, nonce_low);
        const std::string binding = lease_id + std::string(1, '\0') + lease_nonce;
        const auto mapping_name = "Local\\npc.identity." + sha256_hex({
            reinterpret_cast<const std::byte*>(binding.data()), binding.size()});
        const auto mapping_name_wide = wide_from_utf8(mapping_name);
        LocalSecurityDescriptor security;
        if (mapping_name.size() != 83U || mapping_name_wide.empty() ||
            !read_only_same_user_mapping_security(security)) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity reference mapping security could not be created"};
            return false;
        }
        UniqueHandle mapping(CreateFileMappingW(INVALID_HANDLE_VALUE, &security.attributes,
                                                PAGE_READWRITE,
                                                static_cast<DWORD>(byte_length >> 32U),
                                                static_cast<DWORD>(byte_length & 0xffffffffU),
                                                mapping_name_wide.c_str()));
        if (!mapping || GetLastError() == ERROR_ALREADY_EXISTS) {
            failure = {FailureDomain::capture, FailureCode::access_denied, false,
                       "Identity reference mapping creation was denied or collided"};
            return false;
        }
        void* view = MapViewOfFile(mapping.get(), FILE_MAP_WRITE, 0U, 0U,
                                   static_cast<SIZE_T>(byte_length));
        if (!view) {
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity reference mapping could not be initialized"};
            return false;
        }
        std::memcpy(view, decoded.bgra.data(), decoded.bgra.size());
        const auto content_digest = sha256_hex({static_cast<const std::byte*>(view),
                                                static_cast<std::size_t>(byte_length)});
        UnmapViewOfFile(view);
        std::fill(decoded.bgra.begin(), decoded.bgra.end(), std::byte{});
        decoded.bgra.clear();
        if (content_digest.size() != 64U) {
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity reference normalized pixel digest failed"};
            return false;
        }
        if (!QueryPerformanceCounter(&counter) || counter.QuadPart <= 0) {
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "Identity reference expiry clock could not be refreshed"};
            return false;
        }
        constexpr std::uint64_t lease_ms = 2000U;
        const auto expiry = static_cast<std::uint64_t>(counter.QuadPart) +
            static_cast<std::uint64_t>(frequency.QuadPart) * lease_ms / 1000U;
        const auto selected_name = process_basename(target_process_id);
        if (selected_name.empty()) {
            failure = {FailureDomain::capture, FailureCode::target_lost, false,
                       "Identity reference selected target identity is unavailable"};
            return false;
        }
        lease = {1U, request.worker_process_id, request.worker_process_creation_time,
                 request.worker_executable_name, lease_id, mapping_name, lease_nonce,
                 byte_length, decoded.width, decoded.height, decoded.stride_bytes,
                 "b8g8r8a8_unorm", content_digest, decoded.source_asset_sha256,
                 decoded.media_type, expiry, static_cast<std::uint64_t>(frequency.QuadPart),
                 request.cancellation_generation, request.capture_session_id, target_process_id,
                 reinterpret_cast<std::uint64_t>(target_window_), selected_name,
                 device_generation_, geometry_epoch_, request.picker_consent_token,
                 request.game_profile_id, request.character_id, request.subject_id,
                 request.reference_id, request.subject_display_name, request.source_class,
                 request.owner_user_id, request.original_work_license,
                 request.explicit_user_consent, request.local_only, request.imported_at_unix_ms};
        exported_identity_frames_.push_back({std::move(mapping), std::move(worker_process),
                                             request.worker_process_id, lease_id, lease_nonce,
                                             expiry, true});
        return true;
    }

    bool release_identity_reference_import(const std::uint32_t worker_process_id,
                                           const std::string_view lease_id,
                                           const std::string_view lease_nonce) noexcept override {
        const auto iterator = std::find_if(exported_identity_frames_.begin(),
                                           exported_identity_frames_.end(),
            [&](const ExportedIdentityFrame& frame) {
                return frame.reference_import && frame.worker_process_id == worker_process_id &&
                       frame.lease_id == lease_id && frame.lease_nonce == lease_nonce;
            });
        if (iterator == exported_identity_frames_.end()) return false;
        exported_identity_frames_.erase(iterator);
        return true;
    }

    bool begin_manual_actor_picker(const ManualActorPickerRequest& request,
                                   ManualActorPickerReceipt& receipt,
                                   Failure& failure) override {
        // A new picker supersedes every older authorization. Clear the old
        // authority binding before copying or displaying another frozen frame.
        cancel_manual_actor_picker();
        receipt = {};
        LARGE_INTEGER now_counter{}, frequency{};
        if (!QueryPerformanceCounter(&now_counter) || !QueryPerformanceFrequency(&frequency) ||
            now_counter.QuadPart <= 0 || frequency.QuadPart <= 0) {
            failure = {FailureDomain::overlay, FailureCode::internal_error, true,
                       "Manual actor picker could not read the monotonic clock"};
            return false;
        }
        const auto now_qpc = static_cast<std::uint64_t>(now_counter.QuadPart);
        constexpr std::uint64_t maximum_source_age_ms = 80U;
        DWORD target_process_id{};
        if (target_window_) GetWindowThreadProcessId(target_window_, &target_process_id);
        const auto selected_name = process_basename(target_process_id);
        const bool exact_binding =
            request.selected_process_id == target_process_id &&
            request.selected_window_handle == reinterpret_cast<std::uint64_t>(target_window_) &&
            request.source_device_generation == device_generation_ &&
            request.source_geometry_epoch == geometry_epoch_ &&
            request.source_frame_sequence == latest_frame_sequence_ &&
            request.source_frame_qpc == latest_frame_qpc_;
        if (capture_backend_ != CaptureBackend::windows_graphics_capture ||
            !target_window_ || !IsWindow(target_window_) || IsIconic(target_window_) ||
            !latest_frame_texture_ || !d3d_device_ || !d3d_context_ ||
            !last_target_geometry_ || !advancing_frame_verified_ ||
            !residual_overlay_.capture_excluded() || request.request_id.empty() ||
            request.request_id.size() > 128U || request.capture_session_id.empty() ||
            request.capture_session_id.size() > 64U || request.cancellation_generation == 0U ||
            request.timeout_ms < 500U || request.timeout_ms > 15'000U ||
            request.candidates.empty() || request.candidates.size() > 64U ||
            selected_name.empty() || !exact_binding) {
            failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                       "Manual actor picker requires an exact current advancing WGC frame binding"};
            return false;
        }
        if (latest_frame_qpc_ > now_qpc ||
            now_qpc - latest_frame_qpc_ >
                static_cast<std::uint64_t>(frequency.QuadPart) * maximum_source_age_ms / 1000U) {
            failure = {FailureDomain::overlay, FailureCode::timeout, true,
                       "Manual actor picker source frame is no longer current"};
            return false;
        }
        const auto overlay = calculate_overlay_geometry(*last_target_geometry_);
        if (!overlay) {
            failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                       "Manual actor picker target geometry cannot be projected"};
            return false;
        }
        for (std::size_t index = 0; index < request.candidates.size(); ++index) {
            const auto& candidate = request.candidates[index];
            if (candidate.actor_id == 0U || candidate.track_id == 0U ||
                candidate.track_epoch == 0U ||
                !normalized_rect_valid(candidate.normalized_bounds) ||
                candidate.normalized_bounds.width() < 0.005 ||
                candidate.normalized_bounds.height() < 0.005 ||
                !map_normalized_source_rect(candidate.normalized_bounds, *overlay)) {
                failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                           "Manual actor picker received an invalid detected ROI"};
                return false;
            }
            for (std::size_t prior = 0; prior < index; ++prior) {
                if (candidate.actor_id == request.candidates[prior].actor_id ||
                    (candidate.track_id == request.candidates[prior].track_id &&
                     candidate.track_epoch == request.candidates[prior].track_epoch)) {
                    failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                               "Manual actor picker candidate identities are not unique"};
                    return false;
                }
            }
        }
        D3D11_TEXTURE2D_DESC source{};
        latest_frame_texture_->GetDesc(&source);
        const auto byte_length = static_cast<std::uint64_t>(source.Width) * source.Height * 4U;
        if (source.Format != DXGI_FORMAT_B8G8R8A8_UNORM || source.Width == 0U ||
            source.Height == 0U || source.Width > 8192U || source.Height > 8192U ||
            source.SampleDesc.Count != 1U || byte_length == 0U ||
            byte_length > 256U * 1024U * 1024U) {
            failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                       "Manual actor picker supports only bounded single-sample BGRA8 WGC frames"};
            return false;
        }
        auto staging_description = source;
        staging_description.Usage = D3D11_USAGE_STAGING;
        staging_description.BindFlags = 0U;
        staging_description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        staging_description.MiscFlags = 0U;
        ComPtr<ID3D11Texture2D> staging;
        HRESULT hr = d3d_device_->CreateTexture2D(&staging_description, nullptr, &staging);
        if (FAILED(hr)) {
            failure = hresult_failure(FailureDomain::overlay, FailureCode::backend_unavailable,
                                      hr, "Create manual actor frozen-frame staging texture failed");
            return false;
        }
        d3d_context_->CopyResource(staging.Get(), latest_frame_texture_.Get());
        D3D11_MAPPED_SUBRESOURCE mapped{};
        hr = d3d_context_->Map(staging.Get(), 0U, D3D11_MAP_READ, 0U, &mapped);
        if (FAILED(hr)) {
            failure = hresult_failure(FailureDomain::overlay, FailureCode::backend_unavailable,
                                      hr, "Map manual actor frozen WGC frame failed");
            return false;
        }
        if (mapped.RowPitch < source.Width * 4U) {
            d3d_context_->Unmap(staging.Get(), 0U);
            failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                       "Manual actor frozen WGC frame row pitch is invalid"};
            return false;
        }
        std::vector<std::byte> frozen(static_cast<std::size_t>(byte_length));
        const auto tight_stride = static_cast<std::size_t>(source.Width) * 4U;
        for (UINT row = 0; row < source.Height; ++row) {
            std::memcpy(frozen.data() + static_cast<std::size_t>(row) * tight_stride,
                        static_cast<const std::byte*>(mapped.pData) +
                            static_cast<std::size_t>(row) * mapped.RowPitch,
                        tight_stride);
        }
        d3d_context_->Unmap(staging.Get(), 0U);
        const auto candidate_digest = manual_actor_candidate_digest(request.candidates);
        std::uint64_t receipt_nonce_high{}, receipt_nonce_low{};
        if (candidate_digest.size() != 64U ||
            !random_lease_nonce(receipt_nonce_high, receipt_nonce_low)) {
            std::fill(frozen.begin(), frozen.end(), std::byte{});
            failure = {FailureDomain::overlay, FailureCode::internal_error, true,
                       "Manual actor picker receipt provenance could not be sealed"};
            return false;
        }
        const ManualActorPickerReceipt pending{
            1U, request.request_id, ManualActorPickerStatus::pending,
            receipt_nonce_high, receipt_nonce_low, request.capture_session_id,
            request.cancellation_generation, target_process_id,
            reinterpret_cast<std::uint64_t>(target_window_), selected_name,
            device_generation_, geometry_epoch_, latest_frame_sequence_, latest_frame_qpc_,
            0U, 0U, 0U, static_cast<std::uint32_t>(request.candidates.size()),
            candidate_digest, now_qpc, 0U, now_qpc,
            static_cast<std::uint64_t>(frequency.QuadPart), ManualActorPointerKind::none,
            true, true, true, false, true, true};
        const auto expected_window = target_window_;
        const auto expected_process = target_process_id;
        const auto expected_geometry = *last_target_geometry_;
        const auto expected_device_generation = device_generation_;
        const auto expected_geometry_epoch = geometry_epoch_;
        const auto expected_capture_session = request.capture_session_id;
        const auto expected_cancellation_generation = request.cancellation_generation;
        windows::ManualActorPickerStartContext context{
            target_window_, *last_target_geometry_, *overlay,
            {static_cast<std::int32_t>(source.Width), static_cast<std::int32_t>(source.Height)},
            source.Width * 4U, std::move(frozen), request.candidates, pending,
            request.timeout_ms,
            [this, expected_window, expected_process, expected_geometry,
             expected_device_generation, expected_geometry_epoch,
             expected_capture_session, expected_cancellation_generation] {
                {
                    std::scoped_lock authority_lock(picker_authority_mutex_);
                    if (picker_capture_session_ != expected_capture_session ||
                        picker_cancellation_generation_ != expected_cancellation_generation) {
                        return windows::ManualActorPickerGuardState::cancelled;
                    }
                }
                if (picker_target_window_.load(std::memory_order_acquire) !=
                        reinterpret_cast<std::uintptr_t>(expected_window) ||
                    !IsWindow(expected_window) || IsIconic(expected_window)) {
                    return windows::ManualActorPickerGuardState::target_lost;
                }
                DWORD process{};
                if (GetWindowThreadProcessId(expected_window, &process) == 0U ||
                    process != expected_process) return windows::ManualActorPickerGuardState::target_lost;
                if (picker_capture_backend_.load(std::memory_order_acquire) !=
                    static_cast<std::uint32_t>(CaptureBackend::windows_graphics_capture)) {
                    return windows::ManualActorPickerGuardState::capture_changed;
                }
                if (picker_device_generation_.load(std::memory_order_acquire) !=
                    expected_device_generation) return windows::ManualActorPickerGuardState::device_changed;
                const auto current = query_target_geometry(expected_window);
                if (current.monitor.dpi_x != expected_geometry.monitor.dpi_x ||
                    current.monitor.dpi_y != expected_geometry.monitor.dpi_y) {
                    return windows::ManualActorPickerGuardState::dpi_changed;
                }
                if (current.window_bounds_px != expected_geometry.window_bounds_px ||
                    current.client_bounds_px != expected_geometry.client_bounds_px ||
                    current.monitor.stable_id != expected_geometry.monitor.stable_id) {
                    return windows::ManualActorPickerGuardState::target_resized;
                }
                if (picker_geometry_epoch_.load(std::memory_order_acquire) !=
                    expected_geometry_epoch) return windows::ManualActorPickerGuardState::capture_changed;
                return windows::ManualActorPickerGuardState::current;
            }};
        {
            std::scoped_lock authority_lock(picker_authority_mutex_);
            picker_capture_session_ = expected_capture_session;
            picker_cancellation_generation_ = expected_cancellation_generation;
        }
        if (!manual_actor_picker_.begin(std::move(context), receipt, failure)) {
            clear_manual_actor_picker_authority(expected_capture_session,
                                                expected_cancellation_generation);
            return false;
        }
        return true;
    }

    bool query_manual_actor_picker(const std::string_view request_id,
                                   ManualActorPickerReceipt& receipt,
                                   Failure& failure) override {
        return manual_actor_picker_.query(request_id, receipt, failure);
    }

    bool cancel_manual_actor_picker(const std::string_view request_id,
                                    ManualActorPickerReceipt& receipt,
                                    Failure& failure) override {
        if (!manual_actor_picker_.cancel(request_id, receipt, failure)) return false;
        clear_manual_actor_picker_authority(receipt.capture_session_id,
                                            receipt.cancellation_generation);
        return true;
    }

    void cancel_manual_actor_picker() noexcept override {
        // The overlay's receipt mutex decides whether cancellation or an
        // already-running guarded click commit wins. Clear the authority only
        // after that serialized terminal transition; clearing first would let
        // authority change between the final guard and receipt commit.
        manual_actor_picker_.cancel();
        {
            std::scoped_lock authority_lock(picker_authority_mutex_);
            picker_capture_session_.clear();
            picker_cancellation_generation_ = 0U;
        }
    }

    bool import_shared_residual(const SharedResidualLease& lease,
                                std::uintptr_t& native_texture,
                                Failure& failure) override {
        native_texture = 0;
        release_shared_residual();
        if (capture_backend_ != CaptureBackend::windows_graphics_capture ||
            !residual_overlay_.active() || !residual_overlay_.capture_excluded() ||
            !d3d_device_ || !overlay_geometry_) {
            failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                       "Residual import requires active WGC and a capture-excluded overlay"};
            return false;
        }
        if (lease.schema_version != 1U || lease.worker_process_id == 0U ||
            lease.worker_process_id == GetCurrentProcessId() ||
            lease.worker_process_creation_time == 0U || lease.worker_executable_name.empty() ||
            lease.source_handle_value == 0U || lease.lease_nonce_high == 0U ||
            lease.lease_nonce_low == 0U || lease.source_device_generation != device_generation_ ||
            lease.source_geometry_epoch != geometry_epoch_ ||
            lease.source_frame_sequence == 0U ||
            lease.source_frame_sequence > latest_frame_sequence_ ||
            lease.source_frame_qpc == 0U || lease.source_frame_qpc > latest_frame_qpc_) {
            failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                       "Residual lease identity does not match the active capture generation"};
            return false;
        }
        LARGE_INTEGER qpc{};
        LARGE_INTEGER frequency{};
        QueryPerformanceCounter(&qpc);
        QueryPerformanceFrequency(&frequency);
        if (qpc.QuadPart <= 0 || frequency.QuadPart <= 0 ||
            lease.expires_qpc < static_cast<std::uint64_t>(qpc.QuadPart)) {
            failure = {FailureDomain::overlay, FailureCode::timeout, false,
                       "Residual lease expired before import"};
            return false;
        }
        const auto now_qpc = static_cast<std::uint64_t>(qpc.QuadPart);
        if (lease.source_frame_qpc > now_qpc ||
            now_qpc - lease.source_frame_qpc >
                static_cast<std::uint64_t>(frequency.QuadPart) *
                    visual_presentation_deadline_ms / 1000U) {
            failure = {FailureDomain::overlay, FailureCode::timeout, false,
                       "Residual source frame is outside the bounded CPU inference window"};
            return false;
        }
        cleanup_visual_source_leases(now_qpc);
        const auto source_authority = std::find_if(
            exported_visual_sources_.begin(), exported_visual_sources_.end(),
            [&](const ExportedVisualSource& source) {
                return source.worker_process_id == lease.worker_process_id &&
                    source.cancellation_generation == lease.cancellation_generation &&
                    source.source_device_generation == lease.source_device_generation &&
                    source.source_geometry_epoch == lease.source_geometry_epoch &&
                    source.source_frame_sequence == lease.source_frame_sequence &&
                    source.source_frame_qpc == lease.source_frame_qpc &&
                    source.actor_id == lease.actor_id && source.track_id == lease.track_id &&
                    source.track_epoch == lease.track_epoch;
            });
        if (source_authority == exported_visual_sources_.end()) {
            failure = {FailureDomain::overlay, FailureCode::access_denied, false,
                       "Residual does not match an active broker-issued source lease"};
            return false;
        }
        std::erase_if(seen_lease_nonces_, [now_qpc](const SeenLeaseNonce& seen) {
            return seen.expires_qpc < now_qpc;
        });
        if (std::ranges::any_of(seen_lease_nonces_, [&](const SeenLeaseNonce& seen) {
                return seen.high == lease.lease_nonce_high && seen.low == lease.lease_nonce_low;
            })) {
            failure = {FailureDomain::overlay, FailureCode::access_denied, false,
                       "Residual lease nonce was already consumed"};
            return false;
        }
        constexpr std::size_t maximum_live_lease_nonces = 256;
        if (seen_lease_nonces_.size() >= maximum_live_lease_nonces) {
            failure = {FailureDomain::overlay, FailureCode::timeout, false,
                       "Residual lease replay cache is saturated until live leases expire"};
            return false;
        }
        const auto mapped_geometry = calculate_overlay_geometry(*overlay_geometry_);
        const auto mapped_bounds = mapped_geometry
            ? map_normalized_source_rect(lease.normalized_bounds, *mapped_geometry)
            : std::nullopt;
        if (!mapped_bounds || lease.width != static_cast<std::uint32_t>(mapped_bounds->width()) ||
            lease.height != static_cast<std::uint32_t>(mapped_bounds->height()) ||
            lease.stride_bytes != lease.width * 4U || lease.dxgi_format != DXGI_FORMAT_B8G8R8A8_UNORM ||
            lease.alpha_mode != 1U) {
            failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                       "Residual extent, bounds, format, or alpha mode is invalid"};
            return false;
        }

        UniqueHandle worker_process(OpenProcess(
            PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
            FALSE, lease.worker_process_id));
        if (!worker_process) {
            failure = {FailureDomain::overlay, FailureCode::access_denied, false,
                       "Broker could not open the exact residual worker process"};
            return false;
        }
        if (WaitForSingleObject(worker_process.get(), 0) == WAIT_OBJECT_0) {
            failure = {FailureDomain::overlay, FailureCode::target_lost, false,
                       "Residual worker exited before handle duplication"};
            return false;
        }
        FILETIME created{}, exited{}, kernel{}, user{};
        if (!GetProcessTimes(worker_process.get(), &created, &exited, &kernel, &user) ||
            pack_file_time(created) != lease.worker_process_creation_time ||
            !equal_ascii_case_insensitive(process_basename(lease.worker_process_id),
                                           lease.worker_executable_name)) {
            failure = {FailureDomain::overlay, FailureCode::access_denied, false,
                       "Residual worker PID, creation time, or executable identity changed"};
            return false;
        }

        ComPtr<IDXGIDevice> dxgi_device;
        ComPtr<IDXGIAdapter> adapter;
        DXGI_ADAPTER_DESC adapter_description{};
        HRESULT hr = d3d_device_.As(&dxgi_device);
        if (SUCCEEDED(hr)) hr = dxgi_device->GetAdapter(&adapter);
        if (SUCCEEDED(hr)) hr = adapter->GetDesc(&adapter_description);
        if (FAILED(hr) || pack_luid(adapter_description.AdapterLuid) != lease.adapter_luid) {
            failure = hresult_failure(FailureDomain::overlay, FailureCode::backend_unavailable, hr,
                                      "Residual adapter LUID does not match the broker device", false);
            return false;
        }

        HANDLE duplicated_raw{};
        if (!DuplicateHandle(worker_process.get(),
                             reinterpret_cast<HANDLE>(static_cast<std::uintptr_t>(lease.source_handle_value)),
                             GetCurrentProcess(), &duplicated_raw, 0, FALSE, DUPLICATE_SAME_ACCESS)) {
            failure = {FailureDomain::overlay, FailureCode::access_denied, false,
                       "Source handle is not owned by the declared residual worker"};
            return false;
        }
        UniqueHandle duplicated(duplicated_raw);
        ComPtr<ID3D11Device1> device1;
        ComPtr<ID3D11Texture2D> texture;
        hr = d3d_device_.As(&device1);
        if (SUCCEEDED(hr)) {
            hr = device1->OpenSharedResource1(duplicated.get(), IID_PPV_ARGS(&texture));
        }
        if (FAILED(hr) || !texture) {
            failure = hresult_failure(FailureDomain::overlay, FailureCode::access_denied, hr,
                                      "Duplicated worker handle is not a D3D11 shared texture", false);
            return false;
        }
        D3D11_TEXTURE2D_DESC description{};
        texture->GetDesc(&description);
        constexpr UINT required_misc = D3D11_RESOURCE_MISC_SHARED_NTHANDLE |
                                       D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX;
        if (description.Width != lease.width || description.Height != lease.height ||
            description.Format != DXGI_FORMAT_B8G8R8A8_UNORM || description.MipLevels != 1U ||
            description.ArraySize != 1U || description.SampleDesc.Count != 1U ||
            (description.MiscFlags & required_misc) != required_misc) {
            failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                       "Opened residual texture does not match its declared immutable contract"};
            return false;
        }
        ComPtr<IDXGIKeyedMutex> keyed_mutex;
        hr = texture.As(&keyed_mutex);
        if (FAILED(hr) || !keyed_mutex) {
            failure = hresult_failure(FailureDomain::overlay, FailureCode::access_denied, hr,
                                      "Residual texture omitted its required keyed mutex", false);
            return false;
        }

        auto imported = std::make_unique<ImportedResidual>();
        imported->lease = lease;
        imported->worker_process = std::move(worker_process);
        imported->texture = std::move(texture);
        imported->keyed_mutex = std::move(keyed_mutex);
        native_texture = reinterpret_cast<std::uintptr_t>(imported->texture.Get());
        imported_residual_ = std::move(imported);
        seen_lease_nonces_.push_back(
            {lease.lease_nonce_high, lease.lease_nonce_low, lease.expires_qpc});
        return true;
    }

    void release_shared_residual() noexcept override {
        imported_residual_.reset();
    }

    bool shared_residual_active() const noexcept override {
        return imported_residual_ != nullptr;
    }

    bool present_shared_residual(const MouthPatch& patch, Failure& failure) override {
        if (!imported_residual_ || !overlay_geometry_) {
            failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                       "No validated shared residual lease is ready for presentation"};
            return false;
        }
        const auto mapped = calculate_overlay_geometry(*overlay_geometry_);
        const auto patch_bounds = mapped
                                      ? map_normalized_source_rect(patch.normalized_bounds, *mapped)
                                      : std::nullopt;
        if (!mapped || !patch_bounds) {
            failure = {FailureDomain::overlay, FailureCode::invalid_geometry, false,
                       "Validated residual bounds no longer map to the active overlay"};
            release_shared_residual();
            return false;
        }
        const auto lease = imported_residual_->lease;
        const FrameDescriptor current_frame{
            lease.source_frame_sequence,
            lease.source_device_generation,
            patch.source_frame_captured_at,
            overlay_geometry_->captured_content_px,
            ColorSpace::sdr_srgb,
            reinterpret_cast<std::uintptr_t>(latest_frame_texture_.Get()),
            false,
            false,
            lease.source_geometry_epoch,
            lease.source_frame_qpc,
            0,
        };
        pending_failure_.reset();
        present_patch(current_frame, patch, *patch_bounds, *mapped);
        if (pending_failure_) {
            failure = *pending_failure_;
            pending_failure_.reset();
            return false;
        }
        return !shared_residual_active();
    }

    bool initialize_audio(Failure& failure) override {
        return audio_.start(failure);
    }

    void shutdown_audio() noexcept override { audio_.stop(); }

    bool register_ptt_hotkey(const std::uint32_t virtual_key, Failure& failure) override {
        if (virtual_key == 0 || virtual_key > 0xFF) {
            failure = {FailureDomain::hotkey, FailureCode::hotkey_conflict, false,
                       "PTT virtual key must be in the Win32 virtual-key range"};
            return false;
        }
        // Polling GetAsyncKeyState provides global press/release semantics without
        // installing a keyboard hook and without consuming the game's keystroke.
        ptt_virtual_key_ = static_cast<int>(virtual_key);
        ptt_pressed_ = false;
        return true;
    }

    void unregister_ptt_hotkey() noexcept override {
        ptt_virtual_key_ = 0;
        ptt_pressed_ = false;
    }

    bool recreate_graphics_device(const std::uint64_t generation, Failure& failure) override {
        cancel_manual_actor_picker();
        release_shared_residual();
        stop_overlay();
        stop_capture();
        d3d_context_.Reset();
        d3d_device_.Reset();
        device_generation_ = generation;
        publish_picker_binding_snapshot();
        return ensure_d3d_device(failure);
    }

    bool recreate_audio_clients(Failure& failure) override {
        return audio_.restart(failure);
    }

    SharedPcmRing* capture_pcm_ring() noexcept override { return audio_.capture_ring(); }
    SharedPcmRing* render_pcm_ring() noexcept override { return audio_.render_ring(); }

    void suppress_residual() noexcept override {
        residual_overlay_.hide();
        release_shared_residual();
    }

    void present_pristine(const FrameDescriptor&, const OverlayGeometry&) override {
        suppress_residual();
    }

    void present_patch(const FrameDescriptor& frame,
                       const MouthPatch& patch,
                       const RectI patch_bounds,
                       const OverlayGeometry&) override {
        if (!imported_residual_ || capture_backend_ != CaptureBackend::windows_graphics_capture ||
            patch.native_texture != reinterpret_cast<std::uintptr_t>(imported_residual_->texture.Get()) ||
            patch.cancellation_generation != imported_residual_->lease.cancellation_generation ||
            patch.source_device_generation != imported_residual_->lease.source_device_generation ||
            patch.source_geometry_epoch != imported_residual_->lease.source_geometry_epoch ||
            patch.source_frame_sequence != imported_residual_->lease.source_frame_sequence ||
            patch.source_frame_qpc != imported_residual_->lease.source_frame_qpc ||
            frame.device_generation != imported_residual_->lease.source_device_generation ||
            frame.geometry_epoch != imported_residual_->lease.source_geometry_epoch ||
            frame.sequence != imported_residual_->lease.source_frame_sequence ||
            frame.captured_qpc != imported_residual_->lease.source_frame_qpc ||
            patch.actor_id != imported_residual_->lease.actor_id ||
            patch.selected_track_id != imported_residual_->lease.track_id ||
            patch.track_epoch != imported_residual_->lease.track_epoch ||
            WaitForSingleObject(imported_residual_->worker_process.get(), 0) == WAIT_OBJECT_0) {
            suppress_residual();
            return;
        }
        LARGE_INTEGER qpc{};
        QueryPerformanceCounter(&qpc);
        if (qpc.QuadPart <= 0 || imported_residual_->lease.expires_qpc <
                                     static_cast<std::uint64_t>(qpc.QuadPart)) {
            suppress_residual();
            return;
        }
        constexpr DWORD keyed_mutex_timeout_ms = 2U;
        const HRESULT acquired = imported_residual_->keyed_mutex->AcquireSync(
            imported_residual_->lease.keyed_mutex_acquire_key, keyed_mutex_timeout_ms);
        if (acquired != S_OK) {
            suppress_residual();
            return;
        }
        Failure failure;
        const bool presented = residual_overlay_.present(imported_residual_->texture.Get(),
                                                         patch_bounds, failure);
        d3d_context_->Flush();
        const HRESULT released = imported_residual_->keyed_mutex->ReleaseSync(
            imported_residual_->lease.keyed_mutex_release_key);
        release_shared_residual();
        if (!presented || FAILED(released)) {
            if (FAILED(released)) {
                failure = hresult_failure(FailureDomain::overlay, FailureCode::device_removed,
                                          released, "Release residual keyed mutex failed");
            }
            pending_failure_ = std::move(failure);
        }
    }

    void poll() override {
        LARGE_INTEGER visual_source_qpc{};
        if (QueryPerformanceCounter(&visual_source_qpc) && visual_source_qpc.QuadPart > 0) {
            cleanup_visual_source_leases(
                static_cast<std::uint64_t>(visual_source_qpc.QuadPart));
        }
        if (imported_residual_) {
            LARGE_INTEGER qpc{};
            QueryPerformanceCounter(&qpc);
            if (WaitForSingleObject(imported_residual_->worker_process.get(), 0) == WAIT_OBJECT_0 ||
                qpc.QuadPart <= 0 || imported_residual_->lease.expires_qpc <
                                         static_cast<std::uint64_t>(qpc.QuadPart) ||
                imported_residual_->lease.source_device_generation != device_generation_ ||
                imported_residual_->lease.source_geometry_epoch != geometry_epoch_ ||
                imported_residual_->lease.source_frame_sequence > latest_frame_sequence_ ||
                imported_residual_->lease.source_frame_qpc > latest_frame_qpc_ ||
                capture_backend_ != CaptureBackend::windows_graphics_capture) {
                suppress_residual();
            }
        }
        if (auto failure = graphics_capture_.take_failure()) {
            cancel_manual_actor_picker();
            if (callbacks_.on_failure) {
                callbacks_.on_failure(std::move(*failure));
            }
            return;
        }
        if (auto failure = audio_.take_failure()) {
            if (callbacks_.on_failure) {
                callbacks_.on_failure(std::move(*failure));
            }
            return;
        }
        if (pending_failure_) {
            cancel_manual_actor_picker();
            auto failure = std::move(*pending_failure_);
            pending_failure_.reset();
            if (callbacks_.on_failure) {
                callbacks_.on_failure(std::move(failure));
            }
            return;
        }
        if (graphics_capture_.take_closed()) {
            cancel_manual_actor_picker();
            if (callbacks_.on_target_state) {
                callbacks_.on_target_state(TargetState::closed, std::nullopt);
            }
            return;
        }
        if (ptt_virtual_key_ != 0) {
            const bool pressed = (GetAsyncKeyState(ptt_virtual_key_) & 0x8000) != 0;
            if (pressed != ptt_pressed_) {
                ptt_pressed_ = pressed;
                if (callbacks_.on_ptt) {
                    callbacks_.on_ptt(pressed ? PttState::pressed : PttState::released,
                                      std::chrono::steady_clock::now());
                }
            }
        }

        if (!target_window_) {
            return;
        }
        if (!IsWindow(target_window_)) {
            cancel_manual_actor_picker();
            if (callbacks_.on_target_state) {
                callbacks_.on_target_state(TargetState::closed, std::nullopt);
            }
            target_window_ = nullptr;
            publish_picker_binding_snapshot();
            return;
        }
        if (IsIconic(target_window_)) {
            cancel_manual_actor_picker();
            if (capture_backend_ == CaptureBackend::windows_graphics_capture &&
                !wgc_suspended_for_minimize_) {
                // WGC may remain permanently silent after an HWND is restored
                // from minimization without reporting Closed or a D3D failure.
                // Tear down only the exact-window capture session; keep the
                // selected HWND and graphics generation so restore can rebind.
                cancel_visual_source_leases();
                cancel_identity_frame_leases();
                release_shared_residual();
                graphics_capture_.stop();
                latest_frame_texture_.Reset();
                latest_frame_sequence_ = 0;
                latest_frame_qpc_ = 0;
                advancing_frame_verified_ = false;
                wgc_suspended_for_minimize_ = true;
            }
            auto geometry = query_target_geometry(target_window_);
            publish_geometry(std::move(geometry), TargetState::minimized);
            return;
        }

        if (capture_backend_ == CaptureBackend::windows_graphics_capture &&
            wgc_suspended_for_minimize_) {
            Failure failure;
            if (!graphics_capture_.start(target_window_, d3d_device_.Get(), failure)) {
                if (callbacks_.on_failure) callbacks_.on_failure(std::move(failure));
                return;
            }
            wgc_suspended_for_minimize_ = false;
        }

        auto current_geometry = query_target_geometry(target_window_);
        if (capture_backend_ == CaptureBackend::windows_graphics_capture) {
            current_geometry.captured_desktop_bounds_px = current_geometry.window_bounds_px;
            if (last_target_geometry_) {
                current_geometry.captured_content_px = last_target_geometry_->captured_content_px;
            } else {
                current_geometry.captured_content_px = {current_geometry.window_bounds_px.width(),
                                                        current_geometry.window_bounds_px.height()};
            }
        }
        if (capture_backend_ == CaptureBackend::desktop_duplication) {
            current_geometry.captured_desktop_bounds_px = duplication_bounds_;
            current_geometry.captured_content_px = {duplication_bounds_.width(), duplication_bounds_.height()};
            if (!duplication_monitor_id_.empty() &&
                current_geometry.monitor.stable_id != duplication_monitor_id_) {
                publish_geometry(std::move(current_geometry));
                if (callbacks_.on_failure) {
                    callbacks_.on_failure({FailureDomain::device, FailureCode::device_reset, true,
                                           "Target moved to a different Desktop Duplication output"});
                }
                return;
            }
        }
        publish_geometry(std::move(current_geometry));

        if (capture_backend_ == CaptureBackend::windows_graphics_capture) {
            if (auto frame = graphics_capture_.take_latest()) {
                latest_frame_texture_ = std::move(frame->texture);
                auto geometry = query_target_geometry(target_window_);
                geometry.captured_content_px = frame->size_px;
                geometry.captured_desktop_bounds_px = geometry.window_bounds_px;
                publish_geometry(std::move(geometry));
                if (callbacks_.on_frame) {
                    const auto sequence = ++frame_sequence_;
                    if (latest_frame_sequence_ > 0U && latest_frame_qpc_ > 0U &&
                        sequence > latest_frame_sequence_ &&
                        frame->captured_qpc > latest_frame_qpc_) {
                        advancing_frame_verified_ = true;
                    }
                    latest_frame_sequence_ = sequence;
                    latest_frame_qpc_ = frame->captured_qpc;
                    callbacks_.on_frame({sequence,
                                         device_generation_,
                                         frame->captured_at,
                                         frame->size_px,
                                         ColorSpace::sdr_srgb,
                                         reinterpret_cast<std::uintptr_t>(latest_frame_texture_.Get()),
                                         false,
                                         false,
                                         geometry_epoch_,
                                         frame->captured_qpc,
                                         frame->content_hash});
                }
            }
            return;
        }

        if (capture_backend_ == CaptureBackend::desktop_duplication && duplication_) {
            auto duplication = duplication_;
            DXGI_OUTDUPL_FRAME_INFO frame_info{};
            ComPtr<IDXGIResource> resource;
            const HRESULT hr = duplication->AcquireNextFrame(0, &frame_info, &resource);
            if (hr == DXGI_ERROR_WAIT_TIMEOUT) {
                return;
            }
            if (hr == DXGI_ERROR_ACCESS_LOST || hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET) {
                if (callbacks_.on_failure) {
                    callbacks_.on_failure({FailureDomain::device,
                                           hr == DXGI_ERROR_DEVICE_RESET ? FailureCode::device_reset
                                                                         : FailureCode::device_removed,
                                           true,
                                           "Desktop Duplication device was lost"});
                }
                return;
            }
            if (SUCCEEDED(hr)) {
                ComPtr<ID3D11Texture2D> texture;
                if (SUCCEEDED(resource.As(&texture))) {
                    latest_frame_texture_ = texture;
                    D3D11_TEXTURE2D_DESC description{};
                    texture->GetDesc(&description);
                    LARGE_INTEGER qpc{};
                    QueryPerformanceCounter(&qpc);
                    const auto content_hash = windows::fingerprint_texture(
                        d3d_device_.Get(), d3d_context_.Get(), texture.Get());
                    if (callbacks_.on_frame) {
                        const auto sequence = ++frame_sequence_;
                        latest_frame_sequence_ = sequence;
                        latest_frame_qpc_ = static_cast<std::uint64_t>(qpc.QuadPart);
                        callbacks_.on_frame({sequence,
                                             device_generation_,
                                             std::chrono::steady_clock::now(),
                                             {static_cast<std::int32_t>(description.Width),
                                              static_cast<std::int32_t>(description.Height)},
                                             ColorSpace::unknown,
                                             reinterpret_cast<std::uintptr_t>(latest_frame_texture_.Get()),
                                             false,
                                             frame_info.ProtectedContentMaskedOut != FALSE,
                                             geometry_epoch_,
                                             static_cast<std::uint64_t>(qpc.QuadPart),
                                             content_hash});
                    }
                }
                duplication->ReleaseFrame();
            }
        }
    }

private:
    void clear_manual_actor_picker_authority(
        const std::string_view capture_session,
        const std::uint64_t cancellation_generation) noexcept {
        std::scoped_lock authority_lock(picker_authority_mutex_);
        if (picker_capture_session_ == capture_session &&
            picker_cancellation_generation_ == cancellation_generation) {
            picker_capture_session_.clear();
            picker_cancellation_generation_ = 0U;
        }
    }

    static void close_remote_visual_source(const HANDLE worker_process,
                                           const std::uint64_t worker_handle_value) noexcept {
        if (!worker_process || worker_handle_value == 0U) return;
        HANDLE reclaimed{};
        if (DuplicateHandle(worker_process,
                            reinterpret_cast<HANDLE>(
                                static_cast<std::uintptr_t>(worker_handle_value)),
                            GetCurrentProcess(), &reclaimed, 0U, FALSE,
                            DUPLICATE_SAME_ACCESS | DUPLICATE_CLOSE_SOURCE) && reclaimed) {
            CloseHandle(reclaimed);
        }
    }

    void cleanup_visual_source_leases(const std::uint64_t now_qpc) noexcept {
        for (auto iterator = exported_visual_sources_.begin();
             iterator != exported_visual_sources_.end();) {
            const bool worker_exited = !iterator->worker_process ||
                WaitForSingleObject(iterator->worker_process.get(), 0U) == WAIT_OBJECT_0;
            if (worker_exited || iterator->expires_qpc < now_qpc) {
                iterator = exported_visual_sources_.erase(iterator);
            } else {
                ++iterator;
            }
        }
    }

    void cleanup_identity_frame_leases(const std::uint64_t now_qpc) noexcept {
        for (auto iterator = exported_identity_frames_.begin();
             iterator != exported_identity_frames_.end();) {
            const bool worker_exited = !iterator->worker_process ||
                WaitForSingleObject(iterator->worker_process.get(), 0U) == WAIT_OBJECT_0;
            if (worker_exited || iterator->expires_qpc < now_qpc) {
                iterator = exported_identity_frames_.erase(iterator);
            } else {
                ++iterator;
            }
        }
    }

    void publish_geometry(TargetGeometry geometry,
                          const TargetState state = TargetState::selected) {
        const bool changed = !last_target_geometry_ ||
                             !same_material_geometry(*last_target_geometry_, geometry);
        if (changed) {
            // Invalidate picker authority before publishing a new geometry
            // epoch so a click commit cannot observe a half-transitioned bind.
            cancel_manual_actor_picker();
            ++geometry_epoch_;
        }
        geometry.geometry_epoch = geometry_epoch_;
        publish_picker_binding_snapshot();
        if (changed || last_reported_state_ != state) {
            last_target_geometry_ = geometry;
            last_reported_state_ = state;
            if (callbacks_.on_target_state) {
                callbacks_.on_target_state(state, geometry);
            }
        }
    }

    void publish_picker_binding_snapshot() noexcept {
        picker_target_window_.store(reinterpret_cast<std::uintptr_t>(target_window_),
                                    std::memory_order_release);
        picker_capture_backend_.store(static_cast<std::uint32_t>(capture_backend_),
                                      std::memory_order_release);
        picker_device_generation_.store(device_generation_, std::memory_order_release);
        picker_geometry_epoch_.store(geometry_epoch_, std::memory_order_release);
    }

    bool ensure_d3d_device(Failure& failure) {
        if (d3d_device_) {
            return true;
        }
        const D3D_FEATURE_LEVEL levels[]{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
        D3D_FEATURE_LEVEL selected{};
        const HRESULT hr = D3D11CreateDevice(nullptr,
                                             D3D_DRIVER_TYPE_HARDWARE,
                                             nullptr,
                                             D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                             levels,
                                             ARRAYSIZE(levels),
                                             D3D11_SDK_VERSION,
                                             &d3d_device_,
                                             &selected,
                                             &d3d_context_);
        if (FAILED(hr)) {
            failure = hresult_failure(FailureDomain::device, FailureCode::backend_unavailable, hr,
                                      "Create D3D11 device failed");
            return false;
        }
        return true;
    }

    PlatformCallbacks callbacks_;
    HWND target_window_{};
    int ptt_virtual_key_{};
    bool ptt_pressed_{};
    bool com_initialized_here_{};
    CaptureBackend capture_backend_{CaptureBackend::none};
    std::uint64_t device_generation_{};
    std::uint64_t frame_sequence_{};
    std::uint64_t geometry_epoch_{};
    std::uint64_t latest_frame_sequence_{};
    std::uint64_t latest_frame_qpc_{};
    bool advancing_frame_verified_{};
    bool wgc_suspended_for_minimize_{};
    TargetState last_reported_state_{TargetState::none};
    RectI duplication_bounds_;
    std::string duplication_monitor_id_;
    std::optional<TargetGeometry> overlay_geometry_;
    std::optional<TargetGeometry> last_target_geometry_;
    std::optional<Failure> pending_failure_;
    windows::GraphicsCapture graphics_capture_;
    windows::EventDrivenAudio audio_;
    windows::ResidualOverlay residual_overlay_;
    std::mutex picker_authority_mutex_;
    std::string picker_capture_session_;
    std::uint64_t picker_cancellation_generation_{};
    windows::ManualActorPickerOverlay manual_actor_picker_;
    std::atomic<std::uintptr_t> picker_target_window_{};
    std::atomic<std::uint32_t> picker_capture_backend_{};
    std::atomic<std::uint64_t> picker_device_generation_{};
    std::atomic<std::uint64_t> picker_geometry_epoch_{};
    ComPtr<ID3D11Device> d3d_device_;
    ComPtr<ID3D11DeviceContext> d3d_context_;
    ComPtr<IDXGIOutputDuplication> duplication_;
    ComPtr<ID3D11Texture2D> latest_frame_texture_;
    std::unique_ptr<ImportedResidual> imported_residual_;
    std::deque<SeenLeaseNonce> seen_lease_nonces_;
    std::deque<ExportedVisualSource> exported_visual_sources_;
    std::deque<ExportedIdentityFrame> exported_identity_frames_;
};

} // namespace

std::unique_ptr<IMediaPlatform> create_windows_media_platform() {
    return std::make_unique<WindowsMediaPlatform>();
}

} // namespace npc::media

#endif
