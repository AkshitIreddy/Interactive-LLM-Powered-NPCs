#ifdef _WIN32

#include "npc/subtitle_renderer/windows_surface.hpp"

#include <Windows.h>
#include <d3d11.h>
#include <dcomp.h>
#include <dwmapi.h>
#include <dxgi1_6.h>
#include <wrl/client.h>

#include <cmath>
#include <mutex>
#include <string>

namespace npc::subtitle::windows {
namespace {

using Microsoft::WRL::ComPtr;

constexpr wchar_t subtitle_window_class[] = L"NpcSubtitlePresenterWindowV1";

LRESULT CALLBACK subtitle_window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_NCHITTEST) {
        return HTTRANSPARENT;
    }
    if (message == WM_MOUSEACTIVATE) {
        return MA_NOACTIVATE;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

[[nodiscard]] bool ensure_window_class() {
    static std::once_flag once;
    static bool registered{};
    std::call_once(once, [] {
        WNDCLASSEXW definition{};
        definition.cbSize = sizeof(definition);
        definition.lpfnWndProc = subtitle_window_proc;
        definition.hInstance = GetModuleHandleW(nullptr);
        definition.lpszClassName = subtitle_window_class;
        definition.hCursor = LoadCursorW(nullptr, IDC_ARROW);
        registered = RegisterClassExW(&definition) != 0 ||
                     GetLastError() == ERROR_CLASS_ALREADY_EXISTS;
    });
    return registered;
}

[[nodiscard]] std::string hr_error(const char* operation, const HRESULT result) {
    return std::string(operation) + " failed (HRESULT=" +
           std::to_string(static_cast<long>(result)) + ')';
}

[[nodiscard]] bool requires_compositor_mapping(const TargetColorSpace value) noexcept {
    // This surface is always BGRA8/sRGB-authored. Only a measured SDR-sRGB
    // target can truthfully claim the direct SDR treatment; scRGB, HDR, and
    // unavailable console color metadata are all delegated to the compositor.
    return value != TargetColorSpace::sdr_srgb;
}

} // namespace

struct WindowsSubtitleSurface::Impl {
    HWND window{};
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<IDXGISwapChain1> swap_chain;
    ComPtr<IDCompositionDevice> composition_device;
    ComPtr<IDCompositionTarget> composition_target;
    ComPtr<IDCompositionVisual> root_visual;
    std::uint32_t width{};
    std::uint32_t height{};
    bool capture_excluded{};
    bool visible{};

    void release_surface() noexcept {
        if (window) {
            ShowWindow(window, SW_HIDE);
        }
        visible = false;
        root_visual.Reset();
        composition_target.Reset();
        composition_device.Reset();
        swap_chain.Reset();
        context.Reset();
        device.Reset();
        width = 0;
        height = 0;
        capture_excluded = false;
        if (window) {
            DestroyWindow(window);
            window = nullptr;
        }
    }

    [[nodiscard]] bool create(const RenderedLayer& layer, std::string& error) {
        release_surface();
        if (!ensure_window_class()) {
            error = "subtitle overlay window class is unavailable";
            return false;
        }
        window = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            subtitle_window_class, L"", WS_POPUP, layer.desktop_x_px, layer.desktop_y_px,
            static_cast<int>(layer.width_px), static_cast<int>(layer.height_px), nullptr, nullptr,
            GetModuleHandleW(nullptr), nullptr);
        if (!window) {
            error = "creating the subtitle overlay window failed";
            return false;
        }
        constexpr DWORD exclude_from_capture = 0x00000011;
        DWORD applied{};
        if (!SetWindowDisplayAffinity(window, exclude_from_capture) ||
            !GetWindowDisplayAffinity(window, &applied) || applied != exclude_from_capture) {
            error = "Windows could not exclude the subtitle overlay from capture";
            release_surface();
            return false;
        }
        capture_excluded = true;
        const MARGINS margins{-1, -1, -1, -1};
        DwmExtendFrameIntoClientArea(window, &margins);

