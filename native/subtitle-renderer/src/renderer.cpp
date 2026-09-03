#include "npc/subtitle_renderer/renderer.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <limits>
#include <numeric>
#include <utility>

namespace npc::subtitle {
namespace {

struct IntRect {
    std::int32_t left{};
    std::int32_t top{};
    std::int32_t right{};
    std::int32_t bottom{};

    [[nodiscard]] std::int32_t width() const noexcept { return right - left; }
    [[nodiscard]] std::int32_t height() const noexcept { return bottom - top; }
    [[nodiscard]] bool valid() const noexcept { return right > left && bottom > top; }
};

struct Placement {
    ResolvedLayout layout;
    std::int32_t offset_x{};
    std::int32_t offset_y{};
    bool fallback{};
};

struct CoveragePlane {
    std::uint32_t width{};
    std::uint32_t height{};
    std::vector<std::uint8_t> values;

    CoveragePlane() = default;
    CoveragePlane(const std::uint32_t width_value, const std::uint32_t height_value)
        : width(width_value), height(height_value),
          values(static_cast<std::size_t>(width_value) * height_value, 0) {}

    [[nodiscard]] std::uint8_t at(const std::uint32_t x, const std::uint32_t y) const noexcept {
        return values[static_cast<std::size_t>(y) * width + x];
    }
    std::uint8_t& at(const std::uint32_t x, const std::uint32_t y) noexcept {
        return values[static_cast<std::size_t>(y) * width + x];
    }
};

[[nodiscard]] RenderResult fail(const RenderErrorCode code, std::string message) {
    RenderResult result;
    result.error = RenderError{code, std::move(message)};
    return result;
}

[[nodiscard]] bool finite_nonnegative(const float value) noexcept {
    return std::isfinite(value) && value >= 0.0F;
}

[[nodiscard]] std::optional<IntRect> intersect(const IntRect a, const IntRect b) noexcept {
    const IntRect result{std::max(a.left, b.left), std::max(a.top, b.top),
                         std::min(a.right, b.right), std::min(a.bottom, b.bottom)};
    if (!result.valid()) {
        return std::nullopt;
    }
    return result;
}

[[nodiscard]] IntRect enclosing(const RectF rectangle) noexcept {
    return {static_cast<std::int32_t>(std::floor(rectangle.x)),
            static_cast<std::int32_t>(std::floor(rectangle.y)),
            static_cast<std::int32_t>(std::ceil(rectangle.right())),
            static_cast<std::int32_t>(std::ceil(rectangle.bottom()))};
}

[[nodiscard]] RectF translated(RectF rectangle, const float dx, const float dy) noexcept {
    rectangle.x += dx;
    rectangle.y += dy;
    return rectangle;
}

[[nodiscard]] bool intersects(const RectF a, const RectF b) noexcept {
    return a.finite_positive() && b.finite_positive() && a.x < b.right() && a.right() > b.x &&
           a.y < b.bottom() && a.bottom() > b.y;
}

[[nodiscard]] Placement resolve_placement(const RenderRequest& request) {
    Placement placement{request.layout, 0, 0, false};
    if (request.layout.bounds_px.finite_positive() &&
        intersects(request.layout.bounds_px, request.viewport_px)) {
        return placement;
    }
    if (!request.fallback.enabled || !request.layout.bounds_px.finite_positive()) {
        return placement;
    }

    const auto& viewport = request.viewport_px;
    const float safe_margin = std::max(0.0F, request.fallback.safe_margin_px);
    const float usable_left = viewport.x + safe_margin;
    const float usable_right = viewport.right() - safe_margin;
    const float usable_top = viewport.y + safe_margin;
    const float usable_bottom = viewport.bottom() - safe_margin;
    const float width = std::min(request.layout.bounds_px.width,
                                 std::max(1.0F, usable_right - usable_left));
    const float height = std::min(request.layout.bounds_px.height,
                                  std::max(1.0F, usable_bottom - usable_top));
    const float target_x = std::clamp(viewport.x + (viewport.width - width) * 0.5F,
                                      usable_left, std::max(usable_left, usable_right - width));
    const float desired_y = usable_bottom - request.fallback.bottom_offset_px - height;
    const float target_y = std::clamp(desired_y, usable_top,
                                      std::max(usable_top, usable_bottom - height));
    const float dx = target_x - request.layout.bounds_px.x;
    const float dy = target_y - request.layout.bounds_px.y;

    placement.layout.bounds_px = translated(request.layout.bounds_px, dx, dy);
    placement.layout.bounds_px.width = width;
    placement.layout.bounds_px.height = height;
    placement.layout.body_bounds_px = translated(request.layout.body_bounds_px, dx, dy);
    if (placement.layout.speaker_bounds_px) {
        placement.layout.speaker_bounds_px = translated(*placement.layout.speaker_bounds_px, dx, dy);
    }
    placement.layout.anchor = AnchorKind::bottom_center;
    placement.offset_x = static_cast<std::int32_t>(std::lround(dx));
    placement.offset_y = static_cast<std::int32_t>(std::lround(dy));
    placement.fallback = true;
    return placement;
}

[[nodiscard]] bool style_valid(const ResolvedStyle& style, const RenderLimits& limits) noexcept {
    const auto within_extent = [&](const float value) {
        return finite_nonnegative(value) && value <= limits.max_effect_extent_px;
    };
    return style.body.valid() && style.speaker.valid() && style.outline.color.valid() &&
           style.shadow.color.valid() && style.backplate.fill.valid() &&
           style.backplate.border.valid() && within_extent(style.outline.width_px) &&
           std::isfinite(style.shadow.offset_x_px) && std::isfinite(style.shadow.offset_y_px) &&
           std::abs(style.shadow.offset_x_px) <= limits.max_effect_extent_px &&
           std::abs(style.shadow.offset_y_px) <= limits.max_effect_extent_px &&
           within_extent(style.shadow.blur_radius_px) &&
           within_extent(style.backplate.corner_radius_px) &&
           within_extent(style.backplate.border_width_px) && std::isfinite(style.global_alpha) &&
           style.global_alpha >= 0.0F && style.global_alpha <= 1.0F;
}

[[nodiscard]] float effect_extent(const ResolvedStyle& style) noexcept {
    float extent = 1.0F;
    if (style.outline.enabled) {
        extent = std::max(extent, std::ceil(style.outline.width_px) + 1.0F);
    }
    if (style.shadow.enabled) {
        extent = std::max(extent,
                          std::ceil(style.shadow.blur_radius_px * 2.0F +
                                    std::max(std::abs(style.shadow.offset_x_px),
                                             std::abs(style.shadow.offset_y_px))) +
                              1.0F);
    }
    return extent;
}

[[nodiscard]] std::optional<IntRect> compute_layer_bounds(const RenderRequest& request,
                                                           const Placement& placement) {
    const float extent = effect_extent(request.style);
    const RectF expanded{placement.layout.bounds_px.x - extent,
                         placement.layout.bounds_px.y - extent,
                         placement.layout.bounds_px.width + extent * 2.0F,
                         placement.layout.bounds_px.height + extent * 2.0F};
    const auto viewport_clip = intersect(enclosing(request.viewport_px), enclosing(request.output_clip_px));
    if (!viewport_clip) {
        return std::nullopt;
    }
    return intersect(enclosing(expanded), *viewport_clip);
}

void merge_glyph_masks(const RenderRequest& request,
                       const Placement& placement,
                       const IntRect layer_bounds,
                       CoveragePlane& body,
                       CoveragePlane& speaker) {
    for (const auto& run : request.glyph_runs) {
        auto& destination = run.role == TextRole::speaker ? speaker : body;
        RectF translated_clip = translated(run.clip_px, static_cast<float>(placement.offset_x),
                                            static_cast<float>(placement.offset_y));
        const RectF role_bounds = run.role == TextRole::speaker
                                      ? *placement.layout.speaker_bounds_px
                                      : placement.layout.body_bounds_px;
        const auto run_and_role = intersect(enclosing(translated_clip), enclosing(role_bounds));
        if (!run_and_role) {
            continue;
        }
        const auto clip = intersect(*run_and_role, layer_bounds);
        if (!clip) {
            continue;
        }
        for (const auto& glyph : run.glyphs) {
            const std::int32_t glyph_left = glyph.origin_x_px + placement.offset_x;
            const std::int32_t glyph_top = glyph.origin_y_px + placement.offset_y;
            const IntRect glyph_bounds{glyph_left, glyph_top,
                                       glyph_left + static_cast<std::int32_t>(glyph.width_px),
                                       glyph_top + static_cast<std::int32_t>(glyph.height_px)};
            const auto visible = intersect(glyph_bounds, *clip);
            if (!visible) {
                continue;
            }
            for (std::int32_t y = visible->top; y < visible->bottom; ++y) {
                const auto source_y = static_cast<std::uint32_t>(y - glyph_top);
                const auto destination_y = static_cast<std::uint32_t>(y - layer_bounds.top);
                const std::size_t source_row = static_cast<std::size_t>(source_y) * glyph.stride_bytes;
                for (std::int32_t x = visible->left; x < visible->right; ++x) {
                    const auto source_x = static_cast<std::uint32_t>(x - glyph_left);
                    const auto destination_x = static_cast<std::uint32_t>(x - layer_bounds.left);
                    destination.at(destination_x, destination_y) =
                        std::max(destination.at(destination_x, destination_y),
                                 glyph.coverage[source_row + source_x]);
                }
            }
        }
    }
}

[[nodiscard]] CoveragePlane combine_max(const CoveragePlane& left, const CoveragePlane& right) {
    CoveragePlane result(left.width, left.height);
    for (std::size_t index = 0; index < result.values.size(); ++index) {
        result.values[index] = std::max(left.values[index], right.values[index]);
    }
    return result;
}

[[nodiscard]] CoveragePlane dilate(const CoveragePlane& source, const std::uint32_t radius) {
    if (radius == 0) {
        return source;
    }
    CoveragePlane horizontal(source.width, source.height);
    CoveragePlane output(source.width, source.height);
    for (std::uint32_t y = 0; y < source.height; ++y) {
        for (std::uint32_t x = 0; x < source.width; ++x) {
            const std::uint32_t begin = x > radius ? x - radius : 0;
            const std::uint32_t end = std::min(source.width - 1, x + radius);
            std::uint8_t maximum{};
            for (std::uint32_t sample = begin; sample <= end; ++sample) {
                maximum = std::max(maximum, source.at(sample, y));
            }
            horizontal.at(x, y) = maximum;
        }
    }
    for (std::uint32_t y = 0; y < source.height; ++y) {
        const std::uint32_t begin = y > radius ? y - radius : 0;
        const std::uint32_t end = std::min(source.height - 1, y + radius);
        for (std::uint32_t x = 0; x < source.width; ++x) {
            std::uint8_t maximum{};
            for (std::uint32_t sample = begin; sample <= end; ++sample) {
                maximum = std::max(maximum, horizontal.at(x, sample));
            }
            output.at(x, y) = maximum;
        }
    }
    return output;
}

[[nodiscard]] CoveragePlane box_blur(const CoveragePlane& source, const std::uint32_t radius) {
    if (radius == 0) {
        return source;
    }
    CoveragePlane horizontal(source.width, source.height);
    CoveragePlane output(source.width, source.height);
    const std::uint32_t diameter = radius * 2 + 1;
    for (std::uint32_t y = 0; y < source.height; ++y) {
        std::uint64_t sum{};
        for (std::int32_t x = -static_cast<std::int32_t>(radius);
             x <= static_cast<std::int32_t>(radius); ++x) {
            const auto clamped = static_cast<std::uint32_t>(std::clamp<std::int32_t>(
                x, 0, static_cast<std::int32_t>(source.width) - 1));
            sum += source.at(clamped, y);
        }
        for (std::uint32_t x = 0; x < source.width; ++x) {
            horizontal.at(x, y) = static_cast<std::uint8_t>((sum + diameter / 2) / diameter);
            const auto remove_x = static_cast<std::uint32_t>(std::clamp<std::int32_t>(
                static_cast<std::int32_t>(x) - static_cast<std::int32_t>(radius), 0,
                static_cast<std::int32_t>(source.width) - 1));
            const auto add_x = static_cast<std::uint32_t>(std::clamp<std::int32_t>(
                static_cast<std::int32_t>(x) + static_cast<std::int32_t>(radius) + 1, 0,
                static_cast<std::int32_t>(source.width) - 1));
            sum -= source.at(remove_x, y);
            sum += source.at(add_x, y);
        }
    }
    for (std::uint32_t x = 0; x < source.width; ++x) {
        std::uint64_t sum{};
        for (std::int32_t y = -static_cast<std::int32_t>(radius);
             y <= static_cast<std::int32_t>(radius); ++y) {
            const auto clamped = static_cast<std::uint32_t>(std::clamp<std::int32_t>(
                y, 0, static_cast<std::int32_t>(source.height) - 1));
            sum += horizontal.at(x, clamped);
        }
        for (std::uint32_t y = 0; y < source.height; ++y) {
            output.at(x, y) = static_cast<std::uint8_t>((sum + diameter / 2) / diameter);
            const auto remove_y = static_cast<std::uint32_t>(std::clamp<std::int32_t>(
                static_cast<std::int32_t>(y) - static_cast<std::int32_t>(radius), 0,
                static_cast<std::int32_t>(source.height) - 1));
            const auto add_y = static_cast<std::uint32_t>(std::clamp<std::int32_t>(
                static_cast<std::int32_t>(y) + static_cast<std::int32_t>(radius) + 1, 0,
                static_cast<std::int32_t>(source.height) - 1));
            sum -= horizontal.at(x, remove_y);
            sum += horizontal.at(x, add_y);
        }
    }
    return output;
}

[[nodiscard]] CoveragePlane shifted(const CoveragePlane& source,
                                    const std::int32_t dx,
                                    const std::int32_t dy) {
    CoveragePlane output(source.width, source.height);
    for (std::uint32_t y = 0; y < source.height; ++y) {
        const std::int32_t destination_y = static_cast<std::int32_t>(y) + dy;
        if (destination_y < 0 || destination_y >= static_cast<std::int32_t>(source.height)) {
            continue;
        }
        for (std::uint32_t x = 0; x < source.width; ++x) {
            const std::int32_t destination_x = static_cast<std::int32_t>(x) + dx;
            if (destination_x < 0 || destination_x >= static_cast<std::int32_t>(source.width)) {
                continue;
            }
            output.at(static_cast<std::uint32_t>(destination_x),
                      static_cast<std::uint32_t>(destination_y)) = source.at(x, y);
        }
    }
    return output;
}

[[nodiscard]] std::uint8_t rounded_rect_coverage(const float pixel_x,
                                                 const float pixel_y,
                                                 const RectF rectangle,
                                                 const float radius) noexcept {
    if (!rectangle.finite_positive()) {
        return 0;
    }
    const float bounded_radius = std::clamp(radius, 0.0F,
                                            std::min(rectangle.width, rectangle.height) * 0.5F);
    constexpr std::array<float, 4> samples{0.125F, 0.375F, 0.625F, 0.875F};
    std::uint32_t inside{};
    for (const float sy : samples) {
        for (const float sx : samples) {
            const float x = pixel_x + sx;
            const float y = pixel_y + sy;
            const float closest_x = std::clamp(x, rectangle.x + bounded_radius,
                                               rectangle.right() - bounded_radius);
            const float closest_y = std::clamp(y, rectangle.y + bounded_radius,
                                               rectangle.bottom() - bounded_radius);
            const float dx = x - closest_x;
            const float dy = y - closest_y;
            if (x >= rectangle.x && x <= rectangle.right() && y >= rectangle.y &&
                y <= rectangle.bottom() && dx * dx + dy * dy <= bounded_radius * bounded_radius) {
                ++inside;
            }
        }
    }
    return static_cast<std::uint8_t>((inside * 255U + 8U) / 16U);
}

void blend_pixel(std::uint8_t* destination,
                 const Color color,
                 const std::uint8_t coverage,
                 const float global_alpha) noexcept {
    const float effective = color.a * global_alpha * (static_cast<float>(coverage) / 255.0F);
    const auto alpha = static_cast<std::uint32_t>(
        std::clamp(std::lround(effective * 255.0F), 0L, 255L));
    if (alpha == 0) {
        return;
    }
    const auto channel = [alpha](const float value) {
        return static_cast<std::uint32_t>(
            std::clamp(std::lround(value * static_cast<float>(alpha)), 0L,
                       static_cast<long>(alpha)));
    };
    const std::uint32_t source_b = channel(color.b);
    const std::uint32_t source_g = channel(color.g);
    const std::uint32_t source_r = channel(color.r);
    const std::uint32_t inverse = 255U - alpha;
    destination[0] = static_cast<std::uint8_t>(
        std::min(255U, source_b + (static_cast<std::uint32_t>(destination[0]) * inverse + 127U) / 255U));
    destination[1] = static_cast<std::uint8_t>(
        std::min(255U, source_g + (static_cast<std::uint32_t>(destination[1]) * inverse + 127U) / 255U));
    destination[2] = static_cast<std::uint8_t>(
        std::min(255U, source_r + (static_cast<std::uint32_t>(destination[2]) * inverse + 127U) / 255U));
    destination[3] = static_cast<std::uint8_t>(
        std::min(255U, alpha + (static_cast<std::uint32_t>(destination[3]) * inverse + 127U) / 255U));
    destination[0] = std::min(destination[0], destination[3]);
    destination[1] = std::min(destination[1], destination[3]);
    destination[2] = std::min(destination[2], destination[3]);
}

void blend_plane(RenderedLayer& layer,
                 const CoveragePlane& plane,
                 const Color color,
                 const float global_alpha) {
    for (std::uint32_t y = 0; y < layer.height_px; ++y) {
        for (std::uint32_t x = 0; x < layer.width_px; ++x) {
            auto* pixel = layer.bgra_premultiplied.data() +
                          static_cast<std::size_t>(y) * layer.stride_bytes +
                          static_cast<std::size_t>(x) * 4;
            blend_pixel(pixel, color, plane.at(x, y), global_alpha);
        }
    }
}

void draw_backplate(RenderedLayer& layer,
                    const IntRect layer_bounds,
                    const RectF bounds,
                    const BackplateStyle& style,
                    const float global_alpha) {
    if (!style.enabled) {
        return;
    }
    const float border = std::min(style.border_width_px,
                                  std::min(bounds.width, bounds.height) * 0.5F);
    const RectF inner{bounds.x + border, bounds.y + border,
                      std::max(0.0F, bounds.width - border * 2.0F),
                      std::max(0.0F, bounds.height - border * 2.0F)};
    for (std::uint32_t y = 0; y < layer.height_px; ++y) {
        const float desktop_y = static_cast<float>(layer_bounds.top) + static_cast<float>(y);
        for (std::uint32_t x = 0; x < layer.width_px; ++x) {
            const float desktop_x = static_cast<float>(layer_bounds.left) + static_cast<float>(x);
            const auto outer = rounded_rect_coverage(desktop_x, desktop_y, bounds,
                                                      style.corner_radius_px);
            if (outer == 0) {
                continue;
            }
            const auto inner_coverage = border > 0.0F
                                            ? rounded_rect_coverage(
                                                  desktop_x, desktop_y, inner,
                                                  std::max(0.0F, style.corner_radius_px - border))
                                            : outer;
            auto* pixel = layer.bgra_premultiplied.data() +
                          static_cast<std::size_t>(y) * layer.stride_bytes +
                          static_cast<std::size_t>(x) * 4;
            if (border > 0.0F && outer > inner_coverage) {
                blend_pixel(pixel, style.border,
                            static_cast<std::uint8_t>(outer - inner_coverage), global_alpha);
            }
            blend_pixel(pixel, style.fill, inner_coverage, global_alpha);
        }
    }
}

[[nodiscard]] bool validate_glyphs(const RenderRequest& request,
                                   const RenderLimits& limits,
                                   RenderResult& failure) {
    std::uint64_t total_bytes{};
    std::uint64_t total_masks{};
    for (const auto& run : request.glyph_runs) {
        if (!run.clip_px.finite_positive()) {
            failure = fail(RenderErrorCode::malformed_glyph_mask,
                           "glyph run clip must be a finite positive physical-pixel rectangle");
            return false;
        }
        for (const auto& glyph : run.glyphs) {
            ++total_masks;
            if (total_masks > limits.max_glyph_masks || glyph.width_px == 0 || glyph.height_px == 0 ||
                glyph.stride_bytes < glyph.width_px ||
                glyph.height_px > std::numeric_limits<std::uint64_t>::max() / glyph.stride_bytes) {
                failure = fail(total_masks > limits.max_glyph_masks ? RenderErrorCode::resource_limit
                                                                    : RenderErrorCode::malformed_glyph_mask,
                               "glyph dimensions, stride, or count are invalid");
                return false;
            }
            const std::uint64_t required =
                static_cast<std::uint64_t>(glyph.stride_bytes) * glyph.height_px;
            if (required != glyph.coverage.size()) {
                failure = fail(RenderErrorCode::malformed_glyph_mask,
                               "glyph coverage byte count does not match stride times height");
                return false;
            }
            if (required > limits.max_glyph_coverage_bytes - total_bytes) {
                failure = fail(RenderErrorCode::resource_limit,
                               "glyph coverage exceeds the configured byte budget");
                return false;
            }
            total_bytes += required;
        }
    }
    return true;
}

} // namespace

CpuSubtitleRenderer::CpuSubtitleRenderer(RenderLimits limits) : limits_(limits) {}

RenderResult CpuSubtitleRenderer::render(const RenderRequest& request) const {
    if (request.protocol_version != protocol_version_v1) {
        return fail(RenderErrorCode::unsupported_protocol, "unsupported subtitle layer protocol version");
    }
    if (!request.viewport_px.finite_positive()) {
        return fail(RenderErrorCode::invalid_viewport, "viewport must be a finite positive rectangle");
    }
    if (!request.output_clip_px.finite_positive()) {
        return fail(RenderErrorCode::invalid_clip, "output clip must be a finite positive rectangle");
    }
    if (!request.layout.bounds_px.finite_positive() ||
        !request.layout.body_bounds_px.finite_positive() ||
        (request.layout.speaker_bounds_px && !request.layout.speaker_bounds_px->finite_positive())) {
        return fail(RenderErrorCode::invalid_layout, "layout rectangles must be finite and positive");
    }
    const bool has_speaker_run = std::any_of(
        request.glyph_runs.begin(), request.glyph_runs.end(),
        [](const ResolvedGlyphRun& run) { return run.role == TextRole::speaker; });
    if (has_speaker_run && !request.layout.speaker_bounds_px) {
        return fail(RenderErrorCode::invalid_layout,
                    "speaker glyph runs require a resolved speaker rectangle");
    }
    if (!finite_nonnegative(request.fallback.safe_margin_px) ||
        !finite_nonnegative(request.fallback.bottom_offset_px)) {
        return fail(RenderErrorCode::invalid_layout, "fallback geometry must be finite and nonnegative");
    }
    if (!style_valid(request.style, limits_)) {
        return fail(RenderErrorCode::invalid_style, "style contains invalid color or effect values");
    }
    RenderResult validation_failure;
    if (!validate_glyphs(request, limits_, validation_failure)) {
        return validation_failure;
    }

    const Placement placement = resolve_placement(request);
    const auto layer_bounds = compute_layer_bounds(request, placement);
    if (!layer_bounds) {
        return fail(RenderErrorCode::empty_visible_region,
                    "subtitle and output clip do not share a visible region");
    }
    const auto width = static_cast<std::uint32_t>(layer_bounds->width());
    const auto height = static_cast<std::uint32_t>(layer_bounds->height());
    if (width > limits_.max_layer_width_px || height > limits_.max_layer_height_px ||
        width > std::numeric_limits<std::uint64_t>::max() / 4U / height ||
        static_cast<std::uint64_t>(width) * height * 4U > limits_.max_output_bytes) {
        return fail(RenderErrorCode::resource_limit, "subtitle output layer exceeds configured limits");
    }

    RenderedLayer layer;
    layer.presentation_id = request.presentation_id;
    layer.capture_sequence = request.capture_sequence;
    layer.graphics_generation = request.graphics_generation;
    layer.desktop_x_px = layer_bounds->left;
    layer.desktop_y_px = layer_bounds->top;
    layer.width_px = width;
    layer.height_px = height;
    layer.stride_bytes = width * 4U;
    layer.used_bottom_center_fallback = placement.fallback;
    layer.bgra_premultiplied.assign(static_cast<std::size_t>(layer.stride_bytes) * height, 0);

    draw_backplate(layer, *layer_bounds, placement.layout.bounds_px, request.style.backplate,
                   request.style.global_alpha);

    CoveragePlane body(width, height);
    CoveragePlane speaker(width, height);
    merge_glyph_masks(request, placement, *layer_bounds, body, speaker);
    const CoveragePlane combined = combine_max(body, speaker);

    if (request.style.shadow.enabled) {
        CoveragePlane shadow = shifted(
            combined, static_cast<std::int32_t>(std::lround(request.style.shadow.offset_x_px)),
            static_cast<std::int32_t>(std::lround(request.style.shadow.offset_y_px)));
        const auto blur_radius = static_cast<std::uint32_t>(
            std::clamp(std::lround(request.style.shadow.blur_radius_px), 0L, 128L));
        // Two passes approximate a soft tent/Gaussian kernel while remaining exact across CPUs.
        shadow = box_blur(box_blur(shadow, blur_radius), blur_radius);
        blend_plane(layer, shadow, request.style.shadow.color, request.style.global_alpha);
    }
    if (request.style.outline.enabled) {
        const auto outline_radius = static_cast<std::uint32_t>(
            std::clamp(std::lround(request.style.outline.width_px), 0L, 128L));
        const CoveragePlane outline = dilate(combined, outline_radius);
        blend_plane(layer, outline, request.style.outline.color, request.style.global_alpha);
    }
    blend_plane(layer, body, request.style.body, request.style.global_alpha);
    blend_plane(layer, speaker, request.style.speaker, request.style.global_alpha);

    if (!has_safe_premultiplied_alpha(layer)) {
        return fail(RenderErrorCode::invalid_style,
                    "internal alpha-safety invariant failed during composition");
    }
    RenderResult result;
    result.layer = std::move(layer);
    return result;
}

bool CpuSubtitleRenderer::has_safe_premultiplied_alpha(const RenderedLayer& layer) noexcept {
    if (layer.stride_bytes < layer.width_px * 4U ||
        layer.bgra_premultiplied.size() !=
            static_cast<std::size_t>(layer.stride_bytes) * layer.height_px) {
        return false;
    }
    for (std::uint32_t y = 0; y < layer.height_px; ++y) {
        for (std::uint32_t x = 0; x < layer.width_px; ++x) {
            const auto* pixel = layer.bgra_premultiplied.data() +
                                static_cast<std::size_t>(y) * layer.stride_bytes +
                                static_cast<std::size_t>(x) * 4;
            if (pixel[0] > pixel[3] || pixel[1] > pixel[3] || pixel[2] > pixel[3]) {
                return false;
            }
        }
    }
    return true;
}

std::uint64_t deterministic_layer_hash(const RenderedLayer& layer) noexcept {
    constexpr std::uint64_t offset = 14695981039346656037ULL;
    constexpr std::uint64_t prime = 1099511628211ULL;
    std::uint64_t hash = offset;
    auto mix_byte = [&](const std::uint8_t byte) {
        hash ^= byte;
        hash *= prime;
    };
    auto mix_u64 = [&](const std::uint64_t value) {
        for (std::uint32_t shift = 0; shift < 64; shift += 8) {
            mix_byte(static_cast<std::uint8_t>((value >> shift) & 0xFFU));
        }
    };
    mix_u64(layer.protocol_version);
    mix_u64(layer.presentation_id);
    mix_u64(layer.capture_sequence);
    mix_u64(layer.graphics_generation);
    mix_u64(static_cast<std::uint32_t>(layer.desktop_x_px));
    mix_u64(static_cast<std::uint32_t>(layer.desktop_y_px));
    mix_u64(layer.width_px);
    mix_u64(layer.height_px);
    mix_u64(layer.stride_bytes);
    mix_byte(layer.used_bottom_center_fallback ? 1U : 0U);
    for (const auto byte : layer.bgra_premultiplied) {
        mix_byte(byte);
    }
    return hash;
}

} // namespace npc::subtitle
