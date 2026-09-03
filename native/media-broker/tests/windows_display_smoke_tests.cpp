#ifdef _WIN32

#include "npc/media_broker/geometry.hpp"
#include "npc/media_broker/platform.hpp"
#include "npc/media_broker/types.hpp"
#include "windows_capture.hpp"
#include "windows_overlay.hpp"
#include "windows_presentation_context.hpp"

#include <Windows.h>
#include <d3d11.h>
#include <dwmapi.h>
#include <wrl/client.h>

#include <algorithm>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <optional>
#include <string>
#include <thread>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;
using namespace npc::media;
using namespace npc::media::windows;

COLORREF target_color = RGB(24, 96, 144);

class ScopedComApartment final {
public:
    ScopedComApartment() noexcept : result_(CoInitializeEx(nullptr, COINIT_MULTITHREADED)) {}
    ~ScopedComApartment() {
        if (SUCCEEDED(result_)) CoUninitialize();
    }
    ScopedComApartment(const ScopedComApartment&) = delete;
    ScopedComApartment& operator=(const ScopedComApartment&) = delete;
    [[nodiscard]] bool initialized() const noexcept { return SUCCEEDED(result_); }

private:
    HRESULT result_{};
};

struct HostMonitor {
    HMONITOR handle{};
    MONITORINFOEXW info{};
    bool primary{};
};

[[nodiscard]] std::vector<HostMonitor> enumerate_monitors() {
    std::vector<HostMonitor> monitors;
    EnumDisplayMonitors(
        nullptr, nullptr,
        [](const HMONITOR monitor, HDC, LPRECT, const LPARAM data) -> BOOL {
            auto& output = *reinterpret_cast<std::vector<HostMonitor>*>(data);
            MONITORINFOEXW info{};
            info.cbSize = sizeof(info);
            if (GetMonitorInfoW(monitor, &info)) {
                output.push_back({monitor, info, (info.dwFlags & MONITORINFOF_PRIMARY) != 0});
            }
            return TRUE;
        },
        reinterpret_cast<LPARAM>(&monitors));
    return monitors;
}

[[nodiscard]] const HostMonitor* select_nonprimary_or_primary(
    const std::vector<HostMonitor>& monitors) noexcept {
    const auto secondary = std::find_if(monitors.begin(), monitors.end(),
                                        [](const HostMonitor& value) { return !value.primary; });
    return secondary != monitors.end() ? &*secondary : monitors.empty() ? nullptr : &monitors.front();
}

LRESULT CALLBACK display_test_window_proc(const HWND window, const UINT message,
                                          const WPARAM wparam, const LPARAM lparam) {
    if (message == WM_PAINT) {
        PAINTSTRUCT paint{};
        const HDC dc = BeginPaint(window, &paint);
        RECT client{};
        GetClientRect(window, &client);
        const HBRUSH brush = CreateSolidBrush(target_color);
        FillRect(dc, &client, brush);
        DeleteObject(brush);
        EndPaint(window, &paint);
        return 0;
    }
    if (message == WM_ERASEBKGND) return 1;
    return DefWindowProcW(window, message, wparam, lparam);
}

