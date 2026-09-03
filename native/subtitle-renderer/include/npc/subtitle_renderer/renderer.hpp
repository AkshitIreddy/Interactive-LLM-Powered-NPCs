#pragma once

#include "npc/subtitle_renderer/types.hpp"

namespace npc::subtitle {

class CpuSubtitleRenderer final {
public:
    explicit CpuSubtitleRenderer(RenderLimits limits = {});

    [[nodiscard]] RenderResult render(const RenderRequest& request) const;
    [[nodiscard]] const RenderLimits& limits() const noexcept { return limits_; }

    /// Checks the invariant required by a premultiplied DirectComposition
    /// surface: every B/G/R component is less than or equal to alpha.
    [[nodiscard]] static bool has_safe_premultiplied_alpha(const RenderedLayer& layer) noexcept;

private:
    RenderLimits limits_;
};

/// Stable 64-bit FNV-1a over geometry, protocol identity, fallback state, and
/// pixel bytes. It is intended for deterministic tests and diagnostics, not
/// cryptographic integrity.
[[nodiscard]] std::uint64_t deterministic_layer_hash(const RenderedLayer& layer) noexcept;

} // namespace npc::subtitle
