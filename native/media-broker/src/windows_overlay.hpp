#pragma once

#ifdef _WIN32

#include "npc/media_broker/geometry.hpp"

#include <Windows.h>
#include <d3d11.h>

#include <memory>

namespace npc::media::windows {

class ResidualOverlay final {
public:
    ResidualOverlay();
    ~ResidualOverlay();
    ResidualOverlay(const ResidualOverlay&) = delete;
    ResidualOverlay& operator=(const ResidualOverlay&) = delete;

    [[nodiscard]] bool start(ID3D11Device* device,
                             const OverlayGeometry& geometry,
                             Failure& failure);
    void stop() noexcept;
    void hide() noexcept;
    [[nodiscard]] bool present(ID3D11Texture2D* premultiplied_residual,
                               RectI desktop_bounds,
                               Failure& failure);
    [[nodiscard]] bool active() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace npc::media::windows

#endif