void pump_messages() {
    MSG message{};
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

[[nodiscard]] bool target_is_not_foreground(const HWND window) noexcept {
    return GetForegroundWindow() != window;
}

[[nodiscard]] bool set_windowed_client_size(const HWND window, const HostMonitor& monitor,
                                            const int width, const int height) {
    constexpr DWORD style = WS_OVERLAPPEDWINDOW;
    constexpr DWORD ex_style = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
    if (SetWindowLongPtrW(window, GWL_STYLE, static_cast<LONG_PTR>(style)) == 0 &&
        GetLastError() != ERROR_SUCCESS) {
        return false;
    }
    RECT outer{0, 0, width, height};
    const UINT dpi = GetDpiForWindow(window);
    if (!AdjustWindowRectExForDpi(&outer, style, FALSE, ex_style, dpi == 0 ? 96U : dpi)) return false;
    const int outer_width = outer.right - outer.left;
    const int outer_height = outer.bottom - outer.top;
    const int available_width = monitor.info.rcWork.right - monitor.info.rcWork.left;
    const int available_height = monitor.info.rcWork.bottom - monitor.info.rcWork.top;
    const int x = monitor.info.rcWork.left + std::max(0, (available_width - outer_width) / 2);
    const int y = monitor.info.rcWork.top + std::max(0, (available_height - outer_height) / 2);
    return SetWindowPos(window, nullptr, x, y, outer_width, outer_height,
                        SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED) != FALSE;
}

[[nodiscard]] HWND create_test_window(const HostMonitor& monitor, const int width, const int height) {
    WNDCLASSEXW definition{sizeof(definition)};
    definition.lpfnWndProc = display_test_window_proc;
    definition.hInstance = GetModuleHandleW(nullptr);
    definition.lpszClassName = L"NpcMediaBrokerDisplayCertificationTarget";
    definition.hbrBackground = reinterpret_cast<HBRUSH>(COLOR_WINDOW + 1);
    RegisterClassExW(&definition);
    const HWND window = CreateWindowExW(
        WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, definition.lpszClassName,
        L"NPC exact-HWND display certification", WS_OVERLAPPEDWINDOW,
        monitor.info.rcWork.left + 20, monitor.info.rcWork.top + 20,
        width, height, nullptr, nullptr, definition.hInstance, nullptr);
    if (!window || !set_windowed_client_size(window, monitor, width, height)) {
        if (window) DestroyWindow(window);
        return nullptr;
    }
    ShowWindow(window, SW_SHOWNOACTIVATE);
    UpdateWindow(window);
    pump_messages();
    return window;
}

[[nodiscard]] bool create_d3d_device(ComPtr<ID3D11Device>& device,
                                     ComPtr<ID3D11DeviceContext>& context) {
    const D3D_FEATURE_LEVEL levels[]{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
    D3D_FEATURE_LEVEL selected{};
    return SUCCEEDED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr,
                                       D3D11_CREATE_DEVICE_BGRA_SUPPORT, levels,
                                       ARRAYSIZE(levels), D3D11_SDK_VERSION,
                                       &device, &selected, &context));
}

[[nodiscard]] std::optional<OwnedCaptureFrame> wait_for_frame(
    GraphicsCapture& capture, const std::uint64_t after_sequence,
    const std::optional<SizeI> expected_size = std::nullopt,
    const std::optional<std::uint64_t> different_hash = std::nullopt) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(8);
    while (std::chrono::steady_clock::now() < deadline) {
        pump_messages();
        if (auto frame = capture.take_latest(); frame && frame->sequence > after_sequence &&
            frame->captured_qpc != 0 && frame->content_hash != 0 &&
            (!expected_size || frame->size_px == *expected_size) &&
            (!different_hash || frame->content_hash != *different_hash)) {
            return frame;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    return std::nullopt;
}

[[nodiscard]] RectI client_bounds_on_desktop(const HWND window) {
    RECT client{};
    GetClientRect(window, &client);
    POINT origin{client.left, client.top};
    ClientToScreen(window, &origin);
    return {origin.x, origin.y, origin.x + client.right - client.left,
            origin.y + client.bottom - client.top};
}

[[nodiscard]] bool client_size_is(const HWND window, const int width, const int height) {
    RECT client{};
    return GetClientRect(window, &client) != FALSE &&
           client.right - client.left == width && client.bottom - client.top == height;
}

[[nodiscard]] TargetGeometry geometry_for(const HWND window, const SizeI captured_size,
                                          const HostMonitor& monitor) {
    RECT outer{};
    GetWindowRect(window, &outer);
    const auto client = client_bounds_on_desktop(window);
    const UINT dpi = GetDpiForWindow(window);
    return {
        {outer.left, outer.top, outer.right, outer.bottom},
        client,
        client,
        captured_size,
        {"live-selected-monitor",
         {monitor.info.rcMonitor.left, monitor.info.rcMonitor.top,
          monitor.info.rcMonitor.right, monitor.info.rcMonitor.bottom},
         {monitor.info.rcWork.left, monitor.info.rcWork.top,
          monitor.info.rcWork.right, monitor.info.rcWork.bottom},
         dpi == 0 ? 96U : dpi, dpi == 0 ? 96U : dpi,
         ColorSpace::unknown, DisplayRotation::identity, 80.0},
        IsIconic(window) != FALSE,
    };
}

[[nodiscard]] std::string executable_leaf() {
    std::wstring path(32768, L'\0');
    const DWORD length = GetModuleFileNameW(nullptr, path.data(), static_cast<DWORD>(path.size()));
    if (length == 0 || static_cast<std::size_t>(length) >= path.size()) return "unknown.exe";
    path.resize(length);
    const auto separator = path.find_last_of(L"\\/");
    const std::wstring leaf = separator == std::wstring::npos ? path : path.substr(separator + 1U);
    const int bytes = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, leaf.data(),
                                          static_cast<int>(leaf.size()), nullptr, 0,
                                          nullptr, nullptr);
    if (bytes <= 0) return "unknown.exe";
    std::string output(static_cast<std::size_t>(bytes), '\0');
    WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, leaf.data(),
                        static_cast<int>(leaf.size()), output.data(), bytes, nullptr, nullptr);
    return output;
}

