#include "npc/media_broker/geometry.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace npc::media {

RectI intersect(const RectI a, const RectI b) noexcept {
    const RectI result{
        std::max(a.left, b.left),
        std::max(a.top, b.top),
        std::min(a.right, b.right),
        std::min(a.bottom, b.bottom),
    };
    return result.valid() ? result : RectI{};
}

bool normalized_rect_valid(const RectF rect) noexcept {
    return std::isfinite(rect.left) && std::isfinite(rect.top) &&
           std::isfinite(rect.right) && std::isfinite(rect.bottom) &&
           rect.left >= 0.0 && rect.top >= 0.0 && rect.right <= 1.0 && rect.bottom <= 1.0 &&
           rect.width() > 0.0 && rect.height() > 0.0;
}

std::optional<RectI> map_normalized_rect(const RectF normalized, const RectI target) noexcept {
    if (!normalized_rect_valid(normalized) || !target.valid()) {
        return std::nullopt;
    }

    const auto project = [](const double value, const std::int32_t origin, const std::int32_t extent) {
        return static_cast<std::int32_t>(std::llround(
            static_cast<double>(origin) + value * static_cast<double>(extent)));
    };

    RectI result{
        project(normalized.left, target.left, target.width()),
        project(normalized.top, target.top, target.height()),
        project(normalized.right, target.left, target.width()),
        project(normalized.bottom, target.top, target.height()),
    };
    result = intersect(result, target);
    return result.valid() ? std::optional<RectI>{result} : std::nullopt;
}

std::optional<OverlayGeometry> calculate_overlay_geometry(const TargetGeometry& target) noexcept {
    if (!target.client_bounds_px.valid() || !target.captured_content_px.valid() ||
        !target.captured_desktop_bounds_px.valid() ||
        !target.monitor.desktop_bounds_px.valid() || target.monitor.dpi_x == 0 || target.monitor.dpi_y == 0 ||
        target.minimized) {
        return std::nullopt;
    }

    const RectI clipped = intersect(target.client_bounds_px, target.monitor.desktop_bounds_px);
    if (!clipped.valid()) {
        return std::nullopt;
    }

    const double source_per_desktop_x = static_cast<double>(target.captured_content_px.width) /
                                        static_cast<double>(target.captured_desktop_bounds_px.width());
    const double source_per_desktop_y = static_cast<double>(target.captured_content_px.height) /
                                        static_cast<double>(target.captured_desktop_bounds_px.height());

    const auto source_x = [&](const std::int32_t desktop_x) {
        return static_cast<std::int32_t>(std::llround(
            static_cast<double>(desktop_x - target.captured_desktop_bounds_px.left) * source_per_desktop_x));
    };
    const auto source_y = [&](const std::int32_t desktop_y) {
        return static_cast<std::int32_t>(std::llround(
            static_cast<double>(desktop_y - target.captured_desktop_bounds_px.top) * source_per_desktop_y));
    };

    RectI crop{
        source_x(clipped.left),
        source_y(clipped.top),
        source_x(clipped.right),
        source_y(clipped.bottom),
    };
    crop = intersect(crop, RectI{0, 0, target.captured_content_px.width, target.captured_content_px.height});
    if (!crop.valid()) {
        return std::nullopt;
    }

    const ColorSpace output_space = target.monitor.color_space;
    return OverlayGeometry{
        target.client_bounds_px,
        clipped,
        crop,
        target.captured_content_px,
        static_cast<double>(target.monitor.dpi_x) / 96.0,
        static_cast<double>(target.monitor.dpi_y) / 96.0,
        ColorSpace::sdr_srgb,
        output_space,
        target.monitor.rotation,
        output_space == ColorSpace::hdr10_pq || output_space == ColorSpace::hdr_sc_rgb,
    };
}

std::optional<RectI> map_normalized_source_rect(const RectF normalized,
                                                const OverlayGeometry& geometry) noexcept {
    if (!normalized_rect_valid(normalized) || !geometry.source_crop_px.valid() ||
        !geometry.source_size_px.valid() || !geometry.clipped_desktop_bounds_px.valid()) {
        return std::nullopt;
    }

    const RectI source_extent{0, 0, geometry.source_size_px.width, geometry.source_size_px.height};
    auto source_rect = map_normalized_rect(normalized, source_extent);
    if (!source_rect) {
        return std::nullopt;
    }
    *source_rect = intersect(*source_rect, geometry.source_crop_px);
    if (!source_rect->valid()) {
        return std::nullopt;
    }

    const double desktop_per_source_x =
        static_cast<double>(geometry.clipped_desktop_bounds_px.width()) /
        static_cast<double>(geometry.source_crop_px.width());
    const double desktop_per_source_y =
        static_cast<double>(geometry.clipped_desktop_bounds_px.height()) /
        static_cast<double>(geometry.source_crop_px.height());
    const auto x = [&](const std::int32_t value) {
        return static_cast<std::int32_t>(std::llround(
            static_cast<double>(geometry.clipped_desktop_bounds_px.left) +
            static_cast<double>(value - geometry.source_crop_px.left) * desktop_per_source_x));
    };
    const auto y = [&](const std::int32_t value) {
        return static_cast<std::int32_t>(std::llround(
            static_cast<double>(geometry.clipped_desktop_bounds_px.top) +
            static_cast<double>(value - geometry.source_crop_px.top) * desktop_per_source_y));
    };

    const RectI mapped{x(source_rect->left), y(source_rect->top),
                       x(source_rect->right), y(source_rect->bottom)};
    return mapped.valid() ? std::optional<RectI>{mapped} : std::nullopt;
}

} // namespace npc::media
