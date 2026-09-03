#include "npc/subtitle_renderer/presentation.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <string>

namespace {

using namespace npc::subtitle;

int failures{};

#define CHECK(expression)                                                                       \
    do {                                                                                        \
        if (!(expression)) {                                                                    \
            std::cerr << __FILE__ << ':' << __LINE__ << ": CHECK failed: " #expression << '\n'; \
            ++failures;                                                                         \
        }                                                                                       \
    } while (false)

class RecordingSurface final : public ISubtitleSurface {
public:
    bool present(const RenderedLayer& layer,
                 const TargetColorSpace color_space,
                 const float,
                 SurfacePresentation& evidence,
                 std::string& error) override {
        ++presentations;
        last_presentation_id = layer.presentation_id;
        if (fail_next) {
            fail_next = false;
            error = "fixture surface rejected commit";
            return false;
        }
        evidence.presented_qpc = 9000 + presentations;
        evidence.committed = true;
        evidence.color_treatment = forced_treatment.value_or(
            color_space == TargetColorSpace::sdr_srgb
                ? ColorTreatment::sdr_premultiplied_source_over
                : ColorTreatment::windows_compositor_sdr_white_mapping);
        visible = true;
        return true;
    }

    void hide() noexcept override {
        ++hides;
        visible = false;
    }

