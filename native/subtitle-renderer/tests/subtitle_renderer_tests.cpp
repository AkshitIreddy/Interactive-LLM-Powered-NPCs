#include "npc/subtitle_renderer/renderer.hpp"

#include <array>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <string_view>
#include <utility>

namespace {

using npc::subtitle::CpuSubtitleRenderer;
using npc::subtitle::GlyphMask;
using npc::subtitle::RectF;
using npc::subtitle::RenderErrorCode;
using npc::subtitle::RenderRequest;
using npc::subtitle::ResolvedGlyphRun;
using npc::subtitle::TextRole;

int failures{};

#define CHECK(expression)                                                                       \
    do {                                                                                        \
        if (!(expression)) {                                                                    \
            std::cerr << __FILE__ << ':' << __LINE__ << ": CHECK failed: " #expression << '\n'; \
            ++failures;                                                                         \
        }                                                                                       \
    } while (false)

using Pattern = std::array<std::string_view, 7>;

[[nodiscard]] Pattern pattern_for(const char character) {
    switch (character) {
    case 'A': return {"01110", "10001", "10001", "11111", "10001", "10001", "10001"};
    case 'B': return {"11110", "10001", "10001", "11110", "10001", "10001", "11110"};
    case 'D': return {"11110", "10001", "10001", "10001", "10001", "10001", "11110"};
    case 'E': return {"11111", "10000", "10000", "11110", "10000", "10000", "11111"};
    case 'F': return {"11111", "10000", "10000", "11110", "10000", "10000", "10000"};
    case 'I': return {"11111", "00100", "00100", "00100", "00100", "00100", "11111"};
    case 'L': return {"10000", "10000", "10000", "10000", "10000", "10000", "11111"};
    case 'N': return {"10001", "11001", "11001", "10101", "10011", "10011", "10001"};
    case 'O': return {"01110", "10001", "10001", "10001", "10001", "10001", "01110"};
    case 'R': return {"11110", "10001", "10001", "11110", "10100", "10010", "10001"};
    case 'S': return {"01111", "10000", "10000", "01110", "00001", "00001", "11110"};
    case 'T': return {"11111", "00100", "00100", "00100", "00100", "00100", "00100"};
    case 'V': return {"10001", "10001", "10001", "10001", "10001", "01010", "00100"};
    default: return {"00000", "00000", "00000", "00000", "00000", "00000", "00000"};
    }
}

[[nodiscard]] ResolvedGlyphRun block_run(const TextRole role,
                                         const std::string_view text,
                                         const std::int32_t x,
                                         const std::int32_t y,
                                         const std::uint32_t scale,
                                         const RectF clip) {
    ResolvedGlyphRun run;
    run.role = role;
    run.clip_px = clip;
    std::int32_t cursor = x;
    for (const char character : text) {
        if (character == ' ') {
            cursor += static_cast<std::int32_t>(4U * scale);
            continue;
        }
        const auto pattern = pattern_for(character);
        GlyphMask glyph;
        glyph.origin_x_px = cursor;
        glyph.origin_y_px = y;
        glyph.width_px = 5U * scale;
        glyph.height_px = 7U * scale;
        glyph.stride_bytes = glyph.width_px;
        glyph.coverage.resize(static_cast<std::size_t>(glyph.width_px) * glyph.height_px);
        for (std::uint32_t row = 0; row < 7; ++row) {
            for (std::uint32_t column = 0; column < 5; ++column) {
                const std::uint8_t coverage = pattern[row][column] == '1' ? 255U : 0U;
                for (std::uint32_t dy = 0; dy < scale; ++dy) {
                    for (std::uint32_t dx = 0; dx < scale; ++dx) {
                        const auto px = column * scale + dx;
                        const auto py = row * scale + dy;
                        glyph.coverage[static_cast<std::size_t>(py) * glyph.stride_bytes + px] =
                            coverage;
                    }
                }
            }
        }
        run.glyphs.push_back(std::move(glyph));
        cursor += static_cast<std::int32_t>(6U * scale);
    }
    return run;
}

[[nodiscard]] RenderRequest standard_request() {
    RenderRequest request;
    request.presentation_id = 0xA11CE;
    request.capture_sequence = 701;
    request.graphics_generation = 9;
    request.viewport_px = {-1920.0F, 0.0F, 1920.0F, 1080.0F};
    request.output_clip_px = request.viewport_px;
    request.layout.bounds_px = {-1450.0F, 810.0F, 620.0F, 180.0F};
    request.layout.speaker_bounds_px = RectF{-1416.0F, 838.0F, 180.0F, 30.0F};
    request.layout.body_bounds_px = {-1416.0F, 882.0F, 550.0F, 72.0F};
    request.glyph_runs.push_back(block_run(TextRole::speaker, "NOVA", -1416, 838, 3,
                                           *request.layout.speaker_bounds_px));
    request.glyph_runs.push_back(block_run(TextRole::body, "SAFE FIRST", -1416, 884, 4,
                                           request.layout.body_bounds_px));
    return request;
}

[[nodiscard]] std::size_t visible_pixels(const npc::subtitle::RenderedLayer& layer) {
    std::size_t count{};
    for (std::size_t index = 3; index < layer.bgra_premultiplied.size(); index += 4) {
        count += layer.bgra_premultiplied[index] != 0 ? 1U : 0U;
    }
    return count;
}

void renders_deterministic_layer_with_all_effects() {
    CpuSubtitleRenderer renderer;
    const auto first = renderer.render(standard_request());
    const auto second = renderer.render(standard_request());
    CHECK(first);
    CHECK(second);
    if (!first || !second) {
        return;
    }
    CHECK(first.layer->desktop_x_px < -1450);
    CHECK(first.layer->width_px > 620);
    CHECK(first.layer->height_px > 180);
    CHECK(visible_pixels(*first.layer) > 50'000);
    CHECK(CpuSubtitleRenderer::has_safe_premultiplied_alpha(*first.layer));
    const auto hash = npc::subtitle::deterministic_layer_hash(*first.layer);
    CHECK(hash == npc::subtitle::deterministic_layer_hash(*second.layer));
    // Golden value pins fill/outline/shadow/backplate order and integer alpha math.
    constexpr std::uint64_t expected_hash = 7813606705990076709ULL;
    CHECK(hash == expected_hash);
}

void falls_back_to_bottom_center_and_translates_glyphs() {
    auto request = standard_request();
    request.viewport_px = {100.0F, 200.0F, 1000.0F, 600.0F};
    request.output_clip_px = request.viewport_px;
    request.layout.bounds_px = {-5000.0F, -5000.0F, 620.0F, 180.0F};
    request.layout.speaker_bounds_px = RectF{-4966.0F, -4972.0F, 180.0F, 30.0F};
    request.layout.body_bounds_px = {-4966.0F, -4928.0F, 550.0F, 72.0F};
    request.glyph_runs.clear();
    request.glyph_runs.push_back(block_run(TextRole::speaker, "NOVA", -4966, -4972, 3,
                                           *request.layout.speaker_bounds_px));
    request.glyph_runs.push_back(block_run(TextRole::body, "SAFE", -4966, -4926, 4,
                                           request.layout.body_bounds_px));
    const auto result = CpuSubtitleRenderer{}.render(request);
    CHECK(result);
    if (!result) {
        return;
    }
    CHECK(result.layer->used_bottom_center_fallback);
    CHECK(result.layer->desktop_x_px >= 100);
    CHECK(result.layer->desktop_y_px >= 200);
    CHECK(result.layer->desktop_x_px + static_cast<std::int32_t>(result.layer->width_px) <= 1100);
    CHECK(result.layer->desktop_y_px + static_cast<std::int32_t>(result.layer->height_px) <= 800);
    CHECK(visible_pixels(*result.layer) > 10'000);
}

void clips_every_effect_to_output_rectangle() {
    auto request = standard_request();
    request.output_clip_px = {-1200.0F, 850.0F, 160.0F, 90.0F};
    const auto result = CpuSubtitleRenderer{}.render(request);
    CHECK(result);
    if (!result) {
        return;
    }
    CHECK(result.layer->desktop_x_px >= -1200);
    CHECK(result.layer->desktop_y_px >= 850);
    CHECK(result.layer->width_px <= 160);
    CHECK(result.layer->height_px <= 90);
    CHECK(CpuSubtitleRenderer::has_safe_premultiplied_alpha(*result.layer));
}

void speaker_and_body_use_distinct_colors() {
    auto request = standard_request();
    request.style.backplate.enabled = false;
    request.style.shadow.enabled = false;
    request.style.outline.enabled = false;
    request.style.body = {1.0F, 0.0F, 0.0F, 1.0F};
    request.style.speaker = {0.0F, 1.0F, 0.0F, 1.0F};
    const auto result = CpuSubtitleRenderer{}.render(request);
    CHECK(result);
    if (!result) {
        return;
    }
    bool found_red{};
    bool found_green{};
    for (std::size_t index = 0; index < result.layer->bgra_premultiplied.size(); index += 4) {
        const auto* pixel = result.layer->bgra_premultiplied.data() + index;
        found_red = found_red || (pixel[2] > 200 && pixel[1] == 0);
        found_green = found_green || (pixel[1] > 200 && pixel[2] == 0);
    }
    CHECK(found_red);
    CHECK(found_green);
}

void malformed_and_unbounded_requests_fail_closed() {
    auto malformed = standard_request();
    malformed.glyph_runs.front().glyphs.front().stride_bytes = 1;
    const auto malformed_result = CpuSubtitleRenderer{}.render(malformed);
    CHECK(!malformed_result);
    CHECK(malformed_result.error.has_value());
    CHECK(malformed_result.error->code == RenderErrorCode::malformed_glyph_mask);

    auto wrong_version = standard_request();
    wrong_version.protocol_version = 99;
    const auto version_result = CpuSubtitleRenderer{}.render(wrong_version);
    CHECK(!version_result);
    CHECK(version_result.error->code == RenderErrorCode::unsupported_protocol);

    auto invalid_color = standard_request();
    invalid_color.style.body.r = 2.0F;
    const auto color_result = CpuSubtitleRenderer{}.render(invalid_color);
    CHECK(!color_result);
    CHECK(color_result.error->code == RenderErrorCode::invalid_style);

    auto limited = standard_request();
    npc::subtitle::RenderLimits limits;
    limits.max_output_bytes = 1024;
    const auto limited_result = CpuSubtitleRenderer{limits}.render(limited);
    CHECK(!limited_result);
    CHECK(limited_result.error->code == RenderErrorCode::resource_limit);

    auto missing_speaker_bounds = standard_request();
    missing_speaker_bounds.layout.speaker_bounds_px.reset();
    const auto speaker_result = CpuSubtitleRenderer{}.render(missing_speaker_bounds);
    CHECK(!speaker_result);
    CHECK(speaker_result.error->code == RenderErrorCode::invalid_layout);
}

void role_bounds_clip_coverage_even_when_a_mask_is_larger() {
    auto request = standard_request();
    request.glyph_runs.clear();
    request.layout.speaker_bounds_px.reset();
    request.layout.bounds_px = {-1500.0F, 800.0F, 100.0F, 100.0F};
    request.layout.body_bounds_px = {-1470.0F, 830.0F, 10.0F, 10.0F};
    request.style.backplate.enabled = false;
    request.style.shadow.enabled = false;
    request.style.outline.enabled = false;
    request.glyph_runs.push_back(block_run(TextRole::body, "A", -1480, 820, 4,
                                           {-1500.0F, 800.0F, 100.0F, 100.0F}));
    const auto result = CpuSubtitleRenderer{}.render(request);
    CHECK(result);
    if (!result) {
        return;
    }
    bool saw_coverage{};
    for (std::uint32_t y = 0; y < result.layer->height_px; ++y) {
        for (std::uint32_t x = 0; x < result.layer->width_px; ++x) {
            const auto alpha = result.layer->bgra_premultiplied[
                static_cast<std::size_t>(y) * result.layer->stride_bytes + x * 4U + 3U];
            if (alpha == 0) {
                continue;
            }
            saw_coverage = true;
            const auto desktop_x = result.layer->desktop_x_px + static_cast<std::int32_t>(x);
            const auto desktop_y = result.layer->desktop_y_px + static_cast<std::int32_t>(y);
            CHECK(desktop_x >= -1470 && desktop_x < -1460);
            CHECK(desktop_y >= 830 && desktop_y < 840);
        }
    }
    CHECK(saw_coverage);
}

void empty_intersection_is_typed_not_silent() {
    auto request = standard_request();
    request.fallback.enabled = false;
    request.layout.bounds_px = {5000.0F, 5000.0F, 300.0F, 100.0F};
    request.layout.speaker_bounds_px = RectF{5010.0F, 5010.0F, 100.0F, 20.0F};
    request.layout.body_bounds_px = {5010.0F, 5040.0F, 260.0F, 40.0F};
    const auto result = CpuSubtitleRenderer{}.render(request);
    CHECK(!result);
    CHECK(result.error.has_value());
    CHECK(result.error->code == RenderErrorCode::empty_visible_region);
}

} // namespace

int main() {
    renders_deterministic_layer_with_all_effects();
    falls_back_to_bottom_center_and_translates_glyphs();
    clips_every_effect_to_output_rectangle();
    speaker_and_body_use_distinct_colors();
    malformed_and_unbounded_requests_fail_closed();
    role_bounds_clip_coverage_even_when_a_mask_is_larger();
    empty_intersection_is_typed_not_silent();
    if (failures != 0) {
        std::cerr << failures << " subtitle renderer assertion(s) failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "subtitle renderer tests passed\n";
    return EXIT_SUCCESS;
}
