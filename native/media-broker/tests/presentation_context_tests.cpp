#include "npc/media_broker/ipc.hpp"

#include <cstddef>
#include <iostream>
#include <stdexcept>

namespace {

using namespace npc::media;

void require(const bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

ipc::TrustedSubtitlePresentationContext fixture() {
    return {
        1,
        4242,
        0x1234,
        "game.exe",
        8,
        13,
        21,
        34,
        {-1920, 0, 0, 1080},
        {-1912, 31, -8, 1072},
        {1904, 1041},
        R"(\\.\DISPLAY2)",
        {-1920, 0, 0, 1080},
        {-1920, 0, 0, 1040},
        true,
        144,
        144,
        true,
        true,
        true,
        true,
        true,
        2,
        true,
        0,
        10,
        true,
        203.2,
        CaptureBackend::windows_graphics_capture,
        1,
        true,
        true,
        true,
        ColorSpace::hdr_sc_rgb,
        55,
        10'000'000,
        89,
    };
}

void round_trip_preserves_exact_negative_desktop_geometry_and_display_evidence() {
    const auto value = fixture();
    const auto encoded = ipc::encode_trusted_subtitle_presentation_context(value);
    require(encoded.has_value(), "encode presentation context");
    const auto decoded = ipc::decode_trusted_subtitle_presentation_context(*encoded);
    require(decoded.has_value(), "decode presentation context");
    require(decoded->selected_process_id == value.selected_process_id, "process binding");
    require(decoded->selected_window == value.selected_window, "window binding");
    require(decoded->window_bounds_px == value.window_bounds_px, "window bounds");
    require(decoded->client_bounds_px == value.client_bounds_px, "client bounds");
    require(decoded->captured_content_px == value.captured_content_px, "capture extent");
    require(decoded->monitor_id == value.monitor_id, "monitor identity");
    require(decoded->dpi_available && decoded->dpi_x == 144, "DPI evidence");
    require(decoded->hdr_evidence_available && decoded->hdr_active, "HDR evidence");
    require(decoded->color_encoding_available && decoded->bits_per_color_channel == 10,
            "color evidence");
    require(decoded->sdr_white_level_available && decoded->sdr_white_level_nits == 203.2,
            "white-level evidence");
    require(decoded->capture_backend == CaptureBackend::windows_graphics_capture &&
                decoded->capture_scope == 1 && decoded->overlay_capture_excluded,
            "capture-scope evidence");
}

void unavailable_capabilities_are_explicit_and_cannot_carry_guessed_values() {
    auto value = fixture();
    value.dpi_available = false;
    value.dpi_x = 0;
    value.dpi_y = 0;
    value.hdr_evidence_available = false;
    value.hdr_supported = false;
    value.hdr_user_enabled = false;
    value.hdr_active = false;
    value.active_color_mode = 0;
    value.color_encoding_available = false;
    value.color_encoding = 0;
    value.bits_per_color_channel = 0;
    value.sdr_white_level_available = false;
    value.sdr_white_level_nits = 0;
    require(ipc::encode_trusted_subtitle_presentation_context(value).has_value(),
            "explicit unavailable fields");

    value.dpi_x = 96;
    require(!ipc::encode_trusted_subtitle_presentation_context(value),
            "reject guessed unavailable DPI");
    value.dpi_x = 0;
    value.sdr_white_level_nits = 80.0;
    require(!ipc::encode_trusted_subtitle_presentation_context(value),
            "reject guessed unavailable white level");
}

void command_23_is_empty_authenticated_control_only_and_reserved_ids_remain_distinct() {
    const ipc::Command command = ipc::TrustedSubtitlePresentationContextCommand{};
    const auto encoded = ipc::encode_command(
        ipc::CommandKind::trusted_subtitle_presentation_context, command);
    require(encoded && encoded->empty(), "encode command 23");
    const auto decoded = ipc::decode_command(
        ipc::CommandKind::trusted_subtitle_presentation_context, *encoded);
    require(decoded && std::holds_alternative<
                ipc::TrustedSubtitlePresentationContextCommand>(*decoded),
            "decode command 23");
    require(!ipc::decode_command(ipc::CommandKind::trusted_subtitle_presentation_context,
                                 {reinterpret_cast<const std::byte*>("x"), 1}),
            "reject non-empty command 23");
    static_assert(ipc::command_ids_are_unique());
    static_assert(static_cast<std::uint32_t>(
                      ipc::CommandKind::trusted_subtitle_presentation_context) == 23);
}

} // namespace

int main() {
    round_trip_preserves_exact_negative_desktop_geometry_and_display_evidence();
    unavailable_capabilities_are_explicit_and_cannot_carry_guessed_values();
    command_23_is_empty_authenticated_control_only_and_reserved_ids_remain_distinct();
    std::cout << "trusted subtitle presentation-context tests passed\n";
}
