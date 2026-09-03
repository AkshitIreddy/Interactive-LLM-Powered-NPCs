#ifdef _WIN32

#include "windows_overlay.hpp"

#include <dcomp.h>
#include <dwmapi.h>
#include <dxgi1_6.h>
#include <wrl/client.h>

#include <algorithm>
#include <mutex>
#include <string>

namespace npc::media::windows {

using Microsoft::WRL::ComPtr;

namespace {

constexpr wchar_t overlay_class_name[] = L"NpcMediaBrokerResidualOverlay";

LRESULT CALLBACK overlay_window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
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
        definition.lpfnWndProc = overlay_window_proc;
        definition.hInstance = GetModuleHandleW(nullptr);
        definition.lpszClassName = overlay_class_name;
        definition.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(32512));
        registered = RegisterClassExW(&definition) != 0 || GetLastError() == ERROR_CLASS_ALREADY_EXISTS;
    });
    return registered;
}

[[nodiscard]] Failure overlay_failure(const HRESULT value, const char* context) {
    return {FailureDomain::overlay,
            value == DXGI_ERROR_DEVICE_REMOVED || value == DXGI_ERROR_DEVICE_RESET
                ? FailureCode::device_removed
                : FailureCode::backend_unavailable,
            true,
            std::string(context) + " (HRESULT=" + std::to_string(static_cast<long>(value)) + ")"};
}

} // namespace

struct ResidualOverlay::Impl {
    HWND window{};
    OverlayGeometry geometry;
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<IDXGISwapChain1> swap_chain;
    ComPtr<IDCompositionDevice> composition_device;
    ComPtr<IDCompositionTarget> composition_target;
    ComPtr<IDCompositionVisual> root_visual;
    bool capture_excluded{};
};

ResidualOverlay::ResidualOverlay() : impl_(std::make_unique<Impl>()) {}
ResidualOverlay::~ResidualOverlay() { stop(); }

bool ResidualOverlay::start(ID3D11Device* device,
                            const OverlayGeometry& geometry,
                            Failure& failure) {
    stop();
    if (!device || !geometry.clipped_desktop_bounds_px.valid() || !ensure_window_class()) {
        failure = {FailureDomain::overlay, FailureCode::invalid_geometry, true,
                   "Overlay device, bounds, or window class is unavailable"};
        return false;
    }

    impl_->geometry = geometry;
    impl_->device = device;
    device->GetImmediateContext(&impl_->context);
    const auto& bounds = geometry.clipped_desktop_bounds_px;
    impl_->window = CreateWindowExW(
        WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
        overlay_class_name,
        L"",
        WS_POPUP,
        bounds.left,
        bounds.top,
        bounds.width(),
        bounds.height(),
        nullptr,
        nullptr,
        GetModuleHandleW(nullptr),
        nullptr);
    if (!impl_->window) {
        failure = {FailureDomain::overlay, FailureCode::backend_unavailable, true,
                   "Create transparent overlay window failed"};
        return false;
    }
    constexpr DWORD exclude_from_capture = 0x00000011;
    DWORD applied_affinity{};
    if (!SetWindowDisplayAffinity(impl_->window, exclude_from_capture) ||
        !GetWindowDisplayAffinity(impl_->window, &applied_affinity) ||
        applied_affinity != exclude_from_capture) {
        failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                   "Windows could not exclude the residual overlay from capture"};
        stop();
        return false;
    }
    impl_->capture_excluded = true;
    const MARGINS margins{-1, -1, -1, -1};
    DwmExtendFrameIntoClientArea(impl_->window, &margins);

    ComPtr<IDXGIDevice> dxgi_device;
    HRESULT hr = impl_->device.As(&dxgi_device);
    ComPtr<IDXGIAdapter> adapter;
    if (SUCCEEDED(hr)) {
        dxgi_device->GetAdapter(&adapter);
    }
    ComPtr<IDXGIFactory2> factory;
    if (SUCCEEDED(hr)) {
        hr = adapter->GetParent(IID_PPV_ARGS(&factory));
    }
    if (SUCCEEDED(hr)) {
        DXGI_SWAP_CHAIN_DESC1 description{};
        description.Width = static_cast<UINT>(bounds.width());
        description.Height = static_cast<UINT>(bounds.height());
        description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        description.SampleDesc.Count = 1;
        description.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
        description.BufferCount = 2;
        description.Scaling = DXGI_SCALING_STRETCH;
        description.SwapEffect = DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL;
        description.AlphaMode = DXGI_ALPHA_MODE_PREMULTIPLIED;
        hr = factory->CreateSwapChainForComposition(impl_->device.Get(), &description, nullptr,
                                                    &impl_->swap_chain);
    }
    if (SUCCEEDED(hr)) {
        hr = DCompositionCreateDevice(dxgi_device.Get(), IID_PPV_ARGS(&impl_->composition_device));
    }
    if (SUCCEEDED(hr)) {
        hr = impl_->composition_device->CreateTargetForHwnd(impl_->window, TRUE,
                                                            &impl_->composition_target);
    }
    if (SUCCEEDED(hr)) {
        hr = impl_->composition_device->CreateVisual(&impl_->root_visual);
    }
    if (SUCCEEDED(hr)) {
        hr = impl_->root_visual->SetContent(impl_->swap_chain.Get());
    }
    if (SUCCEEDED(hr)) {
        hr = impl_->composition_target->SetRoot(impl_->root_visual.Get());
    }
    if (SUCCEEDED(hr)) {
        hr = impl_->composition_device->Commit();
    }
    if (FAILED(hr)) {
        failure = overlay_failure(hr, "Create DirectComposition premultiplied swap chain failed");
        stop();
        return false;
    }
    hide();
    return true;
}

