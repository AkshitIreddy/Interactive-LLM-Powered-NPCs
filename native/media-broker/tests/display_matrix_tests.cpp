#include "npc/media_broker/broker.hpp"
#include "npc/media_broker/geometry.hpp"
#include "npc/media_broker/platform.hpp"

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <string_view>
#include <vector>

namespace {

using namespace npc::media;

int failures{};

#define REQUIRE(...)                                                                                      \
    do {                                                                                                  \
        if (!(__VA_ARGS__)) {                                                                             \
            std::cerr << __FILE__ << ':' << __LINE__ << ": requirement failed: " #__VA_ARGS__ << '\n'; \
            ++failures;                                                                                   \
        }                                                                                                 \
    } while (false)

struct SimulatedDisplayCase {
    std::string_view label;
    RectI desktop_bounds;
    SizeI content_size;
    std::uint32_t dpi;
    ColorSpace color_space;
    bool single_monitor;
};

[[nodiscard]] TargetGeometry full_output_geometry(const SimulatedDisplayCase& value) {
    const RectI content_bounds{
        value.desktop_bounds.left,
        value.desktop_bounds.top,
        value.desktop_bounds.left + value.content_size.width,
        value.desktop_bounds.top + value.content_size.height,
    };
    return {
        content_bounds,
        content_bounds,
        content_bounds,
        value.content_size,
        {std::string{value.label}, value.desktop_bounds, value.desktop_bounds,
         value.dpi, value.dpi, value.color_space, DisplayRotation::identity, 80.0},
        false,
    };
}

void verify_geometry_matrix() {
    // These rows are deterministic synthetic geometry contracts. They do not
    // claim that the corresponding physical panel, scale, or HDR mode exists.
    const std::vector<SimulatedDisplayCase> cases{
        {"single-720p-100-sdr", {0, 0, 1280, 720}, {1280, 720}, 96,
         ColorSpace::sdr_srgb, true},
        {"single-1080p-150-sdr", {0, 0, 1920, 1080}, {1920, 1080}, 144,
         ColorSpace::sdr_srgb, true},
        {"single-1440p-200-sdr", {0, 0, 2560, 1440}, {2560, 1440}, 192,
         ColorSpace::sdr_srgb, true},
        {"single-4k-100-hdr10", {0, 0, 3840, 2160}, {3840, 2160}, 96,
         ColorSpace::hdr10_pq, true},
        {"multi-left-negative-1080p-150-sdr", {-1920, 0, 0, 1080}, {1920, 1080}, 144,
         ColorSpace::sdr_srgb, false},
        {"multi-above-negative-1440p-200-scrgb", {0, -1440, 2560, 0}, {2560, 1440}, 192,
         ColorSpace::hdr_sc_rgb, false},
        {"multi-right-ultrawide-3440x1440-150-hdr10", {2560, 0, 6000, 1440}, {3440, 1440},
         144, ColorSpace::hdr10_pq, false},
        {"multi-left-super-ultrawide-5120x1440-200-sdr", {-5120, 0, 0, 1440}, {5120, 1440},
         192, ColorSpace::sdr_srgb, false},
    };

    std::size_t single_monitor_rows{};
    for (const auto& value : cases) {
        const auto target = full_output_geometry(value);
        const auto geometry = calculate_overlay_geometry(target);
        REQUIRE(geometry.has_value());
        if (!geometry) continue;
        REQUIRE(geometry->desktop_bounds_px == target.client_bounds_px);
        REQUIRE(geometry->clipped_desktop_bounds_px == target.client_bounds_px);
        REQUIRE(geometry->source_crop_px ==
                RectI{0, 0, value.content_size.width, value.content_size.height});
        REQUIRE(geometry->source_size_px == value.content_size);
        REQUIRE(geometry->dpi_scale_x == static_cast<double>(value.dpi) / 96.0);
        REQUIRE(geometry->dpi_scale_y == static_cast<double>(value.dpi) / 96.0);
        const bool hdr = value.color_space == ColorSpace::hdr10_pq ||
                         value.color_space == ColorSpace::hdr_sc_rgb;
        REQUIRE(geometry->tone_map_required == hdr);
        const auto mapped = map_normalized_source_rect({0.25, 0.25, 0.75, 0.75}, *geometry);
        REQUIRE(mapped.has_value());
        if (mapped) {
            REQUIRE(*mapped == RectI{
                value.desktop_bounds.left + value.content_size.width / 4,
                value.desktop_bounds.top + value.content_size.height / 4,
                value.desktop_bounds.left + value.content_size.width * 3 / 4,
                value.desktop_bounds.top + value.content_size.height * 3 / 4,
            });
        }
        single_monitor_rows += value.single_monitor ? 1U : 0U;
    }
    REQUIRE(single_monitor_rows == 4U);

    const TargetGeometry partially_clipped{
        {-2200, 40, 200, 1240},
        {-2100, 100, 100, 1200},
        {-2560, 0, 0, 1440},
        {2560, 1440},
        {"negative-left-partial", {-2560, 0, 0, 1440}, {-2560, 0, 0, 1400},
         144, 144, ColorSpace::sdr_srgb, DisplayRotation::identity, 80.0},
        false,
    };
    const auto clipped = calculate_overlay_geometry(partially_clipped);
    REQUIRE(clipped.has_value());
    if (clipped) {
        REQUIRE(clipped->clipped_desktop_bounds_px == RectI{-2100, 100, 0, 1200});
        REQUIRE(clipped->source_crop_px == RectI{460, 100, 2560, 1200});
    }
}

[[nodiscard]] GameTarget target(const std::uintptr_t native_window) {
    return {native_window, 42U, "synthetic-game.exe", "Synthetic exact-HWND target"};
}

void verify_display_state_lifecycle() {
    auto platform = std::make_unique<SimulatedMediaPlatform>();
    auto* simulation = platform.get();
    MediaBroker broker(std::move(platform));
    REQUIRE(broker.start());
    REQUIRE(broker.select_target(target(0x1001U)));
    REQUIRE(broker.diagnostics().device_generation == 1U);

    const SimulatedDisplayCase single{
        "single-monitor-no-mirror", {0, 0, 1920, 1080}, {1920, 1080},
        144, ColorSpace::sdr_srgb, true};
    auto windowed = full_output_geometry(single);
    windowed.window_bounds_px = {160, 90, 1760, 990};
    windowed.client_bounds_px = windowed.window_bounds_px;
    windowed.captured_desktop_bounds_px = windowed.client_bounds_px;
    windowed.captured_content_px = {1600, 900};
    simulation->emit_target(TargetState::selected, windowed);
    const auto windowed_epoch = broker.diagnostics().geometry_epoch;
    REQUIRE(windowed_epoch > 0U);
    REQUIRE(broker.diagnostics().state == BrokerState::capturing_primary);
    REQUIRE(broker.diagnostics().capture_backend == CaptureBackend::windows_graphics_capture);
    REQUIRE(broker.diagnostics().overlay_capture_excluded);
    REQUIRE(broker.diagnostics().overlay_visuals_allowed);

    // Borderless is represented by exact client/output bounds. There is no
    // claim that this synthetic state is true exclusive fullscreen.
    auto borderless = full_output_geometry(single);
    simulation->emit_target(TargetState::selected, borderless);
    const auto borderless_epoch = broker.diagnostics().geometry_epoch;
    REQUIRE(borderless_epoch > windowed_epoch);

    auto resized = borderless;
    resized.window_bounds_px = {0, 0, 1280, 720};
    resized.client_bounds_px = resized.window_bounds_px;
    resized.captured_desktop_bounds_px = resized.client_bounds_px;
    resized.captured_content_px = {1280, 720};
    simulation->emit_target(TargetState::selected, resized);
    const auto resized_epoch = broker.diagnostics().geometry_epoch;
    REQUIRE(resized_epoch > borderless_epoch);

    resized.minimized = true;
    simulation->emit_target(TargetState::minimized, resized);
    REQUIRE(broker.diagnostics().state == BrokerState::awaiting_target);
    REQUIRE(broker.diagnostics().overlay_backend == OverlayBackend::none);
    REQUIRE(!broker.diagnostics().overlay_capture_excluded);

    resized.minimized = false;
    simulation->emit_target(TargetState::selected, resized);
    REQUIRE(broker.diagnostics().state == BrokerState::capturing_primary);
    REQUIRE(broker.diagnostics().geometry_epoch > resized_epoch);

    const auto before_loss_generation = broker.diagnostics().cancellation_generation;
    simulation->emit_target(TargetState::closed);
    REQUIRE(broker.diagnostics().state == BrokerState::awaiting_target);
    REQUIRE(broker.diagnostics().capture_backend == CaptureBackend::none);
    REQUIRE(broker.select_target(target(0x1002U)));
    simulation->emit_target(TargetState::selected, resized);
    REQUIRE(broker.diagnostics().selected_window == 0x1002U);
    REQUIRE(broker.diagnostics().cancellation_generation > before_loss_generation);
    REQUIRE(broker.diagnostics().capture_backend == CaptureBackend::windows_graphics_capture);

    const auto generation_before_reset = broker.diagnostics().device_generation;
    const auto epoch_before_reset = broker.diagnostics().geometry_epoch;
    simulation->emit_failure({FailureDomain::device, FailureCode::device_reset, true,
                              "simulated display device reset"});
    REQUIRE(broker.diagnostics().state == BrokerState::recovering_device);
    broker.tick(std::chrono::steady_clock::now() + std::chrono::seconds(2));
    REQUIRE(broker.diagnostics().device_generation > generation_before_reset);
    REQUIRE(broker.diagnostics().geometry_epoch > epoch_before_reset);
    REQUIRE(broker.diagnostics().capture_backend == CaptureBackend::windows_graphics_capture);

    simulation->emit_target(TargetState::exclusive_fullscreen);
    REQUIRE(broker.diagnostics().state == BrokerState::degraded_audio_only);
    REQUIRE(broker.diagnostics().target_state == TargetState::exclusive_fullscreen);
    REQUIRE(broker.diagnostics().capture_backend == CaptureBackend::none);
    REQUIRE(broker.diagnostics().overlay_backend == OverlayBackend::none);
    REQUIRE(!broker.diagnostics().overlay_capture_excluded);
    REQUIRE(broker.diagnostics().last_failure.has_value());
    REQUIRE(broker.diagnostics().last_failure &&
            broker.diagnostics().last_failure->code == FailureCode::exclusive_fullscreen);
}

} // namespace

int main() {
    verify_geometry_matrix();
    verify_display_state_lifecycle();
    if (failures != 0) {
        std::cerr << failures << " simulated display matrix requirement(s) failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "SIMULATED display matrix passed: single/multi-monitor, negative origins, "
                 "720p/1080p/1440p/4K/3440x1440/5120x1440, 100/150/200% DPI, SDR/HDR, "
                 "windowed/borderless geometry, resize, minimize/restore, target loss/reselect, "
                 "device reset/recovery, exact-HWND overlay exclusion, and exclusive-fullscreen "
                 "audio-only truth. No physical display mode is claimed by this test.\n";
    return EXIT_SUCCESS;
}
