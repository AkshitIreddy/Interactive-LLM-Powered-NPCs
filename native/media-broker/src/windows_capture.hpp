#pragma once

#ifdef _WIN32

#include "npc/media_broker/types.hpp"

#include <Windows.h>
#include <d3d11.h>
#include <wrl/client.h>

#include <memory>
#include <optional>

namespace npc::media::windows {

struct OwnedCaptureFrame {
    Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
    SizeI size_px;
    MonotonicTime captured_at;
    std::uint64_t sequence{};
    std::uint64_t captured_qpc{};
    std::uint64_t content_hash{};
};

// Bounded CPU-readable fingerprint used only as capture evidence. It samples a
// fixed grid instead of retaining or exporting game pixels.
[[nodiscard]] std::uint64_t fingerprint_texture(ID3D11Device* device,
                                                ID3D11DeviceContext* context,
                                                ID3D11Texture2D* texture) noexcept;

class GraphicsCapture final {
public:
    // The calling thread must keep a COM apartment initialized for the full
    // lifetime of every start/stop cycle.
    GraphicsCapture();
    ~GraphicsCapture();
    GraphicsCapture(const GraphicsCapture&) = delete;
    GraphicsCapture& operator=(const GraphicsCapture&) = delete;

    [[nodiscard]] bool start(HWND window, ID3D11Device* device, Failure& failure);
    void stop() noexcept;
    [[nodiscard]] std::optional<OwnedCaptureFrame> take_latest();
    [[nodiscard]] std::optional<Failure> take_failure();
    [[nodiscard]] bool take_closed() noexcept;
    [[nodiscard]] bool active() const noexcept;

private:
    struct Impl;
    std::shared_ptr<Impl> impl_;
};

} // namespace npc::media::windows

#endif