void ResidualOverlay::stop() noexcept {
    if (!impl_) {
        return;
    }
    hide();
    impl_->root_visual.Reset();
    impl_->composition_target.Reset();
    impl_->composition_device.Reset();
    impl_->swap_chain.Reset();
    impl_->context.Reset();
    impl_->device.Reset();
    impl_->capture_excluded = false;
    if (impl_->window) {
        DestroyWindow(impl_->window);
        impl_->window = nullptr;
    }
}

void ResidualOverlay::hide() noexcept {
    if (impl_ && impl_->window) {
        ShowWindow(impl_->window, SW_HIDE);
    }
}

bool ResidualOverlay::present(ID3D11Texture2D* residual,
                              const RectI desktop_bounds,
                              Failure& failure) {
    if (!residual || !impl_->swap_chain || !desktop_bounds.valid()) {
        failure = {FailureDomain::overlay, FailureCode::invalid_geometry, true,
                   "Residual texture or patch bounds are unavailable"};
        hide();
        return false;
    }

    D3D11_TEXTURE2D_DESC source_description{};
    residual->GetDesc(&source_description);
    const auto clipped = intersect(desktop_bounds, impl_->geometry.clipped_desktop_bounds_px);
    if (!clipped.valid() || clipped != desktop_bounds ||
        source_description.Format != DXGI_FORMAT_B8G8R8A8_UNORM ||
        source_description.Width != static_cast<UINT>(desktop_bounds.width()) ||
        source_description.Height != static_cast<UINT>(desktop_bounds.height()) ||
        source_description.MipLevels != 1U || source_description.ArraySize != 1U ||
        source_description.SampleDesc.Count != 1U) {
        failure = {FailureDomain::overlay, FailureCode::invalid_geometry, true,
                   "Residual must exactly cover its bounds as one BGRA8 premultiplied texture"};
        hide();
        return false;
    }

    ComPtr<ID3D11Device> source_device;
    residual->GetDevice(&source_device);
    if (source_device.Get() != impl_->device.Get()) {
        failure = {FailureDomain::overlay, FailureCode::backend_unavailable, true,
                   "Residual texture is not opened on the broker D3D11 device"};
        hide();
        return false;
    }

    ComPtr<ID3D11Texture2D> back_buffer;
    HRESULT hr = impl_->swap_chain->GetBuffer(0, IID_PPV_ARGS(&back_buffer));
    ComPtr<ID3D11RenderTargetView> target_view;
    if (SUCCEEDED(hr)) {
        hr = impl_->device->CreateRenderTargetView(back_buffer.Get(), nullptr, &target_view);
    }
    if (FAILED(hr)) {
        failure = overlay_failure(hr, "Acquire overlay back buffer failed");
        hide();
        return false;
    }

    constexpr float transparent[]{0.0F, 0.0F, 0.0F, 0.0F};
    impl_->context->ClearRenderTargetView(target_view.Get(), transparent);
    const D3D11_BOX source_box{0, 0, 0, source_description.Width, source_description.Height, 1};
    impl_->context->CopySubresourceRegion(
        back_buffer.Get(),
        0,
        static_cast<UINT>(clipped.left - impl_->geometry.clipped_desktop_bounds_px.left),
        static_cast<UINT>(clipped.top - impl_->geometry.clipped_desktop_bounds_px.top),
        0,
        residual,
        0,
        &source_box);
    hr = impl_->swap_chain->Present(0, DXGI_PRESENT_DO_NOT_WAIT);
    if (FAILED(hr) && hr != DXGI_ERROR_WAS_STILL_DRAWING) {
        failure = overlay_failure(hr, "Present residual overlay failed");
        hide();
        return false;
    }
    if (hr == DXGI_ERROR_WAS_STILL_DRAWING) {
        // Never leave a previous mouth residual visible while the newest swap is
        // unavailable. Dropping one patch is preferable to a stale face overlay.
        hide();
        return true;
    }
    SetWindowPos(impl_->window, HWND_TOPMOST,
                 impl_->geometry.clipped_desktop_bounds_px.left,
                 impl_->geometry.clipped_desktop_bounds_px.top,
                 impl_->geometry.clipped_desktop_bounds_px.width(),
                 impl_->geometry.clipped_desktop_bounds_px.height(),
                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
    return true;
}

bool ResidualOverlay::active() const noexcept {
    return impl_ && impl_->window && impl_->swap_chain;
}

bool ResidualOverlay::capture_excluded() const noexcept {
    return active() && impl_->capture_excluded;
}

} // namespace npc::media::windows

#endif
