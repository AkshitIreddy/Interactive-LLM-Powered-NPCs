#ifdef _WIN32

#include "npc/subtitle_renderer/windows_directwrite.hpp"

#include <dwrite.h>
#include <wrl/client.h>

#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <string>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;
using namespace npc::subtitle;
using namespace npc::subtitle::windows;

int failures{};

#define CHECK(expression)                                                                       \
    do {                                                                                        \
        if (!(expression)) {                                                                    \
            std::cerr << __FILE__ << ':' << __LINE__ << ": CHECK failed: " #expression << '\n'; \
            ++failures;                                                                         \
        }                                                                                       \
    } while (false)

[[nodiscard]] std::uint64_t coverage_sum(const std::vector<ResolvedGlyphRun>& runs) {
    std::uint64_t sum{};
    for (const auto& run : runs) {
        for (const auto& glyph : run.glyphs) {
            for (const auto coverage : glyph.coverage) {
                sum += coverage;
            }
        }
    }
    return sum;
}

void shapes_ltr_and_rtl_text_to_physical_pixel_masks() {
    ComPtr<IDWriteFactory> factory;
    const auto created = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED, __uuidof(IDWriteFactory),
                                             &factory);
    CHECK(SUCCEEDED(created));
    if (FAILED(created)) {
        return;
    }
    DirectWriteD2DBackend backend(factory.Get());
    std::vector<ResolvedGlyphRun> runs;
    std::string error;

    TextRasterRequest ltr;
    ltr.body_utf8 = "Mixed DPI subtitle â ready";
    ltr.speaker_utf8 = "Mara";
    ltr.locale = "en-US";
    ltr.body_bounds_px = {-1420.0F, 760.0F, 620.0F, 90.0F};
    ltr.speaker_bounds_px = RectF{-1420.0F, 720.0F, 300.0F, 34.0F};
    ltr.body_size_px = 31.5F;
    ltr.speaker_size_px = 20.0F;
    CHECK(backend.resolve_text(ltr, runs, error));
    if (!error.empty()) {
        std::cerr << "LTR shaping warning: " << error << '\n';
    }
    CHECK(error.empty());
    CHECK(runs.size() >= 2);
    CHECK(coverage_sum(runs) > 0);
    bool has_body{};
    bool has_speaker{};
    for (const auto& run : runs) {
        has_body = has_body || run.role == TextRole::body;
        has_speaker = has_speaker || run.role == TextRole::speaker;
    }
    CHECK(has_body && has_speaker);

    TextRasterRequest rtl;
    rtl.body_utf8 = "ÙØ±Ø­Ø¨Ø§ Ø¨Ù ÙÙ Ø§ÙÙØ¯ÙÙØ©";
    rtl.locale = "ar-SA";
    rtl.body_bounds_px = {120.0F, 600.0F, 720.0F, 110.0F};
    rtl.body_size_px = 36.0F;
    rtl.right_to_left = true;
    CHECK(backend.resolve_text(rtl, runs, error));
    if (!error.empty()) {
        std::cerr << "RTL shaping warning: " << error << '\n';
    }
    CHECK(error.empty());
    CHECK(!runs.empty());
    CHECK(coverage_sum(runs) > 0);
    for (const auto& run : runs) {
        CHECK(run.clip_px.x == rtl.body_bounds_px.x);
        CHECK(run.clip_px.width == rtl.body_bounds_px.width);
    }
}

void invalid_utf8_and_unbounded_sizes_fail_closed() {
    ComPtr<IDWriteFactory> factory;
    CHECK(SUCCEEDED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED, __uuidof(IDWriteFactory),
                                        &factory)));
    DirectWriteD2DBackend backend(factory.Get());
    std::vector<ResolvedGlyphRun> runs;
    std::string error;
    TextRasterRequest request;
    request.body_utf8 = std::string("\xC3\x28", 2);
    request.body_bounds_px = {0.0F, 0.0F, 400.0F, 100.0F};
    CHECK(!backend.resolve_text(request, runs, error));
    CHECK(!error.empty());
    CHECK(runs.empty());

    request.body_utf8 = "valid";
    request.body_size_px = 1000.0F;
    CHECK(!backend.resolve_text(request, runs, error));
    CHECK(!error.empty());
    CHECK(runs.empty());
}

} // namespace

int main() {
    shapes_ltr_and_rtl_text_to_physical_pixel_masks();
    invalid_utf8_and_unbounded_sizes_fail_closed();
    if (failures != 0) {
        std::cerr << failures << " DirectWrite shaping assertion(s) failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "DirectWrite subtitle shaping tests passed\n";
    return EXIT_SUCCESS;
}

#endif
