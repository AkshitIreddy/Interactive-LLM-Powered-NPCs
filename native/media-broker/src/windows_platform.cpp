#ifdef _WIN32

#include "npc/media_broker/platform.hpp"

#include "npc/media_broker/geometry.hpp"
#include "windows_audio.hpp"
#include "windows_capture.hpp"
#include "windows_overlay.hpp"

#include <Windows.h>
#include <d3d11.h>
#include <dwmapi.h>
#include <dxgi1_6.h>
#include <wrl/client.h>

#include <atomic>
#include <memory>
#include <utility>

namespace npc::media {

using Microsoft::WRL::ComPtr;

namespace {

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
        std::to_string(reinterpret_cast<std::uintptr_t>(monitor)),
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
        target_window_ = window;
        return true;
    }

    bool start_capture(const GameTarget& target, const CaptureBackend backend, Failure& failure) override {
        stop_capture();
        target_window_ = reinterpret_cast<HWND>(target.native_window);
        if (!ensure_d3d_device(failure)) {
            return false;
        }

        if (backend == CaptureBackend::windows_graphics_capture) {
            if (!graphics_capture_.start(target_window_, d3d_device_.Get(), failure)) {
                return false;
            }
            capture_backend_ = backend;
            const auto geometry = query_target_geometry(target_window_);
            last_target_geometry_ = geometry;
            if (callbacks_.on_target_state) {
                callbacks_.on_target_state(TargetState::selected, geometry);
            }
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
                    capture_backend_ = backend;
                    if (callbacks_.on_target_state) {
                        callbacks_.on_target_state(TargetState::selected, geometry);
                    }
                    return true;
                }
            }
        }
        failure = {FailureDomain::capture, FailureCode::backend_unavailable, true,
                   "Desktop Duplication could not open the output containing the target"};
        return false;
    }

    void stop_capture() noexcept override {
        graphics_capture_.stop();
        duplication_.Reset();
        latest_frame_texture_.Reset();
        capture_backend_ = CaptureBackend::none;
    }

    bool start_overlay(const TargetGeometry& geometry, Failure& failure) override {
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
        residual_overlay_.stop();
        overlay_geometry_.reset();
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
        stop_overlay();
        stop_capture();
        d3d_context_.Reset();
        d3d_device_.Reset();
        device_generation_ = generation;
        return ensure_d3d_device(failure);
    }

    bool recreate_audio_clients(Failure& failure) override {
        return audio_.restart(failure);
    }

    SharedPcmRing* capture_pcm_ring() noexcept override { return audio_.capture_ring(); }
    SharedPcmRing* render_pcm_ring() noexcept override { return audio_.render_ring(); }

    void suppress_residual() noexcept override {
        residual_overlay_.hide();
    }

    void present_pristine(const FrameDescriptor&, const OverlayGeometry&) override {
        suppress_residual();
    }

    void present_patch(const FrameDescriptor&,
                       const MouthPatch& patch,
                       const RectI patch_bounds,
                       const OverlayGeometry&) override {
        Failure failure;
        if (!residual_overlay_.present(reinterpret_cast<ID3D11Texture2D*>(patch.native_texture),
                                       patch_bounds, failure)) {
            pending_failure_ = std::move(failure);
        }
    }

    void poll() override {
        if (auto failure = graphics_capture_.take_failure()) {
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
            auto failure = std::move(*pending_failure_);
            pending_failure_.reset();
            if (callbacks_.on_failure) {
                callbacks_.on_failure(std::move(failure));
            }
            return;
        }
        if (graphics_capture_.take_closed()) {
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

        if (!target_window_ || !IsWindow(target_window_)) {
            if (callbacks_.on_target_state) {
                callbacks_.on_target_state(TargetState::closed, std::nullopt);
            }
            target_window_ = nullptr;
            return;
        }
        if (IsIconic(target_window_)) {
            if (callbacks_.on_target_state) {
                callbacks_.on_target_state(TargetState::minimized, query_target_geometry(target_window_));
            }
            return;
        }

        if (capture_backend_ == CaptureBackend::windows_graphics_capture) {
            if (auto frame = graphics_capture_.take_latest()) {
                latest_frame_texture_ = std::move(frame->texture);
                auto geometry = query_target_geometry(target_window_);
                geometry.captured_content_px = frame->size_px;
                geometry.captured_desktop_bounds_px = geometry.client_bounds_px;
                last_target_geometry_ = geometry;
                if (callbacks_.on_frame) {
                    callbacks_.on_frame({frame->sequence,
                                         device_generation_,
                                         frame->captured_at,
                                         frame->size_px,
                                         ColorSpace::sdr_srgb,
                                         reinterpret_cast<std::uintptr_t>(latest_frame_texture_.Get()),
                                         false,
                                         false});
                }
            }
            return;
        }

        if (capture_backend_ == CaptureBackend::desktop_duplication && duplication_) {
            DXGI_OUTDUPL_FRAME_INFO frame_info{};
            ComPtr<IDXGIResource> resource;
            const HRESULT hr = duplication_->AcquireNextFrame(0, &frame_info, &resource);
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
                    if (callbacks_.on_frame) {
                        callbacks_.on_frame({++frame_sequence_,
                                             device_generation_,
                                             std::chrono::steady_clock::now(),
                                             {static_cast<std::int32_t>(description.Width),
                                              static_cast<std::int32_t>(description.Height)},
                                             ColorSpace::unknown,
                                             reinterpret_cast<std::uintptr_t>(latest_frame_texture_.Get()),
                                             false,
                                             frame_info.ProtectedContentMaskedOut != FALSE});
                    }
                }
                duplication_->ReleaseFrame();
            }
        }
    }

private:
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
    std::optional<TargetGeometry> overlay_geometry_;
    std::optional<TargetGeometry> last_target_geometry_;
    std::optional<Failure> pending_failure_;
    windows::GraphicsCapture graphics_capture_;
    windows::EventDrivenAudio audio_;
    windows::ResidualOverlay residual_overlay_;
    ComPtr<ID3D11Device> d3d_device_;
    ComPtr<ID3D11DeviceContext> d3d_context_;
    ComPtr<IDXGIOutputDuplication> duplication_;
    ComPtr<ID3D11Texture2D> latest_frame_texture_;
};

} // namespace

std::unique_ptr<IMediaPlatform> create_windows_media_platform() {
    return std::make_unique<WindowsMediaPlatform>();
}

} // namespace npc::media

#endif
