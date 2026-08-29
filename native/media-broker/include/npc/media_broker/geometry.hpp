#pragma once

#include "npc/media_broker/types.hpp"

#include <optional>

namespace npc::media {

struct OverlayGeometry {
    RectI desktop_bounds_px;
    RectI clipped_desktop_bounds_px;
    RectI source_crop_px;
    SizeI source_size_px;
    double dpi_scale_x{1.0};
    double dpi_scale_y{1.0};
    ColorSpace source_color_space{ColorSpace::unknown};
    ColorSpace output_color_space{ColorSpace::unknown};
    DisplayRotation display_rotation{DisplayRotation::identity};
    bool tone_map_required{};
};

[[nodiscard]] RectI intersect(RectI a, RectI b) noexcept;
[[nodiscard]] bool normalized_rect_valid(RectF rect) noexcept;
[[nodiscard]] std::optional<RectI> map_normalized_rect(RectF normalized, RectI target) noexcept;
[[nodiscard]] std::optional<RectI> map_normalized_source_rect(RectF normalized,
                                                              const OverlayGeometry& geometry) noexcept;
[[nodiscard]] std::optional<OverlayGeometry> calculate_overlay_geometry(const TargetGeometry& target) noexcept;

} // namespace npc::media
