#ifdef _WIN32

#include "windows_capture.hpp"

#include <Windows.Graphics.Capture.Interop.h>
#include <d3d11_4.h>
#include <windows.graphics.directx.direct3d11.interop.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/Windows.Graphics.DirectX.Direct3D11.h>
#include <winrt/Windows.Graphics.DirectX.h>

#include <atomic>
#include <algorithm>
#include <condition_variable>
#include <mutex>
#include <utility>

namespace npc::media::windows {

using Microsoft::WRL::ComPtr;
using namespace winrt::Windows::Graphics;
using namespace winrt::Windows::Graphics::Capture;
using namespace winrt::Windows::Graphics::DirectX;
using namespace winrt::Windows::Graphics::DirectX::Direct3D11;

std::uint64_t fingerprint_texture(ID3D11Device* device,
                                  ID3D11DeviceContext* context,
                                  ID3D11Texture2D* texture) noexcept {
    if (!device || !context || !texture) return 0;
    D3D11_TEXTURE2D_DESC description{};
    texture->GetDesc(&description);
    if (description.Width == 0 || description.Height == 0 || description.SampleDesc.Count != 1) return 0;
    description.BindFlags = 0;
    description.MiscFlags = 0;
    description.Usage = D3D11_USAGE_STAGING;
    description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> staging;
    if (FAILED(device->CreateTexture2D(&description, nullptr, &staging))) return 0;
    context->CopyResource(staging.Get(), texture);
    D3D11_MAPPED_SUBRESOURCE mapped{};
    if (FAILED(context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped))) return 0;

    constexpr std::uint64_t offset_basis = 14695981039346656037ULL;
    constexpr std::uint64_t prime = 1099511628211ULL;
    std::uint64_t hash = offset_basis;
    const auto mix = [&hash](const std::uint8_t value) {
        hash ^= value;
        hash *= prime;
    };
    for (unsigned shift = 0; shift < 32; shift += 8) mix(static_cast<std::uint8_t>(description.Width >> shift));
    for (unsigned shift = 0; shift < 32; shift += 8) mix(static_cast<std::uint8_t>(description.Height >> shift));
    constexpr std::uint32_t grid = 16;
    const auto* pixels = static_cast<const std::uint8_t*>(mapped.pData);
    for (std::uint32_t y_index = 0; y_index < grid; ++y_index) {
        const auto y = std::min(description.Height - 1, (description.Height * y_index) / grid);
        const auto* row = pixels + static_cast<std::size_t>(mapped.RowPitch) * y;
        for (std::uint32_t x_index = 0; x_index < grid; ++x_index) {
            const auto x = std::min(description.Width - 1, (description.Width * x_index) / grid);
            const auto* sample = row + static_cast<std::size_t>(x) * 4;
            mix(sample[0]);
            mix(sample[1]);
            mix(sample[2]);
            mix(sample[3]);
        }
    }
    context->Unmap(staging.Get(), 0);
    return hash == 0 ? 1 : hash;
}

struct GraphicsCapture::Impl {
    struct CallbackLease {
        Impl* owner{};

        CallbackLease() = default;
        explicit CallbackLease(Impl* value) : owner(value) {}
        CallbackLease(const CallbackLease&) = delete;
        CallbackLease& operator=(const CallbackLease&) = delete;
        CallbackLease(CallbackLease&& other) noexcept
            : owner(std::exchange(other.owner, nullptr)) {}
        CallbackLease& operator=(CallbackLease&& other) noexcept {
            if (this != &other) {
                reset();
                owner = std::exchange(other.owner, nullptr);
            }
            return *this;
        }
        ~CallbackLease() { reset(); }

        explicit operator bool() const noexcept { return owner != nullptr; }

        void reset() noexcept {
            if (!owner) return;
            owner->leave_callback();
            owner = nullptr;
        }
    };

    std::mutex mutex;
    std::mutex callback_mutex;
    std::condition_variable callbacks_drained;
    bool accepting_callbacks{};
    std::size_t callbacks_in_flight{};
    ComPtr<ID3D11Device> d3d_device;
    ComPtr<ID3D11DeviceContext> d3d_context;
    IDirect3DDevice direct3d_device{nullptr};
    GraphicsCaptureItem item{nullptr};
    Direct3D11CaptureFramePool frame_pool{nullptr};
    GraphicsCaptureSession session{nullptr};
    winrt::event_token frame_token{};
    winrt::event_token closed_token{};
    SizeInt32 pool_size{};
    std::optional<OwnedCaptureFrame> latest;
    std::optional<Failure> failure;
    std::atomic_bool closed{};
    std::atomic_bool active{};
    std::uint64_t sequence{};