[[nodiscard]] bool repaint_and_advance(const HWND window, GraphicsCapture& capture,
                                       std::optional<OwnedCaptureFrame>& frame,
                                       const COLORREF color) {
    if (!frame) return false;
    target_color = color;
    InvalidateRect(window, nullptr, FALSE);
    UpdateWindow(window);
    auto next = wait_for_frame(capture, frame->sequence, std::nullopt, frame->content_hash);
    if (!next) return false;
    frame = std::move(next);
    return true;
}

} // namespace

int main() {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    ScopedComApartment apartment;
    if (!apartment.initialized()) {
        std::cerr << "failed to initialize COM MTA\n";
        return EXIT_FAILURE;
    }
    const auto monitors = enumerate_monitors();
    const HostMonitor* selected_monitor = select_nonprimary_or_primary(monitors);
    if (!selected_monitor) {
        std::cerr << "no active Windows monitor was enumerated\n";
        return EXIT_FAILURE;
    }
    const bool selected_secondary = !selected_monitor->primary;
    const bool host_has_negative_origin = std::any_of(
        monitors.begin(), monitors.end(), [](const HostMonitor& monitor) {
            return monitor.info.rcMonitor.left < 0 || monitor.info.rcMonitor.top < 0;
        });

    HWND window = create_test_window(*selected_monitor, 1280, 720);
    if (!window || !target_is_not_foreground(window)) {
        std::cerr << "nonactivating display target creation failed\n";
        if (window) DestroyWindow(window);
        return EXIT_FAILURE;
    }

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    if (!create_d3d_device(device, context)) {
        std::cerr << "D3D11 display certification device creation failed\n";
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
    auto frame = wait_for_frame(capture, 0);
    if (!frame || !client_size_is(window, 1280, 720) || !target_is_not_foreground(window)) {
        std::cerr << "live exact-HWND 720p windowed capture did not advance without activation\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    if (!set_windowed_client_size(window, *selected_monitor, 1920, 1080)) {
        std::cerr << "live 1080p resize failed\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    ShowWindow(window, SW_SHOWNOACTIVATE);
    UpdateWindow(window);
    frame = wait_for_frame(capture, frame->sequence);
    if (!frame || !client_size_is(window, 1920, 1080) || !target_is_not_foreground(window)) {
        std::cerr << "live exact-HWND 1080p resize did not recover\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    const int monitor_width = selected_monitor->info.rcMonitor.right -
                              selected_monitor->info.rcMonitor.left;
    const int monitor_height = selected_monitor->info.rcMonitor.bottom -
                               selected_monitor->info.rcMonitor.top;
    const int borderless_width = std::min(1920, monitor_width);
    const int borderless_height = std::min(1080, monitor_height);
    if (SetWindowLongPtrW(window, GWL_STYLE,
                          static_cast<LONG_PTR>(WS_POPUP | WS_VISIBLE)) == 0 &&
        GetLastError() != ERROR_SUCCESS) {
        std::cerr << "borderless style transition failed\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const int borderless_x = selected_monitor->info.rcMonitor.left +
                             (monitor_width - borderless_width) / 2;
    const int borderless_y = selected_monitor->info.rcMonitor.top +
                             (monitor_height - borderless_height) / 2;
    if (!SetWindowPos(window, nullptr, borderless_x, borderless_y,
                      borderless_width, borderless_height,
                      SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED)) {
        std::cerr << "borderless 1080p transition failed\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    UpdateWindow(window);
    frame = wait_for_frame(capture, frame->sequence,
                           SizeI{borderless_width, borderless_height});
    const LONG_PTR borderless_style = GetWindowLongPtrW(window, GWL_STYLE);
    if (!frame || !client_size_is(window, borderless_width, borderless_height) ||
        frame->size_px != SizeI{borderless_width, borderless_height} ||
        (borderless_style & WS_POPUP) == 0 || (borderless_style & WS_CAPTION) != 0 ||
        !target_is_not_foreground(window)) {
        std::cerr << "live borderless exact-HWND capture truth failed: frame="
                  << (frame ? frame->size_px.width : 0) << 'x'
                  << (frame ? frame->size_px.height : 0)
                  << " borderless=" << borderless_width << 'x' << borderless_height
                  << " client_match=" << client_size_is(window, borderless_width, borderless_height)
                  << " style=" << static_cast<std::uint64_t>(borderless_style)
                  << " foreground=" << (GetForegroundWindow() == window) << '\n';
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    // The no-focus constraint precludes generating a literal Alt+Tab. An
    // advancing nonforeground capture proves the relevant inactive-window path.
    if (!repaint_and_advance(window, capture, frame, RGB(132, 42, 84)) ||
        !target_is_not_foreground(window)) {
        std::cerr << "nonforeground exact-HWND capture stopped advancing\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    if (!set_windowed_client_size(window, *selected_monitor, 1280, 720)) {
        std::cerr << "windowed restore before minimize failed\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    ShowWindow(window, SW_SHOWNOACTIVATE);
    UpdateWindow(window);
    frame = wait_for_frame(capture, frame->sequence);
    if (!frame || !client_size_is(window, 1280, 720)) {
        std::cerr << "windowed capture did not resume after borderless transition\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    ShowWindow(window, SW_SHOWMINNOACTIVE);
    pump_messages();
    if (!IsIconic(window) || !target_is_not_foreground(window)) {
        std::cerr << "nonactivating minimize state was not observed\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    ShowWindow(window, SW_SHOWNOACTIVATE);
    if (!set_windowed_client_size(window, *selected_monitor, 1280, 720)) {
        std::cerr << "nonactivating restore failed\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    target_color = RGB(28, 140, 92);
    InvalidateRect(window, nullptr, FALSE);
    UpdateWindow(window);
    const auto pre_minimize_hash = frame->content_hash;
    auto restored_frame = wait_for_frame(capture, frame->sequence, std::nullopt,
                                         pre_minimize_hash);
    bool minimize_restart_required{};
    if (!restored_frame && client_size_is(window, 1280, 720) && !IsIconic(window) &&
        target_is_not_foreground(window) && !capture.take_failure() && !capture.take_closed()) {
        // Raw WGC can stop delivering after a minimized target is restored.
        // Product recovery must explicitly rebind the same selected HWND.
        capture.stop();
        minimize_restart_required = true;
        if (capture.start(window, device.Get(), failure)) {
            restored_frame = wait_for_frame(capture, 0, std::nullopt, pre_minimize_hash);
        }
    }
    frame = std::move(restored_frame);
    if (!frame || !client_size_is(window, 1280, 720) || IsIconic(window) ||
        !target_is_not_foreground(window)) {
        const auto capture_failure = capture.take_failure();
        std::cerr << "exact-HWND capture did not recover after minimize/restore: frame="
                  << static_cast<bool>(frame)
                  << " client_match=" << client_size_is(window, 1280, 720)
                  << " iconic=" << (IsIconic(window) != FALSE)
                  << " foreground=" << (GetForegroundWindow() == window)
                  << " closed=" << capture.take_closed();
        if (capture_failure) std::cerr << " failure=" << capture_failure->message;
        std::cerr << '\n';
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    const auto target_geometry = geometry_for(window, frame->size_px, *selected_monitor);
    const auto mapped_geometry = calculate_overlay_geometry(target_geometry);
    ResidualOverlay overlay;
    if (!mapped_geometry || !overlay.start(device.Get(), *mapped_geometry, failure) ||
        !overlay.capture_excluded()) {
        std::cerr << (failure.message.empty() ? "capture-excluded overlay start failed" : failure.message)
                  << '\n';
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    constexpr UINT residual_width = 96;
    constexpr UINT residual_height = 96;
    std::vector<std::uint32_t> pixels(residual_width * residual_height, 0xE02070C0U);
    D3D11_TEXTURE2D_DESC texture_description{};
    texture_description.Width = residual_width;
    texture_description.Height = residual_height;
    texture_description.MipLevels = 1;
    texture_description.ArraySize = 1;
    texture_description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    texture_description.SampleDesc.Count = 1;
    texture_description.Usage = D3D11_USAGE_IMMUTABLE;
    texture_description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    const D3D11_SUBRESOURCE_DATA texture_data{pixels.data(), residual_width * 4U, 0};
    ComPtr<ID3D11Texture2D> residual;
    if (FAILED(device->CreateTexture2D(&texture_description, &texture_data, &residual))) {
        std::cerr << "overlay residual texture creation failed\n";
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const RectI client = client_bounds_on_desktop(window);
    const RectI patch{client.left + client.width() / 2 - static_cast<int>(residual_width / 2U),
                      client.top + client.height() / 2 - static_cast<int>(residual_height / 2U),
                      client.left + client.width() / 2 + static_cast<int>(residual_width / 2U),
                      client.top + client.height() / 2 + static_cast<int>(residual_height / 2U)};
    if (!overlay.present(residual.Get(), patch, failure)) {
        std::cerr << failure.message << '\n';
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    DwmFlush();
    DWORD target_affinity{};
    if (!GetWindowDisplayAffinity(window, &target_affinity) || target_affinity != WDA_NONE ||
        !overlay.capture_excluded()) {
        std::cerr << "overlay exclusion altered the selected target affinity or was not attested\n";
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    Diagnostics diagnostics;
    diagnostics.selected_window = reinterpret_cast<std::uintptr_t>(window);
    diagnostics.selected_process_id = GetCurrentProcessId();
    diagnostics.selected_executable_name = executable_leaf();
    diagnostics.device_generation = 1;
    diagnostics.geometry_epoch = 1;
    diagnostics.latest_frame_sequence = frame->sequence;
    diagnostics.latest_frame_qpc = frame->captured_qpc;
    diagnostics.latest_content_size_px = frame->size_px;
    diagnostics.capture_backend = CaptureBackend::windows_graphics_capture;
    diagnostics.overlay_capture_excluded = true;
    diagnostics.overlay_visuals_allowed = true;
    std::string presentation_error;
    const auto presentation = query_trusted_subtitle_presentation_context(diagnostics,
                                                                          presentation_error);
    if (!presentation || !presentation->dpi_available || presentation->dpi_x == 0 ||
        presentation->capture_backend != CaptureBackend::windows_graphics_capture ||
        presentation->capture_scope != 1U || !presentation->overlay_capture_excluded) {
        std::cerr << "trusted live display evidence failed: " << presentation_error << '\n';
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    overlay.stop();

    // Destroying the exact target while capture is active must close the item;
    // a replacement HWND is then selected without reusing stale frames.
    const HWND lost_window = window;
    DestroyWindow(lost_window);
    window = nullptr;
    bool closed{};
    const auto closed_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (!closed && std::chrono::steady_clock::now() < closed_deadline) {
        pump_messages();
        closed = capture.take_closed();
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    capture.stop();
    if (!closed) {
        std::cerr << "destroyed exact-HWND target did not produce target-loss closure\n";
        return EXIT_FAILURE;
    }
    window = create_test_window(*selected_monitor, 1280, 720);
    if (!window || !capture.start(window, device.Get(), failure)) {
        std::cerr << "replacement exact-HWND target reselect failed: " << failure.message << '\n';
        if (window) DestroyWindow(window);
        return EXIT_FAILURE;
    }
    frame = wait_for_frame(capture, 0);
    if (!frame || !client_size_is(window, 1280, 720) || !target_is_not_foreground(window)) {
        std::cerr << "replacement exact-HWND target did not advance\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    // This is a live D3D recreate/rebind exercise. Forced device-loss policy
    // and generation advancement are covered by the simulated broker matrix.
    capture.stop();
    context.Reset();
    device.Reset();
    if (!create_d3d_device(device, context) || !capture.start(window, device.Get(), failure)) {
        std::cerr << "live graphics-device recreate/rebind failed: " << failure.message << '\n';
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    frame = wait_for_frame(capture, 0);
    if (!frame || !client_size_is(window, 1280, 720) || !target_is_not_foreground(window)) {
        std::cerr << "capture did not recover on recreated graphics device\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    capture.stop();

    // Exercise the product platform's automatic minimize/restore transition,
    // including a strictly advancing sequence after the internal WGC rebind.
    auto product_platform = create_windows_media_platform();
    std::optional<FrameDescriptor> product_frame;
    std::optional<Failure> product_failure;
    TargetState product_target_state{TargetState::none};
    product_platform->set_callbacks({
        .on_frame = [&](FrameDescriptor value) { product_frame = std::move(value); },
        .on_failure = [&](Failure value) { product_failure = std::move(value); },
        .on_target_state = [&](const TargetState state, std::optional<TargetGeometry>) {
            product_target_state = state;
        },
    });
    const GameTarget product_target{
        reinterpret_cast<std::uintptr_t>(window), GetCurrentProcessId(),
        executable_leaf(), "Display certification exact-HWND target"};
    Failure product_setup_failure;
    if (!product_platform->recreate_graphics_device(1, product_setup_failure) ||
        !product_platform->validate_target(product_target, product_setup_failure) ||
        !product_platform->start_capture(product_target,
                                         CaptureBackend::windows_graphics_capture,
                                         product_setup_failure)) {
        std::cerr << "product WGC minimize/restore setup failed: "
                  << product_setup_failure.message << '\n';
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const auto product_initial_deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(8);
    bool product_color_toggle{};
    while ((!product_frame || product_frame->sequence < 2U) && !product_failure &&
           std::chrono::steady_clock::now() < product_initial_deadline) {
        product_color_toggle = !product_color_toggle;
        target_color = product_color_toggle ? RGB(52, 72, 184) : RGB(184, 72, 52);
        InvalidateRect(window, nullptr, FALSE);
        UpdateWindow(window);
        pump_messages();
        product_platform->poll();
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!product_frame || product_frame->sequence < 2U || product_failure) {
        std::cerr << "product WGC did not establish advancing pre-minimize frames";
        if (product_failure) std::cerr << ": " << product_failure->message;
        std::cerr << '\n';
        product_platform->stop_capture();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const auto sequence_before_product_minimize = product_frame->sequence;
    const auto qpc_before_product_minimize = product_frame->captured_qpc;
    ShowWindow(window, SW_SHOWMINNOACTIVE);
    const auto product_minimize_deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(3);
    while (product_target_state != TargetState::minimized && !product_failure &&
           std::chrono::steady_clock::now() < product_minimize_deadline) {
        pump_messages();
        product_platform->poll();
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (product_target_state != TargetState::minimized || product_failure ||
        !target_is_not_foreground(window)) {
        std::cerr << "product platform did not publish nonactivating minimized state\n";
        product_platform->stop_capture();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    ShowWindow(window, SW_SHOWNOACTIVATE);
    if (!set_windowed_client_size(window, *selected_monitor, 1280, 720)) {
        std::cerr << "product platform target restore failed\n";
        product_platform->stop_capture();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    target_color = RGB(52, 72, 184);
    InvalidateRect(window, nullptr, FALSE);
    UpdateWindow(window);
    const auto product_restore_deadline =
        std::chrono::steady_clock::now() + std::chrono::seconds(8);
    while ((!product_frame || product_frame->sequence <= sequence_before_product_minimize ||
            product_frame->captured_qpc <= qpc_before_product_minimize ||
           product_target_state != TargetState::selected) &&
           !product_failure && std::chrono::steady_clock::now() < product_restore_deadline) {
        InvalidateRect(window, nullptr, FALSE);
        UpdateWindow(window);
        pump_messages();
        product_platform->poll();
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!product_frame || product_frame->sequence <= sequence_before_product_minimize ||
        product_frame->captured_qpc <= qpc_before_product_minimize ||
        product_target_state != TargetState::selected || product_failure ||
        !target_is_not_foreground(window)) {
        std::cerr << "product WGC did not recover a strictly advancing frame after restore";
        if (product_failure) std::cerr << ": " << product_failure->message;
        std::cerr << '\n';
        product_platform->stop_capture();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    product_platform->stop_capture();
    DestroyWindow(window);

    std::cout << "LIVE current-host display evidence: monitors=" << monitors.size()
              << " selected=";
    std::wcout << selected_monitor->info.szDevice;
    std::cout << " selected_secondary=" << selected_secondary
              << " selected_bounds=" << selected_monitor->info.rcMonitor.left << ','
              << selected_monitor->info.rcMonitor.top << ','
              << selected_monitor->info.rcMonitor.right << ','
              << selected_monitor->info.rcMonitor.bottom
              << " negative_origin_present=" << host_has_negative_origin
              << " dpi=" << presentation->dpi_x
              << " hdr_evidence_available=" << presentation->hdr_evidence_available
              << " hdr_supported=" << presentation->hdr_supported
              << " hdr_user_enabled=" << presentation->hdr_user_enabled
              << " hdr_active=" << presentation->hdr_active
              << " color_encoding_available=" << presentation->color_encoding_available
              << " bits_per_channel=" << presentation->bits_per_color_channel
              << " minimize_restart_required=" << minimize_restart_required << '\n';
    std::cout << "LIVE exact-HWND WGC passed: nonactivating secondary target, 1280x720 and "
                 "1920x1080 windowed resize, 1920x1080 borderless, nonforeground continuity, "
                 "minimize/restore with explicit same-target WGC rebind when required, target "
                 "loss/reselect, D3D recreate/rebind, and external-overlay "
                 "capture-affinity exclusion attestation. Exclusive fullscreen and a literal "
                 "Alt+Tab were not performed.\n";
    std::cout << "LIVE product platform minimize/restore recovery passed with a strictly advancing "
                 "post-rebind sequence/QPC and no target activation.\n";
    std::cout << (monitors.size() == 1U
                      ? "LIVE physical single-monitor path passed without mirror/self-capture workaround.\n"
                      : "NOT LIVE: physical single-monitor topology unavailable; single-monitor behavior is simulated only.\n");
    return EXIT_SUCCESS;
}

#else

int main() { return 0; }

#endif
