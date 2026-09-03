#pragma once

#include "npc/subtitle_renderer/renderer.hpp"

#ifdef _WIN32

#include <d2d1_1.h>
#include <dwrite.h>
#include <wrl/client.h>

#include <optional>
#include <string>

namespace npc::subtitle::windows {

struct NativeGlyphRun {
    const DWRITE_GLYPH_RUN* glyph_run{};
    float baseline_origin_x_px{};
    float baseline_origin_y_px{};
    RectF clip_px;
    TextRole role{TextRole::body};
    DWRITE_MEASURING_MODE measuring_mode{DWRITE_MEASURING_MODE_NATURAL};
};

/// UTF-8 text contract for the Windows-native shaping boundary. Bounds and
/// sizes are physical pixels. DirectWrite owns Unicode bidi, script shaping,
/// font fallback, and grapheme-cluster-safe line breaking before coverage is
/// handed to the portable renderer.
struct TextRasterRequest {
    std::string body_utf8;
    std::optional<std::string> speaker_utf8;
    std::string locale{"und"};
    RectF body_bounds_px;
    std::optional<RectF> speaker_bounds_px;
    float body_size_px{28.0F};
    float speaker_size_px{18.0F};
    bool right_to_left{};
};

/// Windows adapter for the same portable core. DirectWrite turns a shaped
/// DWRITE_GLYPH_RUN into an 8-bit coverage mask; Direct2D receives the final
/// BGRA premultiplied layer as an ID2D1Bitmap1 ready for overlay composition.
class DirectWriteD2DBackend final {
public:
    explicit DirectWriteD2DBackend(IDWriteFactory* write_factory);

    [[nodiscard]] bool resolve(const NativeGlyphRun& source,
                               ResolvedGlyphRun& destination,
                               std::string& error) const;

    [[nodiscard]] bool resolve_text(const TextRasterRequest& source,
                                    std::vector<ResolvedGlyphRun>& destination,
                                    std::string& error) const;

    [[nodiscard]] bool create_bitmap(ID2D1DeviceContext* context,
                                     const RenderedLayer& layer,
                                     ID2D1Bitmap1** bitmap,
                                     std::string& error) const;

private:
    Microsoft::WRL::ComPtr<IDWriteFactory> write_factory_;
};

} // namespace npc::subtitle::windows

#endif