    [[nodiscard]] CallbackLease enter_callback() noexcept {
        std::scoped_lock lock(callback_mutex);
        if (!accepting_callbacks) return {};
        ++callbacks_in_flight;
        return CallbackLease{this};
    }

    void leave_callback() noexcept {
        std::scoped_lock lock(callback_mutex);
        if (--callbacks_in_flight == 0) callbacks_drained.notify_all();
    }

    void accept_callbacks() noexcept {
        std::scoped_lock lock(callback_mutex);
        accepting_callbacks = true;
    }

    void stop_accepting_callbacks() noexcept {
        std::scoped_lock lock(callback_mutex);
        accepting_callbacks = false;
    }

    void wait_for_callbacks() noexcept {
        std::unique_lock lock(callback_mutex);
        callbacks_drained.wait(lock, [this] { return callbacks_in_flight == 0; });
    }

    void record_failure(const winrt::hresult_error& error, const char* context) {
        std::scoped_lock lock(mutex);
        failure = {FailureDomain::capture,
                   error.code() == DXGI_ERROR_DEVICE_REMOVED || error.code() == DXGI_ERROR_DEVICE_RESET
                       ? FailureCode::device_removed
                       : FailureCode::backend_unavailable,
                   true,
                   std::string(context) + " (HRESULT=" + std::to_string(error.code().value) + ")"};
    }

    void on_frame(const Direct3D11CaptureFramePool& sender) noexcept {
        try {
            auto frame = sender.TryGetNextFrame();
            if (!frame) {
                return;
            }
            const auto content_size = frame.ContentSize();
            ComPtr<ID3D11Texture2D> frame_texture;
            auto access = frame.Surface().as<
                ::Windows::Graphics::DirectX::Direct3D11::IDirect3DDxgiInterfaceAccess>();
            winrt::check_hresult(access->GetInterface(IID_PPV_ARGS(&frame_texture)));

            D3D11_TEXTURE2D_DESC description{};
            frame_texture->GetDesc(&description);
            description.Usage = D3D11_USAGE_DEFAULT;
            description.CPUAccessFlags = 0;
            description.MiscFlags = 0;

            ComPtr<ID3D11Texture2D> owned_texture;
            winrt::check_hresult(
                d3d_device->CreateTexture2D(&description, nullptr, &owned_texture));
            d3d_context->CopyResource(owned_texture.Get(), frame_texture.Get());
            LARGE_INTEGER qpc{};
            QueryPerformanceCounter(&qpc);
            const auto content_hash = fingerprint_texture(
                d3d_device.Get(), d3d_context.Get(), owned_texture.Get());

            // The frame pool owns frame_texture. Windows explicitly forbids retaining
            // its surface after the frame is checked back in, so publish only the
            // independent copy after returning the WGC frame to the pool.
            frame.Close();

            {
                std::scoped_lock lock(mutex);
                latest = OwnedCaptureFrame{
                    std::move(owned_texture),
                    {content_size.Width, content_size.Height},
                    std::chrono::steady_clock::now(),
                    ++sequence,
                    static_cast<std::uint64_t>(qpc.QuadPart),
                    content_hash,
                };
            }

            if (content_size.Width > 0 && content_size.Height > 0 &&
                (content_size.Width != pool_size.Width || content_size.Height != pool_size.Height)) {
                pool_size = content_size;
                sender.Recreate(direct3d_device,
                                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                                2,
                                pool_size);
            }
        } catch (const winrt::hresult_error& error) {
            record_failure(error, "WGC frame acquisition failed");
        } catch (...) {
            std::scoped_lock lock(mutex);
            failure = {FailureDomain::capture, FailureCode::internal_error, true,
                       "WGC frame acquisition raised an unknown exception"};
        }
    }
};

GraphicsCapture::GraphicsCapture() : impl_(std::make_shared<Impl>()) {}
GraphicsCapture::~GraphicsCapture() { stop(); }

