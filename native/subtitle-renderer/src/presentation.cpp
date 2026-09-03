#include "npc/subtitle_renderer/presentation.hpp"

#include <algorithm>
#include <cmath>
#include <iomanip>
#include <sstream>
#include <utility>

namespace npc::subtitle {
namespace {

[[nodiscard]] PresentationResult fail(const PresentationErrorCode code,
                                      std::string message,
                                      std::optional<RenderError> render_error = std::nullopt) {
    PresentationResult result;
    result.error = PresentationError{code, std::move(message), std::move(render_error)};
    return result;
}

[[nodiscard]] bool valid_text_identity(std::string_view value, const std::size_t maximum) noexcept {
    if (value.empty() || value.size() > maximum) {
        return false;
    }
    for (const unsigned char character : value) {
        if (character < 0x20U || character == 0x7FU) {
            return false;
        }
    }
    return true;
}

[[nodiscard]] bool lowercase_sha256(const std::string_view value) noexcept {
    return value.size() == 64 && std::all_of(value.begin(), value.end(), [](const char byte) {
               return (byte >= '0' && byte <= '9') || (byte >= 'a' && byte <= 'f');
           });
}

[[nodiscard]] bool renderer_authority_valid(const RendererAuthorityBinding& authority) noexcept {
    const auto finite_positive = [](const float value) {
        return std::isfinite(value) && value > 0.0F;
    };
    return lowercase_sha256(authority.sha256) && !authority.sources_json.empty() &&
           authority.sources_json.size() <= 16U * 1024U &&
           valid_text_identity(authority.style_id, 128) &&
           std::isfinite(authority.safe_area_dp) && authority.safe_area_dp >= 0.0F &&
           authority.safe_area_dp <= 256.0F && std::isfinite(authority.text_scale) &&
           authority.text_scale >= 0.75F && authority.text_scale <= 2.0F &&
           finite_positive(authority.body_size_dp) && finite_positive(authority.speaker_size_dp) &&
           finite_positive(authority.line_height) && authority.max_body_lines > 0 &&
           authority.max_body_lines <= 32 &&
           std::isfinite(authority.max_width_fraction) && authority.max_width_fraction >= 0.1F &&
           authority.max_width_fraction <= 1.0F && finite_positive(authority.min_width_dp) &&
           finite_positive(authority.max_width_dp) &&
           authority.min_width_dp <= authority.max_width_dp &&
           finite_positive(authority.padding_x_dp) && finite_positive(authority.padding_y_dp) &&
           finite_positive(authority.speaker_gap_dp) &&
           finite_positive(authority.fallback_bottom_dp) &&
           std::isfinite(authority.corner_radius_dp) && authority.corner_radius_dp >= 0.0F &&
           authority.body.valid() && authority.speaker.valid() &&
           authority.outline.color.valid() && authority.shadow.color.valid() &&
           authority.backplate.fill.valid() && authority.backplate.border.valid() &&
           std::isfinite(authority.outline.width_px) && authority.outline.width_px >= 0.0F &&
           std::isfinite(authority.shadow.offset_x_px) &&
           std::isfinite(authority.shadow.offset_y_px) &&
           std::isfinite(authority.shadow.blur_radius_px) &&
           authority.shadow.blur_radius_px >= 0.0F &&
           std::isfinite(authority.backplate.border_width_px) &&
           authority.backplate.border_width_px >= 0.0F && std::isfinite(authority.opacity) &&
           authority.opacity >= 0.25F && authority.opacity <= 1.0F;
}

[[nodiscard]] bool binding_valid(const PresentationSessionBinding& binding) noexcept {
    std::byte nonce_or{};
    for (const auto value : binding.launch_nonce) {
        nonce_or |= value;
    }
    return nonce_or != std::byte{} && valid_text_identity(binding.session_id, 128) &&
           binding.client_process_id != 0 &&
           binding.client_process_creation_time != 0 &&
           valid_text_identity(binding.client_executable_name, 260) &&
           renderer_authority_valid(binding.renderer_authority);
}

[[nodiscard]] bool nonce_matches(
    const std::array<std::byte, presentation_nonce_bytes>& left,
    const std::array<std::byte, presentation_nonce_bytes>& right) noexcept {
    std::byte difference{};
    for (std::size_t index = 0; index < left.size(); ++index) {
        difference |= left[index] ^ right[index];
    }
    return difference == std::byte{};
}

[[nodiscard]] bool auth_matches(const PresentationSessionBinding& binding,
                                const PresentationAuth& auth) noexcept {
    return nonce_matches(binding.launch_nonce, auth.launch_nonce) &&
           binding.client_process_id == auth.client_process_id &&
           binding.client_process_creation_time == auth.client_process_creation_time &&
           binding.client_executable_name == auth.client_executable_name;
}

[[nodiscard]] bool dpi_valid(const std::uint32_t value) noexcept {
    // Windows supports unusual accessibility scaling. Bound only corrupted or
    // hostile values; geometry itself remains in physical desktop pixels.
    return value >= 48 && value <= 960;
}

[[nodiscard]] bool color_contract_valid(const SubtitlePresentationRequest& request) noexcept {
    if (!std::isfinite(request.sdr_white_level_nits) || request.sdr_white_level_nits < 40.0F ||
        request.sdr_white_level_nits > 1000.0F) {
        return false;
    }
    return request.target_color_space == TargetColorSpace::sdr_srgb ||
           request.target_color_space == TargetColorSpace::sdr_sc_rgb ||
           request.target_color_space == TargetColorSpace::hdr10_pq ||
           request.target_color_space == TargetColorSpace::hdr_sc_rgb ||
           request.target_color_space == TargetColorSpace::unknown;
}

[[nodiscard]] ColorTreatment canonical_color_treatment(
    const TargetColorSpace target_color_space) noexcept {
    // The packaged surface is always BGRA8/sRGB-authored. Only an exact SDR
    // sRGB target can bypass compositor SDR-white mapping.
    return target_color_space == TargetColorSpace::sdr_srgb
               ? ColorTreatment::sdr_premultiplied_source_over
               : ColorTreatment::windows_compositor_sdr_white_mapping;
}

} // namespace

SubtitlePresentationService::SubtitlePresentationService(PresentationSessionBinding binding,
                                                         ISubtitleSurface& surface,
                                                         RenderLimits limits)
    : binding_(std::move(binding)), surface_(surface), renderer_(limits) {}

SubtitlePresentationService::~SubtitlePresentationService() { shutdown(); }

PresentationResult SubtitlePresentationService::present(const SubtitlePresentationRequest& request,
                                                        const std::uint64_t now_qpc) {
    if (shutdown_) {
        return fail(PresentationErrorCode::surface_failed, "subtitle service is shut down");
    }
    if (request.protocol_version != presentation_protocol_version_v1 ||
        request.render.protocol_version != protocol_version_v1) {
        return fail(PresentationErrorCode::unsupported_protocol,
                    "subtitle presentation or renderer protocol is unsupported");
    }
    if (!binding_valid(binding_)) {
        return fail(PresentationErrorCode::authentication_failed,
                    "subtitle service has no valid authenticated peer binding");
    }
    if (binding_.session_id != request.auth.session_id) {
        return fail(PresentationErrorCode::session_mismatch,
                    "subtitle presentation belongs to another launch session");
    }
    if (!auth_matches(binding_, request.auth)) {
        return fail(PresentationErrorCode::authentication_failed,
                    "subtitle presentation peer does not match the launch binding");
    }
    if (request.auth.request_sequence == 0 ||
        request.auth.request_sequence <= highest_request_sequence_) {
        return fail(PresentationErrorCode::sequence_replayed,
                    "subtitle presentation request sequence was replayed");
    }
    // Consume an authenticated sequence even when its payload later fails.
    // Otherwise a malicious client could probe validation with one sequence.
    highest_request_sequence_ = request.auth.request_sequence;
    if (request.auth.deadline_qpc == 0 || now_qpc >= request.auth.deadline_qpc) {
        return fail(PresentationErrorCode::deadline_expired,
                    "subtitle presentation deadline expired");
    }
    if (request.cancellation_generation != cancellation_generation_) {
        return fail(PresentationErrorCode::cancellation_mismatch,
                    "subtitle presentation cancellation generation is stale or premature");
    }
    if (request.renderer_authority_revision != binding_.renderer_authority.revision ||
        request.renderer_authority_sha256 != binding_.renderer_authority.sha256) {
        return fail(PresentationErrorCode::renderer_authority_mismatch,
                    "subtitle renderer authority changed after the authenticated launch handshake");
    }
    const bool native_identity = request.provenance == PresentationProvenance::trusted_native_capture
                                     ? request.target_geometry_epoch != 0 &&
                                           request.render.capture_sequence != 0 &&
                                           request.render.graphics_generation != 0
                                     : request.target_geometry_epoch == 0 &&
                                           request.render.capture_sequence == 0 &&
                                           request.render.graphics_generation == 0 &&
                                           request.render.fallback.enabled;
    if (!valid_text_identity(request.turn_id, 128) || request.sentence_id == 0 ||
        request.render.presentation_id == 0 || !native_identity) {
        return fail(PresentationErrorCode::invalid_identity,
                    "subtitle turn, sentence, target, capture, or presentation identity is invalid");
    }
    if (!dpi_valid(request.dpi_x) || !dpi_valid(request.dpi_y)) {
        return fail(PresentationErrorCode::invalid_dpi,
                    "subtitle DPI is outside the mixed-DPI contract");
    }
    if (!request.grapheme_clusters_preserved ||
        (request.direction == ParagraphDirection::right_to_left &&
         !request.bidi_shaping_applied)) {
        return fail(PresentationErrorCode::shaping_contract_failed,
                    "subtitle text was not shaped with required bidi and grapheme semantics");
    }
    if (!color_contract_valid(request)) {
        return fail(PresentationErrorCode::color_contract_failed,
                    "subtitle target color-space metadata is invalid");
    }
    const auto rendered = renderer_.render(request.render);
    if (!rendered || !rendered.layer) {
        return fail(PresentationErrorCode::render_failed, "subtitle layer rendering failed",
                    rendered.error);
    }
    if (rendered.layer->presentation_id != request.render.presentation_id ||
        rendered.layer->capture_sequence != request.render.capture_sequence ||
        rendered.layer->graphics_generation != request.render.graphics_generation) {
        return fail(PresentationErrorCode::render_failed,
                    "subtitle renderer did not preserve presentation identity");
    }

    SurfacePresentation surface_evidence;
    std::string surface_error;
    if (!surface_.present(*rendered.layer, request.target_color_space,
                          request.sdr_white_level_nits, surface_evidence, surface_error) ||
        !surface_evidence.committed || surface_evidence.presented_qpc == 0) {
        surface_.hide();
        active_receipt_.reset();
        return fail(PresentationErrorCode::surface_failed,
                    surface_error.empty() ? "subtitle surface did not commit" : surface_error);
    }
    if (surface_evidence.color_treatment !=
        canonical_color_treatment(request.target_color_space)) {
        surface_.hide();
        active_receipt_.reset();
        return fail(PresentationErrorCode::color_contract_failed,
                    "subtitle surface reported a non-canonical color treatment");
    }

    const auto layer_hash = deterministic_layer_hash(*rendered.layer);
    SubtitlePresentationReceipt receipt;
    receipt.receipt_id = subtitle_receipt_id(
        binding_.session_id, request.turn_id, request.sentence_id,
        request.render.presentation_id, layer_hash, binding_.renderer_authority.revision,
        binding_.renderer_authority.sha256);
    receipt.session_id = binding_.session_id;
    receipt.turn_id = request.turn_id;
    receipt.sentence_id = request.sentence_id;
    receipt.provenance = request.provenance;
    receipt.presentation_id = request.render.presentation_id;
    receipt.request_sequence = request.auth.request_sequence;
    receipt.cancellation_generation = request.cancellation_generation;
    receipt.target_geometry_epoch = request.target_geometry_epoch;
    receipt.capture_sequence = request.render.capture_sequence;
    receipt.graphics_generation = request.render.graphics_generation;
    receipt.layer_hash = layer_hash;
    receipt.presented_qpc = surface_evidence.presented_qpc;
    receipt.desktop_x_px = rendered.layer->desktop_x_px;
    receipt.desktop_y_px = rendered.layer->desktop_y_px;
    receipt.width_px = rendered.layer->width_px;
    receipt.height_px = rendered.layer->height_px;
    receipt.dpi_x = request.dpi_x;
    receipt.dpi_y = request.dpi_y;
    receipt.direction = request.direction;
    receipt.bidi_shaping_applied = request.bidi_shaping_applied;
    receipt.grapheme_clusters_preserved = request.grapheme_clusters_preserved;
    receipt.used_bottom_center_fallback = rendered.layer->used_bottom_center_fallback;
    receipt.color_treatment = surface_evidence.color_treatment;
    receipt.renderer_authority_revision = binding_.renderer_authority.revision;
    receipt.renderer_authority_sha256 = binding_.renderer_authority.sha256;
    receipt.renderer_authority_sources_json = binding_.renderer_authority.sources_json;
    receipt.renderer_style_id = binding_.renderer_authority.style_id;
    receipt.renderer_safe_area_dp = binding_.renderer_authority.safe_area_dp;
    receipt.renderer_text_scale = binding_.renderer_authority.text_scale;
    receipt.renderer_backplate_enabled = binding_.renderer_authority.backplate.enabled;
    receipt.renderer_opacity = binding_.renderer_authority.opacity;
    receipt.committed = true;
    active_receipt_ = receipt;
    PresentationResult result;
    result.receipt = std::move(receipt);
    return result;
}

void SubtitlePresentationService::cancel(const std::uint64_t new_generation) noexcept {
    cancellation_generation_ =
        std::max(cancellation_generation_ + 1, new_generation);
    surface_.hide();
    active_receipt_.reset();
}

void SubtitlePresentationService::target_lost() noexcept {
    cancel(cancellation_generation_ + 1);
}

void SubtitlePresentationService::shutdown() noexcept {
    if (shutdown_) {
        return;
    }
    shutdown_ = true;
    surface_.hide();
    active_receipt_.reset();
}

std::uint64_t SubtitlePresentationService::cancellation_generation() const noexcept {
    return cancellation_generation_;
}

std::uint64_t SubtitlePresentationService::highest_request_sequence() const noexcept {
    return highest_request_sequence_;
}

const std::optional<SubtitlePresentationReceipt>&
SubtitlePresentationService::active_receipt() const noexcept {
    return active_receipt_;
}

std::string subtitle_receipt_id(const std::string_view session_id,
                                const std::string_view turn_id,
                                const std::uint64_t sentence_id,
                                const std::uint64_t presentation_id,
                                const std::uint64_t layer_hash,
                                const std::uint64_t renderer_authority_revision,
                                const std::string_view renderer_authority_sha256) {
    std::uint64_t hash = 14695981039346656037ULL;
    const auto absorb = [&](const std::span<const std::byte> bytes) {
        for (const auto value : bytes) {
            hash ^= std::to_integer<std::uint8_t>(value);
            hash *= 1099511628211ULL;
        }
    };
    absorb(std::as_bytes(std::span(session_id.data(), session_id.size())));
    absorb(std::as_bytes(std::span(turn_id.data(), turn_id.size())));
    const std::array values{sentence_id, presentation_id, layer_hash,
                            renderer_authority_revision};
    absorb(std::as_bytes(std::span(values)));
    absorb(std::as_bytes(std::span(renderer_authority_sha256.data(),
                                   renderer_authority_sha256.size())));
    std::ostringstream output;
    output << "subtitle-v1-" << std::hex << std::setfill('0') << std::setw(16) << hash;
    return output.str();
}

} // namespace npc::subtitle
