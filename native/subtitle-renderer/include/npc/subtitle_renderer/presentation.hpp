#pragma once

#include "npc/subtitle_renderer/renderer.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <string_view>

namespace npc::subtitle {

inline constexpr std::uint32_t presentation_protocol_version_v1 = 1;
inline constexpr std::size_t presentation_nonce_bytes = 32;

enum class ParagraphDirection : std::uint8_t { left_to_right = 0, right_to_left = 1 };
enum class TargetColorSpace : std::uint8_t {
    sdr_srgb = 0,
    sdr_sc_rgb = 1,
    hdr10_pq = 2,
    hdr_sc_rgb = 3,
    unknown = 4,
};
enum class ColorTreatment : std::uint8_t {
    sdr_premultiplied_source_over = 0,
    // Values 1 and 2 remain reserved for decoding pre-v1 review receipts.
    // The packaged BGRA8 presenter never emits them because it has no custom
    // linear-scRGB or PQ tone-mapping shader.
    scrgb_linear_source_over = 1,
    hdr10_tone_mapped_source_over = 2,
    windows_compositor_sdr_white_mapping = 3,
};
enum class PresentationProvenance : std::uint8_t {
    trusted_native_capture = 0,
    console_bottom_center_unavailable = 1,
};

/// Immutable renderer preference authority established during the private
/// launch handshake. Dimensions are density-independent pixels and are scaled
/// exactly once by the presenter using the target DPI. `sources_json` is the
/// canonical validated provenance object from npc-subtitle-engine.
struct RendererAuthorityBinding {
    std::uint64_t revision{};
    std::string sha256;
    std::string sources_json;
    std::string style_id;
    float safe_area_dp{};
    float text_scale{1.0F};
    float body_size_dp{};
    float speaker_size_dp{};
    float line_height{};
    std::uint16_t max_body_lines{};
    float max_width_fraction{};
    float min_width_dp{};
    float max_width_dp{};
    float padding_x_dp{};
    float padding_y_dp{};
    float speaker_gap_dp{};
    float fallback_bottom_dp{};
    float corner_radius_dp{};
    Color body;
    Color speaker;
    OutlineStyle outline;
    ShadowStyle shadow;
    BackplateStyle backplate;
    float opacity{1.0F};
};

/// Immutable peer identity established by the broker's authenticated launch
/// handshake. A PID alone is insufficient because Windows can reuse it.
struct PresentationSessionBinding {
    std::array<std::byte, presentation_nonce_bytes> launch_nonce{};
    std::string session_id;
    std::uint32_t client_process_id{};
    std::uint64_t client_process_creation_time{};
    std::string client_executable_name;
    RendererAuthorityBinding renderer_authority;
};

struct PresentationAuth {
    std::array<std::byte, presentation_nonce_bytes> launch_nonce{};
    std::string session_id;
    std::uint32_t client_process_id{};
    std::uint64_t client_process_creation_time{};
    std::string client_executable_name;
    std::uint64_t request_sequence{};
    std::uint64_t deadline_qpc{};
};

/// One already-shaped sentence. The physical-pixel RenderRequest remains the
/// authoritative renderer input. Direction and DPI are attested metadata used
/// to reject callers that bypass the shaping or mixed-DPI contract.
struct SubtitlePresentationRequest {
    std::uint32_t protocol_version{presentation_protocol_version_v1};
    PresentationAuth auth;
    std::string turn_id;
    std::uint64_t sentence_id{};
    PresentationProvenance provenance{PresentationProvenance::trusted_native_capture};
    std::uint64_t cancellation_generation{};
    std::uint64_t target_geometry_epoch{};
    std::uint64_t renderer_authority_revision{};
    std::string renderer_authority_sha256;
    std::uint32_t dpi_x{96};
    std::uint32_t dpi_y{96};
    ParagraphDirection direction{ParagraphDirection::left_to_right};
    bool bidi_shaping_applied{};
    bool grapheme_clusters_preserved{true};
    TargetColorSpace target_color_space{TargetColorSpace::sdr_srgb};
    float sdr_white_level_nits{80.0F};
    RenderRequest render;
};

struct SurfacePresentation {
    std::uint64_t presented_qpc{};
    ColorTreatment color_treatment{ColorTreatment::sdr_premultiplied_source_over};
    bool committed{};
};

class ISubtitleSurface {
public:
    virtual ~ISubtitleSurface() = default;