bool GraphicsCapture::start(const HWND window, ID3D11Device* device, Failure& failure) {
    stop();
    if (!window || !device || !GraphicsCaptureSession::IsSupported()) {
        failure = {FailureDomain::capture, FailureCode::unsupported_path, true,
                   "Windows Graphics Capture is unavailable on this system"};
        return false;
    }

    try {
        impl_->d3d_device = device;
        impl_->d3d_device->GetImmediateContext(&impl_->d3d_context);
        ComPtr<ID3D11Multithread> multithread;
        if (SUCCEEDED(impl_->d3d_context.As(&multithread))) {
            multithread->SetMultithreadProtected(TRUE);
        }

        ComPtr<IDXGIDevice> dxgi_device;
        winrt::check_hresult(device->QueryInterface(IID_PPV_ARGS(&dxgi_device)));
        ComPtr<IInspectable> inspectable;
        winrt::check_hresult(CreateDirect3D11DeviceFromDXGIDevice(dxgi_device.Get(), &inspectable));
        impl_->direct3d_device = {inspectable.Detach(), winrt::take_ownership_from_abi};

        auto interop = winrt::get_activation_factory<GraphicsCaptureItem, IGraphicsCaptureItemInterop>();
        winrt::check_hresult(interop->CreateForWindow(
            window, winrt::guid_of<GraphicsCaptureItem>(), winrt::put_abi(impl_->item)));
        impl_->pool_size = impl_->item.Size();
        if (impl_->pool_size.Width <= 0 || impl_->pool_size.Height <= 0) {
            failure = {FailureDomain::capture, FailureCode::invalid_geometry, true,
                       "WGC target has zero-sized content"};
            stop();
            return false;
        }

        impl_->frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            impl_->direct3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            impl_->pool_size);
        impl_->accept_callbacks();
        impl_->frame_token = impl_->frame_pool.FrameArrived(
            [state = std::weak_ptr<Impl>{impl_}](const Direct3D11CaptureFramePool& sender,
                                                const winrt::Windows::Foundation::IInspectable&) {
                if (const auto locked = state.lock()) {
                    auto callback = locked->enter_callback();
                    if (!callback) return;
                    locked->on_frame(sender);
                }
            });
        impl_->closed_token = impl_->item.Closed(
            [state = std::weak_ptr<Impl>{impl_}](const GraphicsCaptureItem&,
                                                const winrt::Windows::Foundation::IInspectable&) {
                if (const auto locked = state.lock()) {
                    auto callback = locked->enter_callback();
                    if (!callback) return;
                    locked->closed.store(true, std::memory_order_release);
                }
            });
        impl_->session = impl_->frame_pool.CreateCaptureSession(impl_->item);
        try {
            impl_->session.IsCursorCaptureEnabled(false);
            impl_->session.IsBorderRequired(false);
        } catch (const winrt::hresult_error&) {
            // Older Windows builds can capture correctly without these optional
            // presentation controls. Never turn this into a fake support claim.
        }
        impl_->session.StartCapture();
        impl_->active.store(true, std::memory_order_release);
        return true;
    } catch (const winrt::hresult_error& error) {
        failure = {FailureDomain::capture,
                   error.code() == E_ACCESSDENIED ? FailureCode::access_denied : FailureCode::backend_unavailable,
                   true,
                   "WGC startup failed (HRESULT=" + std::to_string(error.code().value) + ")"};
        stop();
        return false;
    }
}

void GraphicsCapture::stop() noexcept {
    impl_->active.store(false, std::memory_order_release);
    impl_->stop_accepting_callbacks();
    // Revoke the free-threaded event sources before waiting. Waiting first can
    // deadlock with the frame pool's worker while it is dispatching or
    // recreating a frame. The admission gate makes already-queued handlers
    // harmless; admitted handlers drain while the pool and every D3D object are
    // still valid.
    try {
        if (impl_->frame_pool) {
            impl_->frame_pool.FrameArrived(impl_->frame_token);
        }
    } catch (...) {
    }
    try {
        if (impl_->item) {
            impl_->item.Closed(impl_->closed_token);
        }
    } catch (...) {
    }
    impl_->wait_for_callbacks();
    try {
        if (impl_->session) {
            impl_->session.Close();
        }
        if (impl_->frame_pool) {
            impl_->frame_pool.Close();
        }
    } catch (...) {
    }
    impl_->session = nullptr;
    impl_->frame_pool = nullptr;
    impl_->item = nullptr;
    impl_->frame_token = {};
    impl_->closed_token = {};
    impl_->direct3d_device = nullptr;
    impl_->d3d_context.Reset();
    impl_->d3d_device.Reset();
    impl_->closed.store(false, std::memory_order_release);
    std::scoped_lock lock(impl_->mutex);
    impl_->latest.reset();
    impl_->failure.reset();
}

std::optional<OwnedCaptureFrame> GraphicsCapture::take_latest() {
    std::scoped_lock lock(impl_->mutex);
    auto result = std::move(impl_->latest);
    impl_->latest.reset();
    return result;
}

std::optional<Failure> GraphicsCapture::take_failure() {
    std::scoped_lock lock(impl_->mutex);
    auto result = std::move(impl_->failure);
    impl_->failure.reset();
    return result;
}

bool GraphicsCapture::take_closed() noexcept {
    return impl_->closed.exchange(false, std::memory_order_acq_rel);
}

bool GraphicsCapture::active() const noexcept { return impl_->active.load(std::memory_order_acquire); }

} // namespace npc::media::windows

#endif
