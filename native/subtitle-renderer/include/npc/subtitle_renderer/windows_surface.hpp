#pragma once

#include "npc/subtitle_renderer/presentation.hpp"

#ifdef _WIN32

#include <memory>

namespace npc::subtitle::windows {

/// Broker-owned, click-through DirectComposition subtitle surface. It owns no
/// game resource and is excluded from capture. The window is exactly the
/// tightly cropped subtitle layer, so it cannot cover the game with stale
/// transparent pixels after cancellation.
class WindowsSubtitleSurface final : public ISubtitleSurface {
public:
    WindowsSubtitleSurface();
    ~WindowsSubtitleSurface() override;
    WindowsSubtitleSurface(const WindowsSubtitleSurface&) = delete;
    WindowsSubtitleSurface& operator=(const WindowsSubtitleSurface&) = delete;

    [[nodiscard]] bool present(const RenderedLayer& layer,
                               TargetColorSpace target_color_space,
                               float sdr_white_level_nits,
                               SurfacePresentation& evidence,
                               std::string& error) override;
    void hide() noexcept override;
    void shutdown() noexcept;

    [[nodiscard]] bool capture_excluded() const noexcept;
    [[nodiscard]] bool visible() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace npc::subtitle::windows

#endif