    [[nodiscard]] virtual bool present(const RenderedLayer& layer,
                                       TargetColorSpace target_color_space,
                                       float sdr_white_level_nits,
                                       SurfacePresentation& evidence,
                                       std::string& error) = 0;
    virtual void hide() noexcept = 0;
};

struct SubtitlePresentationReceipt {
    std::uint32_t protocol_version{presentation_protocol_version_v1};
    std::string receipt_id;
    std::string session_id;
    std::string turn_id;
    std::uint64_t sentence_id{};
    PresentationProvenance provenance{PresentationProvenance::trusted_native_capture};
    std::uint64_t presentation_id{};
    std::uint64_t request_sequence{};
    std::uint64_t cancellation_generation{};
    std::uint64_t target_geometry_epoch{};
    std::uint64_t capture_sequence{};
    std::uint64_t graphics_generation{};
    std::uint64_t layer_hash{};
    std::uint64_t presented_qpc{};
    std::int32_t desktop_x_px{};
    std::int32_t desktop_y_px{};
    std::uint32_t width_px{};
    std::uint32_t height_px{};
    std::uint32_t dpi_x{};
    std::uint32_t dpi_y{};
    ParagraphDirection direction{ParagraphDirection::left_to_right};
    bool bidi_shaping_applied{};
    bool grapheme_clusters_preserved{};
    bool used_bottom_center_fallback{};
    ColorTreatment color_treatment{ColorTreatment::sdr_premultiplied_source_over};
    std::uint64_t renderer_authority_revision{};
    std::string renderer_authority_sha256;
    std::string renderer_authority_sources_json;
    std::string renderer_style_id;
    float renderer_safe_area_dp{};
    float renderer_text_scale{};
    bool renderer_backplate_enabled{};
    float renderer_opacity{};
    bool committed{};
};

enum class PresentationErrorCode : std::uint8_t {
    none = 0,
    unsupported_protocol,
    authentication_failed,
    session_mismatch,
    sequence_replayed,
    deadline_expired,
    cancellation_mismatch,
    invalid_identity,
    invalid_dpi,
    shaping_contract_failed,
    color_contract_failed,
    renderer_authority_mismatch,
    render_failed,
    surface_failed,
};

struct PresentationError {
    PresentationErrorCode code{PresentationErrorCode::none};
    std::string message;
    std::optional<RenderError> render_error;
};

struct PresentationResult {
    std::optional<SubtitlePresentationReceipt> receipt;
    std::optional<PresentationError> error;

    [[nodiscard]] explicit operator bool() const noexcept { return receipt.has_value(); }
};

/// Authenticated, replacement-based subtitle presentation state. Only one
/// subtitle may remain visible. Cancellation, generation changes, target loss,
/// and destruction all synchronously hide it.
class SubtitlePresentationService final {
public:
    SubtitlePresentationService(PresentationSessionBinding binding,
                                ISubtitleSurface& surface,
                                RenderLimits limits = {});
    ~SubtitlePresentationService();
    SubtitlePresentationService(const SubtitlePresentationService&) = delete;
    SubtitlePresentationService& operator=(const SubtitlePresentationService&) = delete;

    [[nodiscard]] PresentationResult present(const SubtitlePresentationRequest& request,
                                             std::uint64_t now_qpc);
    void cancel(std::uint64_t new_generation) noexcept;
    void target_lost() noexcept;
    void shutdown() noexcept;

    [[nodiscard]] std::uint64_t cancellation_generation() const noexcept;
    [[nodiscard]] std::uint64_t highest_request_sequence() const noexcept;
    [[nodiscard]] const std::optional<SubtitlePresentationReceipt>& active_receipt() const noexcept;

private:
    PresentationSessionBinding binding_;
    ISubtitleSurface& surface_;
    CpuSubtitleRenderer renderer_;
    std::uint64_t cancellation_generation_{};
    std::uint64_t highest_request_sequence_{};
    std::optional<SubtitlePresentationReceipt> active_receipt_;
    bool shutdown_{};
};

[[nodiscard]] std::string subtitle_receipt_id(std::string_view session_id,
                                              std::string_view turn_id,
                                              std::uint64_t sentence_id,
                                              std::uint64_t presentation_id,
                                              std::uint64_t layer_hash,
                                              std::uint64_t renderer_authority_revision,
                                              std::string_view renderer_authority_sha256);

} // namespace npc::subtitle
