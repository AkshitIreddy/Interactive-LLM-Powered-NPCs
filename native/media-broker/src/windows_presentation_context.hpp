#pragma once

#ifdef _WIN32

#include "npc/media_broker/types.hpp"

#include <cstdint>
#include <optional>
#include <string>

namespace npc::media::windows {

// Native-only evidence for placing subtitles over the currently selected game
// window. Optional display capabilities use explicit availability bits; the
// broker never substitutes a guessed HDR mode, color encoding, DPI, or nits.
struct TrustedSubtitlePresentationContext {
    std::uint32_t schema_version{1};
    std::uint32_t selected_process_id{};
    std::uint64_t selected_window{};
    std::string selected_executable_name;
    std::uint64_t capture_device_generation{};
    std::uint64_t geometry_epoch{};
    std::uint64_t source_frame_sequence{};
    std::uint64_t source_frame_qpc{};
    RectI window_bounds_px;
    RectI client_bounds_px;
    SizeI captured_content_px;
    std::string monitor_id;
    RectI monitor_bounds_px;
    RectI monitor_work_area_px;
    bool dpi_available{};
    std::uint32_t dpi_x{};
    std::uint32_t dpi_y{};
    bool hdr_evidence_available{};
    bool hdr_supported{};
    bool hdr_user_enabled{};
    bool hdr_active{};
    bool advanced_color_active{};
    std::uint32_t active_color_mode{};
    bool color_encoding_available{};
    std::uint32_t color_encoding{};
    std::uint32_t bits_per_color_channel{};
    bool sdr_white_level_available{};
    double sdr_white_level_nits{};
    CaptureBackend capture_backend{CaptureBackend::none};
    // 1 = exact selected HWND via WGC; 2 = monitor capture cropped to the
    // selected client region. Consumers can fail closed on the weaker scope.
    std::uint32_t capture_scope{};
    bool overlay_capture_excluded{};
    bool overlay_visuals_allowed{};
    bool target_color_space_available{};
    ColorSpace target_color_space{ColorSpace::unknown};
    std::uint64_t attested_at_qpc{};
    std::uint64_t qpc_frequency{};
    std::uint64_t attestation_id{};
};

[[nodiscard]] std::optional<TrustedSubtitlePresentationContext>
query_trusted_subtitle_presentation_context(const Diagnostics& diagnostics,
                                            std::string& error);

} // namespace npc::media::windows

#endif
