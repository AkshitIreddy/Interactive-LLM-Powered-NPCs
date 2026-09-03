#pragma once

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <optional>
#include <string>
#include <vector>

namespace npc::subtitle {

inline constexpr std::uint32_t protocol_version_v1 = 1;

struct PointF {
    float x{};
    float y{};
};

struct RectF {
    float x{};
    float y{};
    float width{};
    float height{};

    [[nodiscard]] float right() const noexcept { return x + width; }
    [[nodiscard]] float bottom() const noexcept { return y + height; }
    [[nodiscard]] bool finite_positive() const noexcept {
        return std::isfinite(x) && std::isfinite(y) && std::isfinite(width) &&
               std::isfinite(height) && width > 0.0F && height > 0.0F;
    }
};

struct Color {
    float r{};
    float g{};
    float b{};
    float a{};

    [[nodiscard]] bool valid() const noexcept {
        return std::isfinite(r) && std::isfinite(g) && std::isfinite(b) && std::isfinite(a) &&
               r >= 0.0F && r <= 1.0F && g >= 0.0F && g <= 1.0F && b >= 0.0F &&
               b <= 1.0F && a >= 0.0F && a <= 1.0F;
    }
};

enum class TextRole : std::uint8_t { body = 0, speaker = 1 };
enum class AnchorKind : std::uint8_t {
    head_above = 0,
    head_below = 1,
    head_left = 2,
    head_right = 3,
    bottom_center = 4,
};

/// One shaped glyph or a whole platform-rasterized glyph run. Coverage is one
/// byte per pixel, row-major, with an explicit stride. The desktop origin is a
/// physical-pixel coordinate; the renderer never performs DPI conversion.
struct GlyphMask {
    std::int32_t origin_x_px{};
    std::int32_t origin_y_px{};
    std::uint32_t width_px{};
    std::uint32_t height_px{};
    std::uint32_t stride_bytes{};
    std::vector<std::uint8_t> coverage;
};

struct ResolvedGlyphRun {
    TextRole role{TextRole::body};
    RectF clip_px;
    std::vector<GlyphMask> glyphs;
};

/// Mirrors the physical-pixel output of npc-subtitle-engine. The optional
/// speaker rectangle must be present whenever a speaker glyph run is supplied.
struct ResolvedLayout {
    RectF bounds_px;
    std::optional<RectF> speaker_bounds_px;
    RectF body_bounds_px;
    AnchorKind anchor{AnchorKind::bottom_center};
};

struct OutlineStyle {
    bool enabled{true};
    float width_px{2.0F};
    Color color{0.0F, 0.0F, 0.0F, 0.9F};
};

struct ShadowStyle {
    bool enabled{true};
    float offset_x_px{1.0F};
    float offset_y_px{2.0F};
    float blur_radius_px{3.0F};
    Color color{0.0F, 0.0F, 0.0F, 0.65F};
};

struct BackplateStyle {
    bool enabled{true};
    float corner_radius_px{10.0F};
    float border_width_px{1.0F};
    Color fill{0.02F, 0.025F, 0.04F, 0.82F};
    Color border{1.0F, 1.0F, 1.0F, 0.14F};
};

struct ResolvedStyle {
    Color body{1.0F, 1.0F, 1.0F, 1.0F};
    Color speaker{0.36F, 0.84F, 1.0F, 1.0F};
    OutlineStyle outline;
    ShadowStyle shadow;
    BackplateStyle backplate;
    float global_alpha{1.0F};
};

struct BottomCenterFallback {
    bool enabled{true};
    float safe_margin_px{24.0F};
    float bottom_offset_px{54.0F};
};

/// Defensive limits are part of the protocol. A caller must split or reject
/// work that exceeds them; the renderer never attempts an unbounded allocation.
struct RenderLimits {
    std::uint32_t max_layer_width_px{4096};
    std::uint32_t max_layer_height_px{2160};
    std::uint32_t max_glyph_masks{2048};
    std::uint64_t max_glyph_coverage_bytes{32ULL * 1024ULL * 1024ULL};
    std::uint64_t max_output_bytes{64ULL * 1024ULL * 1024ULL};
    float max_effect_extent_px{128.0F};
};

struct RenderRequest {
    std::uint32_t protocol_version{protocol_version_v1};
    std::uint64_t presentation_id{};
    std::uint64_t capture_sequence{};
    std::uint64_t graphics_generation{};
    RectF viewport_px;
    RectF output_clip_px;
    ResolvedLayout layout;
    ResolvedStyle style;
    BottomCenterFallback fallback;
    std::vector<ResolvedGlyphRun> glyph_runs;
};

/// A tightly-cropped physical-pixel layer. Pixels are BGRA8, sRGB color values
/// with premultiplied alpha. `stride_bytes` is always width * 4 in v1.
struct RenderedLayer {
    std::uint32_t protocol_version{protocol_version_v1};
    std::uint64_t presentation_id{};
    std::uint64_t capture_sequence{};
    std::uint64_t graphics_generation{};
    std::int32_t desktop_x_px{};
    std::int32_t desktop_y_px{};
    std::uint32_t width_px{};
    std::uint32_t height_px{};
    std::uint32_t stride_bytes{};
    bool used_bottom_center_fallback{};
    std::vector<std::uint8_t> bgra_premultiplied;

    [[nodiscard]] bool empty() const noexcept {
        return width_px == 0 || height_px == 0 || bgra_premultiplied.empty();
    }
};

enum class RenderErrorCode : std::uint8_t {
    none = 0,
    unsupported_protocol,
    invalid_viewport,
    invalid_clip,
    invalid_layout,
    invalid_style,
    malformed_glyph_mask,
    resource_limit,
    empty_visible_region,
};

struct RenderError {
    RenderErrorCode code{RenderErrorCode::none};
    std::string message;
};

struct RenderResult {
    std::optional<RenderedLayer> layer;
    std::optional<RenderError> error;

    [[nodiscard]] explicit operator bool() const noexcept { return layer.has_value(); }
};

} // namespace npc::subtitle