        UINT flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
#ifndef NDEBUG
        flags |= D3D11_CREATE_DEVICE_DEBUG;
#endif
        constexpr D3D_FEATURE_LEVEL levels[]{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
        D3D_FEATURE_LEVEL selected{};
        HRESULT result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, flags,
                                           levels, static_cast<UINT>(std::size(levels)),
                                           D3D11_SDK_VERSION, &device, &selected, &context);
#ifndef NDEBUG
        if (result == DXGI_ERROR_SDK_COMPONENT_MISSING) {
            flags &= ~D3D11_CREATE_DEVICE_DEBUG;
            result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, flags,
                                       levels, static_cast<UINT>(std::size(levels)),
                                       D3D11_SDK_VERSION, &device, &selected, &context);
        }
#endif
        ComPtr<IDXGIDevice> dxgi_device;
        ComPtr<IDXGIAdapter> adapter;
        ComPtr<IDXGIFactory2> factory;
        if (SUCCEEDED(result)) {
            result = device.As(&dxgi_device);
        }
        if (SUCCEEDED(result)) {
            result = dxgi_device->GetAdapter(&adapter);
        }
        if (SUCCEEDED(result)) {
            result = adapter->GetParent(IID_PPV_ARGS(&factory));
        }
        if (SUCCEEDED(result)) {
            DXGI_SWAP_CHAIN_DESC1 description{};
            description.Width = layer.width_px;
            description.Height = layer.height_px;
            description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
            description.SampleDesc.Count = 1;
            description.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
            description.BufferCount = 2;
            description.Scaling = DXGI_SCALING_STRETCH;
            description.SwapEffect = DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL;
            description.AlphaMode = DXGI_ALPHA_MODE_PREMULTIPLIED;
            result = factory->CreateSwapChainForComposition(device.Get(), &description, nullptr,
                                                             &swap_chain);
        }
        if (SUCCEEDED(result)) {
            result = DCompositionCreateDevice(dxgi_device.Get(),
                                              IID_PPV_ARGS(&composition_device));
        }
        if (SUCCEEDED(result)) {
            result = composition_device->CreateTargetForHwnd(window, TRUE, &composition_target);
        }
        if (SUCCEEDED(result)) {
            result = composition_device->CreateVisual(&root_visual);
        }
        if (SUCCEEDED(result)) {
            result = root_visual->SetContent(swap_chain.Get());
        }
        if (SUCCEEDED(result)) {
            result = composition_target->SetRoot(root_visual.Get());
        }
        if (SUCCEEDED(result)) {
            result = composition_device->Commit();
        }
        if (FAILED(result)) {
            error = hr_error("creating DirectComposition subtitle surface", result);
            release_surface();
            return false;
        }
        width = layer.width_px;
        height = layer.height_px;
        return true;
    }
};

WindowsSubtitleSurface::WindowsSubtitleSurface() : impl_(std::make_unique<Impl>()) {}
WindowsSubtitleSurface::~WindowsSubtitleSurface() { shutdown(); }

bool WindowsSubtitleSurface::present(const RenderedLayer& layer,
                                     const TargetColorSpace target_color_space,
                                     const float sdr_white_level_nits,
                                     SurfacePresentation& evidence,
                                     std::string& error) {
    evidence = {};
    error.clear();
    if (layer.empty() || !CpuSubtitleRenderer::has_safe_premultiplied_alpha(layer) ||
        !std::isfinite(sdr_white_level_nits) || sdr_white_level_nits < 40.0F ||
        sdr_white_level_nits > 1000.0F) {
        error = "subtitle layer or color metadata is invalid";
        hide();
        return false;
    }
    if (!impl_->swap_chain || impl_->width != layer.width_px || impl_->height != layer.height_px) {
        if (!impl_->create(layer, error)) {
            return false;
        }
    }
    ComPtr<ID3D11Texture2D> back_buffer;
    HRESULT result = impl_->swap_chain->GetBuffer(0, IID_PPV_ARGS(&back_buffer));
    if (SUCCEEDED(result)) {
        impl_->context->UpdateSubresource(back_buffer.Get(), 0, nullptr,
                                          layer.bgra_premultiplied.data(), layer.stride_bytes, 0);
        result = impl_->swap_chain->Present(1, 0);
    }
    if (FAILED(result)) {
        error = hr_error("committing subtitle swap chain", result);
        hide();
        return false;
    }
    SetWindowPos(impl_->window, HWND_TOPMOST, layer.desktop_x_px, layer.desktop_y_px,
                 static_cast<int>(layer.width_px), static_cast<int>(layer.height_px),
                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
    impl_->visible = true;
    LARGE_INTEGER timestamp{};
    QueryPerformanceCounter(&timestamp);
    evidence.presented_qpc = static_cast<std::uint64_t>(timestamp.QuadPart);
    evidence.committed = true;
    // BGRA8 remains sRGB-authored. On an HDR desktop the Windows compositor
    // maps the SDR visual using the display's SDR-white setting; this surface
    // does not claim a custom PQ/scRGB shader.
    evidence.color_treatment = requires_compositor_mapping(target_color_space)
                                   ? ColorTreatment::windows_compositor_sdr_white_mapping
                                   : ColorTreatment::sdr_premultiplied_source_over;
    return true;
}

void WindowsSubtitleSurface::hide() noexcept {
    if (impl_ && impl_->window) {
        ShowWindow(impl_->window, SW_HIDE);
        impl_->visible = false;
    }
}

void WindowsSubtitleSurface::shutdown() noexcept {
    if (impl_) {
        impl_->release_surface();
    }
}

bool WindowsSubtitleSurface::capture_excluded() const noexcept {
    return impl_ && impl_->capture_excluded;
}

bool WindowsSubtitleSurface::visible() const noexcept { return impl_ && impl_->visible; }

} // namespace npc::subtitle::windows

#endif
