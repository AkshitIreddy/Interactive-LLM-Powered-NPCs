#ifdef _WIN32

#include "npc/subtitle_renderer/windows_directwrite.hpp"

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <limits>
#include <string>
#include <vector>

#include <dxgiformat.h>
#include <Windows.h>

namespace npc::subtitle::windows {
namespace {

[[nodiscard]] std::string hresult_message(const char* operation, const HRESULT value) {
    return std::string(operation) + " failed (HRESULT=" +
           std::to_string(static_cast<long>(value)) + ")";
}

[[nodiscard]] bool utf8_to_wide(const std::string& source,
                                std::wstring& destination,
                                std::string& error) {
    destination.clear();
    if (source.empty()) {
        error = "subtitle text must not be empty";
        return false;
    }
    if (source.size() > 16U * 1024U) {
        error = "subtitle text exceeds the native shaping byte limit";
        return false;
    }
    const auto byte_count = static_cast<int>(source.size());
    const int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, source.data(),
                                           byte_count, nullptr, 0);
    if (length <= 0) {
        error = "subtitle text is not valid UTF-8";
        return false;
    }
    destination.resize(static_cast<std::size_t>(length));
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, source.data(), byte_count,
                            destination.data(), length) != length) {
        error = "subtitle UTF-8 conversion failed";
        destination.clear();
        return false;
    }
    return true;
}