    std::uint64_t presentations{};
    std::uint64_t hides{};
    std::uint64_t last_presentation_id{};
    bool visible{};
    bool fail_next{};
    std::optional<ColorTreatment> forced_treatment;
};

[[nodiscard]] PresentationSessionBinding binding() {
    PresentationSessionBinding value;
    for (std::size_t index = 0; index < value.launch_nonce.size(); ++index) {
        value.launch_nonce[index] = static_cast<std::byte>(index + 1U);
    }
    value.session_id = "session-subtitle-tests";
    value.client_process_id = 4242;
    value.client_process_creation_time = 99112233;
    value.client_executable_name = "interactive-npcs.exe";
    auto& authority = value.renderer_authority;
    authority.revision = 7;
    authority.sha256 = std::string(64, 'a');
    authority.sources_json = R"({"selectedStyle":{"kind":"bundledManifestDefault"}})";
    authority.style_id = "cinematic_glass";
    authority.safe_area_dp = 32.0F;
    authority.text_scale = 1.25F;
    authority.body_size_dp = 27.5F;
    authority.speaker_size_dp = 17.5F;
    authority.line_height = 1.28F;
    authority.max_body_lines = 4;
    authority.max_width_fraction = 0.56F;
    authority.min_width_dp = 220.0F;
    authority.max_width_dp = 760.0F;
    authority.padding_x_dp = 22.0F;
    authority.padding_y_dp = 16.0F;
    authority.speaker_gap_dp = 7.0F;
    authority.fallback_bottom_dp = 34.0F;
    authority.corner_radius_dp = 14.0F;
    authority.body = {0.965F, 0.975F, 0.99F, 1.0F};
    authority.speaker = {0.4F, 0.84F, 1.0F, 1.0F};
    authority.outline = {true, 1.35F, {0.01F, 0.012F, 0.018F, 0.96F}};
    authority.shadow = {true, 0.0F, 3.0F, 10.0F, {0.0F, 0.0F, 0.0F, 0.68F}};
    authority.backplate = {true, 0.0F, 1.0F,
                           {0.018F, 0.024F, 0.039F, 0.82F},
                           {0.75F, 0.82F, 0.94F, 0.22F}};
    authority.opacity = 0.8F;
    return value;
}

[[nodiscard]] SubtitlePresentationRequest request(const PresentationSessionBinding& peer,
                                                  const std::uint64_t sequence = 1,
                                                  const std::uint64_t generation = 0) {
    SubtitlePresentationRequest value;
    value.auth.launch_nonce = peer.launch_nonce;
    value.auth.session_id = peer.session_id;
    value.auth.client_process_id = peer.client_process_id;
    value.auth.client_process_creation_time = peer.client_process_creation_time;
    value.auth.client_executable_name = peer.client_executable_name;
    value.auth.request_sequence = sequence;
    value.auth.deadline_qpc = 20'000;
    value.turn_id = "turn-12";
    value.sentence_id = sequence;
    value.cancellation_generation = generation;
    value.target_geometry_epoch = 3;
    value.renderer_authority_revision = peer.renderer_authority.revision;
    value.renderer_authority_sha256 = peer.renderer_authority.sha256;
    value.dpi_x = 144;
    value.dpi_y = 144;
    value.render.presentation_id = 1000 + sequence;
    value.render.capture_sequence = 81;
    value.render.graphics_generation = 7;
    value.render.viewport_px = {-1600.0F, 0.0F, 1600.0F, 900.0F};
    value.render.output_clip_px = value.render.viewport_px;
    value.render.layout.bounds_px = {-1100.0F, 700.0F, 520.0F, 120.0F};
    value.render.layout.body_bounds_px = {-1070.0F, 730.0F, 460.0F, 60.0F};
    ResolvedGlyphRun run;
    run.role = TextRole::body;
    run.clip_px = value.render.layout.body_bounds_px;
    GlyphMask mask;
    mask.origin_x_px = -1060;
    mask.origin_y_px = 740;
    mask.width_px = 120;
    mask.height_px = 24;
    mask.stride_bytes = mask.width_px;
    mask.coverage.assign(static_cast<std::size_t>(mask.width_px) * mask.height_px, 220);
    run.glyphs.push_back(std::move(mask));
    value.render.glyph_runs.push_back(std::move(run));
    return value;
}

void committed_receipt_attests_physical_layout_and_compositor_managed_hdr() {
    const auto peer = binding();
    RecordingSurface surface;
    SubtitlePresentationService service(peer, surface);
    auto value = request(peer);
    value.target_color_space = TargetColorSpace::hdr10_pq;
    value.sdr_white_level_nits = 203.0F;
    const auto result = service.present(value, 10'000);
    CHECK(result);
    if (!result) {
        return;
    }
    CHECK(result.receipt->receipt_id.starts_with("subtitle-v1-"));
    CHECK(result.receipt->session_id == peer.session_id);
    CHECK(result.receipt->turn_id == "turn-12");
    CHECK(result.receipt->sentence_id == 1);
    CHECK(result.receipt->presentation_id == 1001);
    CHECK(result.receipt->request_sequence == 1);
    CHECK(result.receipt->target_geometry_epoch == 3);
    CHECK(result.receipt->capture_sequence == 81);
    CHECK(result.receipt->graphics_generation == 7);
    CHECK(result.receipt->dpi_x == 144 && result.receipt->dpi_y == 144);
    CHECK(result.receipt->width_px > 0 && result.receipt->height_px > 0);
    CHECK(result.receipt->layer_hash != 0);
    CHECK(result.receipt->color_treatment ==
          ColorTreatment::windows_compositor_sdr_white_mapping);
    CHECK(result.receipt->renderer_authority_revision == peer.renderer_authority.revision);
    CHECK(result.receipt->renderer_authority_sha256 == peer.renderer_authority.sha256);
    CHECK(result.receipt->renderer_authority_sources_json == peer.renderer_authority.sources_json);
    CHECK(result.receipt->renderer_style_id == "cinematic_glass");
    CHECK(result.receipt->renderer_safe_area_dp == 32.0F);
    CHECK(result.receipt->renderer_text_scale == 1.25F);
    CHECK(result.receipt->renderer_backplate_enabled);
    CHECK(result.receipt->renderer_opacity == 0.8F);
    CHECK(result.receipt->committed);
    CHECK(service.active_receipt().has_value());
    CHECK(surface.visible);
}

void renderer_authority_mismatch_is_consumed_as_a_nonreplayable_sequence() {
    const auto peer = binding();
    RecordingSurface surface;
    SubtitlePresentationService service(peer, surface);
    auto digest_mismatch = request(peer);
    digest_mismatch.renderer_authority_sha256 = std::string(64, 'b');
    const auto rejected = service.present(digest_mismatch, 10'000);
    CHECK(!rejected);
    CHECK(rejected.error->code == PresentationErrorCode::renderer_authority_mismatch);
    CHECK(service.highest_request_sequence() == 1);
    CHECK(surface.presentations == 0);

    auto replay = request(peer);
    const auto replayed = service.present(replay, 10'000);
    CHECK(!replayed);
    CHECK(replayed.error->code == PresentationErrorCode::sequence_replayed);

    auto revision_mismatch = request(peer, 2);
    revision_mismatch.renderer_authority_revision += 1;
    const auto wrong_revision = service.present(revision_mismatch, 10'000);
    CHECK(!wrong_revision);
    CHECK(wrong_revision.error->code == PresentationErrorCode::renderer_authority_mismatch);
}

void peer_authentication_replay_and_deadline_fail_closed() {
    const auto peer = binding();
    RecordingSurface surface;
    SubtitlePresentationService service(peer, surface);

    auto wrong_nonce = request(peer);
    wrong_nonce.auth.launch_nonce.front() = std::byte{0xFF};
    const auto auth_result = service.present(wrong_nonce, 10'000);
    CHECK(!auth_result);
    CHECK(auth_result.error->code == PresentationErrorCode::authentication_failed);
    CHECK(service.highest_request_sequence() == 0);

    const auto accepted = service.present(request(peer), 10'000);
    CHECK(accepted);
    const auto replay = service.present(request(peer), 10'000);
    CHECK(!replay);
    CHECK(replay.error->code == PresentationErrorCode::sequence_replayed);

    auto expired = request(peer, 2);
    expired.auth.deadline_qpc = 10'000;
    const auto deadline = service.present(expired, 10'000);
    CHECK(!deadline);
    CHECK(deadline.error->code == PresentationErrorCode::deadline_expired);
    CHECK(service.highest_request_sequence() == 2);
    CHECK(surface.presentations == 1);
}

void rtl_requires_bidi_shaping_and_grapheme_preservation() {
    const auto peer = binding();
    RecordingSurface surface;
    SubtitlePresentationService service(peer, surface);

    auto unshaped = request(peer);
    unshaped.direction = ParagraphDirection::right_to_left;
    const auto rejected = service.present(unshaped, 10'000);
    CHECK(!rejected);
    CHECK(rejected.error->code == PresentationErrorCode::shaping_contract_failed);

    auto shaped = request(peer, 2);
    shaped.direction = ParagraphDirection::right_to_left;
    shaped.bidi_shaping_applied = true;
    const auto accepted = service.present(shaped, 10'000);
    CHECK(accepted);
    CHECK(accepted.receipt->direction == ParagraphDirection::right_to_left);
    CHECK(accepted.receipt->bidi_shaping_applied);

    auto broken_clusters = request(peer, 3);
    broken_clusters.grapheme_clusters_preserved = false;
    const auto clusters = service.present(broken_clusters, 10'000);
    CHECK(!clusters);
    CHECK(clusters.error->code == PresentationErrorCode::shaping_contract_failed);
}

void cancellation_replacement_target_loss_and_shutdown_hide_immediately() {
    const auto peer = binding();
    RecordingSurface surface;
    {
        SubtitlePresentationService service(peer, surface);
        CHECK(service.present(request(peer), 10'000));
        CHECK(service.present(request(peer, 2), 10'000));
        CHECK(surface.presentations == 2);
        CHECK(service.active_receipt()->presentation_id == 1002);

        service.cancel(5);
        CHECK(service.cancellation_generation() == 5);
        CHECK(!surface.visible);
        CHECK(!service.active_receipt());

        const auto stale = service.present(request(peer, 3, 0), 10'000);
        CHECK(!stale);
        CHECK(stale.error->code == PresentationErrorCode::cancellation_mismatch);
        CHECK(service.present(request(peer, 4, 5), 10'000));
        service.target_lost();
        CHECK(service.cancellation_generation() == 6);
        CHECK(!surface.visible);
        service.shutdown();
        const auto stopped = service.present(request(peer, 5, 6), 10'000);
        CHECK(!stopped);
        CHECK(stopped.error->code == PresentationErrorCode::surface_failed);
    }
    CHECK(!surface.visible);
    CHECK(surface.hides >= 3);
}

void surface_failure_cannot_fabricate_a_delivery_receipt() {
    const auto peer = binding();
    RecordingSurface surface;
    SubtitlePresentationService service(peer, surface);
    surface.fail_next = true;
    const auto result = service.present(request(peer), 10'000);
    CHECK(!result);
    CHECK(result.error->code == PresentationErrorCode::surface_failed);
    CHECK(!service.active_receipt());
    CHECK(!surface.visible);
    CHECK(surface.hides == 1);
}

void malformed_dpi_color_and_render_payloads_are_typed() {
    const auto peer = binding();
    RecordingSurface surface;
    SubtitlePresentationService service(peer, surface);

    auto dpi = request(peer);
    dpi.dpi_x = 0;
    const auto dpi_result = service.present(dpi, 10'000);
    CHECK(!dpi_result);
    CHECK(dpi_result.error->code == PresentationErrorCode::invalid_dpi);

    auto color = request(peer, 2);
    color.sdr_white_level_nits = 5000.0F;
    const auto color_result = service.present(color, 10'000);
    CHECK(!color_result);
    CHECK(color_result.error->code == PresentationErrorCode::color_contract_failed);

    surface.forced_treatment = ColorTreatment::hdr10_tone_mapped_source_over;
    auto noncanonical_surface = request(peer, 3);
    noncanonical_surface.target_color_space = TargetColorSpace::hdr10_pq;
    const auto noncanonical_result = service.present(noncanonical_surface, 10'000);
    CHECK(!noncanonical_result);
    CHECK(noncanonical_result.error->code == PresentationErrorCode::color_contract_failed);
    CHECK(!service.active_receipt());
    surface.forced_treatment.reset();

    auto render = request(peer, 4);
    render.render.glyph_runs.front().glyphs.front().stride_bytes = 1;
    const auto render_result = service.present(render, 10'000);
    CHECK(!render_result);
    CHECK(render_result.error->code == PresentationErrorCode::render_failed);
    CHECK(render_result.error->render_error.has_value());
    CHECK(render_result.error->render_error->code == RenderErrorCode::malformed_glyph_mask);
}

void console_fallback_receipt_never_fabricates_native_capture_identity() {
    const auto peer = binding();
    RecordingSurface surface;
    SubtitlePresentationService service(peer, surface);
    auto value = request(peer);
    value.provenance = PresentationProvenance::console_bottom_center_unavailable;
    value.target_geometry_epoch = 0;
    value.render.capture_sequence = 0;
    value.render.graphics_generation = 0;
    value.render.layout.bounds_px = {-5000.0F, -5000.0F, 520.0F, 120.0F};
    value.render.layout.body_bounds_px = {-4970.0F, -4970.0F, 460.0F, 60.0F};
    value.render.glyph_runs.front().clip_px = value.render.layout.body_bounds_px;
    value.render.glyph_runs.front().glyphs.front().origin_x_px = -4960;
    value.render.glyph_runs.front().glyphs.front().origin_y_px = -4960;
    value.target_color_space = TargetColorSpace::unknown;
    value.sdr_white_level_nits = 80.0F;
    const auto result = service.present(value, 10'000);
    CHECK(result);
    if (!result) {
        return;
    }
    CHECK(result.receipt->provenance ==
          PresentationProvenance::console_bottom_center_unavailable);
    CHECK(result.receipt->target_geometry_epoch == 0);
    CHECK(result.receipt->capture_sequence == 0);
    CHECK(result.receipt->graphics_generation == 0);
    CHECK(result.receipt->used_bottom_center_fallback);
    CHECK(result.receipt->color_treatment ==
          ColorTreatment::windows_compositor_sdr_white_mapping);

    auto fabricated = value;
    fabricated.auth.request_sequence = 2;
    fabricated.sentence_id = 2;
    fabricated.render.presentation_id = 1002;
    fabricated.render.capture_sequence = 99;
    const auto rejected = service.present(fabricated, 10'000);
    CHECK(!rejected);
    CHECK(rejected.error->code == PresentationErrorCode::invalid_identity);
}

} // namespace

int main() {
    committed_receipt_attests_physical_layout_and_compositor_managed_hdr();
    peer_authentication_replay_and_deadline_fail_closed();
    renderer_authority_mismatch_is_consumed_as_a_nonreplayable_sequence();
    rtl_requires_bidi_shaping_and_grapheme_preservation();
    cancellation_replacement_target_loss_and_shutdown_hide_immediately();
    surface_failure_cannot_fabricate_a_delivery_receipt();
    malformed_dpi_color_and_render_payloads_are_typed();
    console_fallback_receipt_never_fabricates_native_capture_identity();
    if (failures != 0) {
        std::cerr << failures << " subtitle presentation assertion(s) failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "subtitle presentation tests passed\n";
    return EXIT_SUCCESS;
}