class GlyphCollector final : public IDWriteTextRenderer {
public:
    GlyphCollector(const DirectWriteD2DBackend& backend,
                   const TextRole role,
                   const RectF clip,
                   std::vector<ResolvedGlyphRun>& destination,
                   std::string& error)
        : backend_(backend), role_(role), clip_(clip), destination_(destination), error_(error) {}

    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID identifier, void** object) override {
        if (!object) {
            return E_POINTER;
        }
        *object = nullptr;
        if (identifier == __uuidof(IUnknown) || identifier == __uuidof(IDWritePixelSnapping) ||
            identifier == __uuidof(IDWriteTextRenderer)) {
            *object = static_cast<IDWriteTextRenderer*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }

    ULONG STDMETHODCALLTYPE AddRef() override { return ++references_; }

    ULONG STDMETHODCALLTYPE Release() override {
        const auto remaining = --references_;
        if (remaining == 0) {
            delete this;
        }
        return remaining;
    }

    HRESULT STDMETHODCALLTYPE IsPixelSnappingDisabled(void*, BOOL* disabled) override {
        if (!disabled) {
            return E_POINTER;
        }
        *disabled = FALSE;
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE GetCurrentTransform(void*, DWRITE_MATRIX* transform) override {
        if (!transform) {
            return E_POINTER;
        }
        *transform = DWRITE_MATRIX{1.0F, 0.0F, 0.0F, 1.0F, 0.0F, 0.0F};
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE GetPixelsPerDip(void*, FLOAT* pixels_per_dip) override {
        if (!pixels_per_dip) {
            return E_POINTER;
        }
        // Inputs are physical pixels, so one DirectWrite layout unit maps to
        // exactly one output pixel at this boundary.
        *pixels_per_dip = 1.0F;
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE DrawGlyphRun(void*,
                                           FLOAT baseline_origin_x,
                                           FLOAT baseline_origin_y,
                                           DWRITE_MEASURING_MODE measuring_mode,
                                           const DWRITE_GLYPH_RUN* glyph_run,
                                           const DWRITE_GLYPH_RUN_DESCRIPTION*,
                                           IUnknown*) override {
        if (!glyph_run || !glyph_run->fontFace) {
            error_ = "DirectWrite supplied an invalid subtitle glyph run";
            return E_FAIL;
        }
        // DirectWrite may issue an empty run at a bidi/control boundary. It has
        // no coverage and is not an error or a presentation receipt.
        if (glyph_run->glyphCount == 0) {
            return S_OK;
        }
        ResolvedGlyphRun resolved;
        const NativeGlyphRun native{glyph_run, baseline_origin_x, baseline_origin_y, clip_, role_,
                                    measuring_mode};
        if (!backend_.resolve(native, resolved, error_)) {
            return E_FAIL;
        }
        if (!resolved.glyphs.empty()) {
            destination_.push_back(std::move(resolved));
        }
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE DrawUnderline(void*, FLOAT, FLOAT, const DWRITE_UNDERLINE*,
                                            IUnknown*) override {
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE DrawStrikethrough(void*, FLOAT, FLOAT,
                                                const DWRITE_STRIKETHROUGH*, IUnknown*) override {
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE DrawInlineObject(void*, FLOAT, FLOAT, IDWriteInlineObject*, BOOL,
                                               BOOL, IUnknown*) override {
        error_ = "inline subtitle objects are outside the v1 shaping contract";
        return E_NOTIMPL;
    }

private:
    std::atomic<ULONG> references_{1};
    const DirectWriteD2DBackend& backend_;
    TextRole role_;
    RectF clip_;
    std::vector<ResolvedGlyphRun>& destination_;
    std::string& error_;
};

[[nodiscard]] bool create_text_format(IDWriteFactory* factory,
                                      const float size_px,
                                      const std::wstring& locale,
                                      IDWriteTextFormat** format,
                                      std::string& error) {
    if (!factory || !format || !std::isfinite(size_px) || size_px < 6.0F || size_px > 256.0F) {
        error = "subtitle text format parameters are invalid";
        return false;
    }
    *format = nullptr;
    HRESULT result = factory->CreateTextFormat(
        L"Segoe UI Variable Text", nullptr, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL, size_px, locale.c_str(), format);
    if (FAILED(result)) {
        result = factory->CreateTextFormat(
            L"Segoe UI", nullptr, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL, size_px, locale.c_str(), format);
    }
    if (FAILED(result)) {
        error = hresult_message("CreateTextFormat", result);
        return false;
    }
    return true;
}

[[nodiscard]] bool resolve_text_role(const DirectWriteD2DBackend& backend,
                                     IDWriteFactory* factory,
                                     const std::wstring& text,
                                     const std::wstring& locale,
                                     const RectF bounds,
                                     const float size_px,
                                     const bool right_to_left,
                                     const TextRole role,
                                     std::vector<ResolvedGlyphRun>& destination,
                                     std::string& error) {
    Microsoft::WRL::ComPtr<IDWriteTextFormat> format;
    if (!create_text_format(factory, size_px, locale, &format, error)) {
        return false;
    }
    format->SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
    format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
    format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
    format->SetReadingDirection(right_to_left ? DWRITE_READING_DIRECTION_RIGHT_TO_LEFT
                                              : DWRITE_READING_DIRECTION_LEFT_TO_RIGHT);
    format->SetFlowDirection(DWRITE_FLOW_DIRECTION_TOP_TO_BOTTOM);

    Microsoft::WRL::ComPtr<IDWriteTextLayout> layout;
    const HRESULT create_result = factory->CreateTextLayout(
        text.data(), static_cast<UINT32>(text.size()), format.Get(), bounds.width, bounds.height,
        &layout);
    if (FAILED(create_result)) {
        error = hresult_message("CreateTextLayout", create_result);
        return false;
    }
    auto* raw_collector = new GlyphCollector(backend, role, bounds, destination, error);
    Microsoft::WRL::ComPtr<IDWriteTextRenderer> collector;
    collector.Attach(raw_collector);
    const HRESULT draw_result = layout->Draw(nullptr, collector.Get(), bounds.x, bounds.y);
    if (FAILED(draw_result)) {
        if (error.empty()) {
            error = hresult_message("IDWriteTextLayout::Draw", draw_result);
        }
        return false;
    }
    return true;
}

} // namespace

DirectWriteD2DBackend::DirectWriteD2DBackend(IDWriteFactory* write_factory)
    : write_factory_(write_factory) {}

bool DirectWriteD2DBackend::resolve(const NativeGlyphRun& source,
                                    ResolvedGlyphRun& destination,
                                    std::string& error) const {
    destination = {};
    if (!write_factory_ || !source.glyph_run || !source.glyph_run->fontFace ||
        source.glyph_run->glyphCount == 0 || !source.clip_px.finite_positive()) {
        error = "DirectWrite glyph run, factory, or physical-pixel clip is unavailable";
        return false;
    }
    Microsoft::WRL::ComPtr<IDWriteGlyphRunAnalysis> analysis;
    const HRESULT create_result = write_factory_->CreateGlyphRunAnalysis(
        source.glyph_run, 1.0F, nullptr, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
        source.measuring_mode, source.baseline_origin_x_px, source.baseline_origin_y_px, &analysis);
    if (FAILED(create_result)) {
        error = hresult_message("CreateGlyphRunAnalysis", create_result);
        return false;
    }

    RECT bounds{};
    const HRESULT bounds_result =
        analysis->GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds);
    if (FAILED(bounds_result)) {
        error = hresult_message("GetAlphaTextureBounds", bounds_result);
        return false;
    }
    const LONG width = bounds.right - bounds.left;
    const LONG height = bounds.bottom - bounds.top;
    destination.role = source.role;
    destination.clip_px = source.clip_px;
    if (width <= 0 || height <= 0) {
        return true;
    }
    const std::uint64_t texel_count = static_cast<std::uint64_t>(width) *
                                      static_cast<std::uint64_t>(height);
    if (texel_count > std::numeric_limits<std::uint32_t>::max() / 3U) {
        error = "DirectWrite glyph alpha texture exceeds the v1 byte limit";
        return false;
    }
    std::vector<std::uint8_t> clear_type(static_cast<std::size_t>(texel_count) * 3U);
    const HRESULT texture_result = analysis->CreateAlphaTexture(
        DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, clear_type.data(),
        static_cast<UINT32>(clear_type.size()));
    if (FAILED(texture_result)) {
        error = hresult_message("CreateAlphaTexture", texture_result);
        return false;
    }

    GlyphMask mask;
    mask.origin_x_px = bounds.left;
    mask.origin_y_px = bounds.top;
    mask.width_px = static_cast<std::uint32_t>(width);
    mask.height_px = static_cast<std::uint32_t>(height);
    mask.stride_bytes = mask.width_px;
    mask.coverage.resize(static_cast<std::size_t>(texel_count));
    for (std::size_t index = 0; index < mask.coverage.size(); ++index) {
        const std::uint32_t red = clear_type[index * 3U];
        const std::uint32_t green = clear_type[index * 3U + 1U];
        const std::uint32_t blue = clear_type[index * 3U + 2U];
        // Transparent overlays cannot use LCD subpixel color safely. Collapse
        // DirectWrite ClearType coverage to deterministic grayscale alpha.
        mask.coverage[index] = static_cast<std::uint8_t>((red + green + blue + 1U) / 3U);
    }
    destination.glyphs.push_back(std::move(mask));
    return true;
}

bool DirectWriteD2DBackend::resolve_text(const TextRasterRequest& source,
                                        std::vector<ResolvedGlyphRun>& destination,
                                        std::string& error) const {
    destination.clear();
    error.clear();
    if (!write_factory_ || source.body_utf8.empty() || !source.body_bounds_px.finite_positive() ||
        source.speaker_utf8.has_value() != source.speaker_bounds_px.has_value()) {
        error = "subtitle text, factory, or physical-pixel bounds are unavailable";
        return false;
    }
    std::wstring body;
    if (!utf8_to_wide(source.body_utf8, body, error)) {
        return false;
    }
    std::wstring locale;
    if (!utf8_to_wide(source.locale.empty() ? std::string{"und"} : source.locale, locale, error)) {
        return false;
    }
    if (!resolve_text_role(*this, write_factory_.Get(), body, locale, source.body_bounds_px,
                           source.body_size_px, source.right_to_left, TextRole::body,
                           destination, error)) {
        destination.clear();
        return false;
    }
    if (source.speaker_utf8) {
        std::wstring speaker;
        if (!utf8_to_wide(*source.speaker_utf8, speaker, error) ||
            !resolve_text_role(*this, write_factory_.Get(), speaker, locale,
                               *source.speaker_bounds_px, source.speaker_size_px,
                               source.right_to_left, TextRole::speaker, destination, error)) {
            destination.clear();
            return false;
        }
    }
    if (destination.empty()) {
        error = "DirectWrite produced no visible subtitle glyph coverage";
        return false;
    }
    return true;
}

bool DirectWriteD2DBackend::create_bitmap(ID2D1DeviceContext* context,
                                          const RenderedLayer& layer,
                                          ID2D1Bitmap1** bitmap,
                                          std::string& error) const {
    if (bitmap) {
        *bitmap = nullptr;
    }
    if (!context || !bitmap || layer.empty() ||
        !CpuSubtitleRenderer::has_safe_premultiplied_alpha(layer)) {
        error = "Direct2D context, destination, or alpha-safe layer is unavailable";
        return false;
    }
    const D2D1_BITMAP_PROPERTIES1 properties{
        D2D1_PIXEL_FORMAT{DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED},
        96.0F,
        96.0F,
        D2D1_BITMAP_OPTIONS_NONE,
        nullptr,
    };
    const HRESULT result = context->CreateBitmap(
        D2D1_SIZE_U{layer.width_px, layer.height_px}, layer.bgra_premultiplied.data(),
        layer.stride_bytes, &properties, bitmap);
    if (FAILED(result)) {
        error = hresult_message("ID2D1DeviceContext::CreateBitmap", result);
        return false;
    }
    return true;
}

} // namespace npc::subtitle::windows

#endif
