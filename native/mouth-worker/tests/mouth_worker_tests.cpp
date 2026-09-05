#include "npc/mouth_worker/compositor.hpp"
#include "npc/mouth_worker/worker.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <iostream>
#include <limits>
#include <string_view>
#include <utility>
#include <vector>

namespace {

using namespace npc::mouth;

int failures = 0;

void expect(const bool condition, const std::string_view message) {
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        ++failures;
    }
}

[[nodiscard]] std::uint64_t digest(const std::vector<std::uint8_t>& bytes) {
    std::uint64_t value = 1469598103934665603ULL;
    for (const auto byte : bytes) {
        value ^= byte;
        value *= 1099511628211ULL;
    }
    return value;
}

[[nodiscard]] CpuFrame make_frame(const std::uint64_t sequence,
                                  const Nanoseconds captured_at_ns,
                                  const std::uint32_t width = 320U,
                                  const std::uint32_t height = 180U) {
    CpuFrame frame{};
    frame.lease.schema_version = 1U;
    frame.lease.transport = LeaseTransport::cpu_reference;
    frame.lease.lease_nonce_high = 0x1122334455667788ULL;
    frame.lease.lease_nonce_low = sequence;
    frame.lease.owner_process_id = 100U;
    frame.lease.intended_consumer_process_id = 200U;
    frame.lease.width = width;
    frame.lease.height = height;
    frame.lease.stride_bytes = width * 4U;
    frame.lease.expires_at_ns = captured_at_ns + 100'000'000;
    frame.identity = {sequence, 7U, 11U, captured_at_ns};
    frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) * height);
    for (std::uint32_t y = 0; y < height; ++y) {
        for (std::uint32_t x = 0; x < width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * frame.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            frame.bgra[offset + 0U] = static_cast<std::uint8_t>((x * 3U + y) % 256U);
            frame.bgra[offset + 1U] = static_cast<std::uint8_t>((x + y * 2U) % 256U);
            frame.bgra[offset + 2U] = static_cast<std::uint8_t>((x * 2U + y * 3U) % 256U);
            frame.bgra[offset + 3U] = 255U;
        }
    }
    return frame;
}

[[nodiscard]] WorkItem make_item(const std::uint64_t sequence = 41U,
                                 const Nanoseconds captured_at_ns = 1'000'000'000,
                                 const std::uint64_t generation = 3U) {
    WorkItem item{};
    item.track = {generation, 71U, 101U, 5U};
    item.source = make_frame(sequence, captured_at_ns);
    item.tracking.track = item.track;
    item.tracking.frame = item.source.identity;
    item.tracking.face_bounds = {0.31, 0.2, 0.38, 0.66};
    item.tracking.mouth_bounds = {0.43, 0.58, 0.14, 0.12};
    item.tracking.mouth_landmarks.schema_version = 1U;
    item.tracking.mouth_landmarks.provider_instance_id = 0x50524f5649444552ULL;
    item.tracking.mouth_landmarks.left_corner = {0.442, 0.64, 0.96};
    item.tracking.mouth_landmarks.right_corner = {0.558, 0.654, 0.96};
    item.tracking.mouth_landmarks.upper_lip_center = {0.5, 0.616, 0.95};
    item.tracking.mouth_landmarks.lower_lip_center = {0.5, 0.676, 0.95};
    item.tracking.pose = {4.0, -2.0, 7.0};
    item.tracking.face_confidence = 0.96;
    item.tracking.landmark_confidence = 0.95;
    item.tracking.visibility_ratio = 0.93;
    item.tracking.measured_at_ns = captured_at_ns + 2'000'000;
    item.drive.kind = DriveKind::timed_viseme;
    item.drive.clock.stream_generation = generation;
    item.drive.clock.segment_id = 19U;
    item.drive.clock.sample_count = 1'600U;
    item.drive.clock.sample_rate = 48'000U;
    item.drive.clock.channels = 1U;
    item.drive.clock.playback_at_ns = captured_at_ns;
    item.drive.viseme = Viseme::open_vowel;
    item.drive.viseme_strength = 0.9;
    item.deadline_ns = captured_at_ns + 50'000'000;
    return item;
}

[[nodiscard]] CanonicalMouthPatch make_atlas_patch(const std::uint8_t blue,
                                                   const std::uint8_t green,
                                                   const std::uint8_t red) {
    CanonicalMouthPatch patch{};
    patch.width = 48U;
    patch.height = 32U;
    patch.stride_bytes = patch.width * 4U;
    patch.premultiplied_bgra.resize(
        static_cast<std::size_t>(patch.stride_bytes) * patch.height, 0U);
    for (std::uint32_t y = 0; y < patch.height; ++y) {
        for (std::uint32_t x = 0; x < patch.width; ++x) {
            const double nx = (static_cast<double>(x) + 0.5) /
                                  static_cast<double>(patch.width) * 2.0 - 1.0;
            const double ny = (static_cast<double>(y) + 0.5) /
                                  static_cast<double>(patch.height) * 2.0 - 1.0;
            const double radius = std::sqrt(nx * nx + ny * ny);
            const auto alpha = static_cast<std::uint8_t>(std::lround(
                std::clamp((1.0 - radius) / 0.25, 0.0, 1.0) * 255.0));
            const auto offset = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            patch.premultiplied_bgra[offset + 0U] = static_cast<std::uint8_t>(
                (static_cast<std::uint32_t>(blue) * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 1U] = static_cast<std::uint8_t>(
                (static_cast<std::uint32_t>(green) * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 2U] = static_cast<std::uint8_t>(
                (static_cast<std::uint32_t>(red) * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 3U] = alpha;
        }
    }
    return patch;
}

[[nodiscard]] CharacterMouthAtlas make_character_atlas(const WorkItem& item) {
    CharacterMouthAtlas atlas{};
    atlas.cancellation_generation = item.track.cancellation_generation;
    atlas.actor_id = item.track.actor_id;
    atlas.identity_revision = 17U;
    atlas.states = {
        {coefficients_for_viseme(Viseme::silence), make_atlas_patch(22U, 42U, 78U)},
        {coefficients_for_viseme(Viseme::rounded), make_atlas_patch(56U, 86U, 126U)},
        {coefficients_for_viseme(Viseme::open_vowel), make_atlas_patch(96U, 142U, 204U)},
        {coefficients_for_viseme(Viseme::spread_vowel), make_atlas_patch(42U, 118U, 188U)},
    };
    return atlas;
}

void test_viseme_and_audio_drives() {
    const auto silence = coefficients_for_viseme(Viseme::silence);
    const auto open = coefficients_for_viseme(Viseme::open_vowel, 1.0);
    expect(silence.lip_close == 1.0 && silence.jaw_open == 0.0,
           "silence closes the reference mouth");
    expect(open.jaw_open > 0.8 && open.lower_lip_depress > 0.5,
           "open vowel maps to bounded opening coefficients");

    std::vector<float> quiet(480U, 0.0F);
    const auto quiet_coefficients = coefficients_from_pcm(quiet, 48'000U, 1U);
    expect(quiet_coefficients.lip_close > 0.99 && quiet_coefficients.jaw_open == 0.0,
           "silent PCM fails naturally to a closed mouth");

    std::vector<float> voiced(480U);
    for (std::size_t index = 0; index < voiced.size(); ++index) {
        voiced[index] = static_cast<float>(0.24 * std::sin(
            static_cast<double>(index) * 2.0 * 3.14159265358979323846 * 220.0 / 48'000.0));
    }
    const auto voiced_coefficients = coefficients_from_pcm(voiced, 48'000U, 1U);
    expect(voiced_coefficients.jaw_open > 0.5 && voiced_coefficients.lip_close < 0.2,
           "voiced PCM drives a real causal mouth opening");

    std::vector<float> normalized_speech(1'600U);
    for (std::size_t index = 0; index < normalized_speech.size(); ++index) {
        normalized_speech[index] = static_cast<float>(0.04 * std::sin(
            static_cast<double>(index) * 2.0 * 3.14159265358979323846 * 180.0 / 48'000.0));
    }
    const auto normalized_speech_coefficients =
        coefficients_from_pcm(normalized_speech, 48'000U, 1U);
    expect(normalized_speech_coefficients.jaw_open > 0.45 &&
               normalized_speech_coefficients.lip_close < 0.4,
           "normally normalized hosted speech remains visibly expressive");

    const auto tone = [](const double frequency) {
        std::vector<float> samples(1'600U);
        for (std::size_t index = 0; index < samples.size(); ++index) {
            samples[index] = static_cast<float>(0.12 * std::sin(
                static_cast<double>(index) * 2.0 * 3.14159265358979323846 *
                frequency / 48'000.0));
        }
        return coefficients_from_pcm(samples, 48'000U, 1U);
    };
    const auto rounded_tone = tone(260.0);
    const auto open_tone = tone(780.0);
    const auto spread_tone = tone(2'300.0);
    expect(rounded_tone.pucker > open_tone.pucker + 0.35 &&
               rounded_tone.funnel > open_tone.funnel + 0.35,
           "same-level low formant energy selects a rounded mouth shape");
    expect(open_tone.jaw_open > rounded_tone.jaw_open + 0.12 &&
               open_tone.lower_lip_depress > rounded_tone.lower_lip_depress + 0.2,
           "same-level mid formant energy selects a taller open-vowel shape");
    expect(spread_tone.smile_left > rounded_tone.smile_left + 0.35 &&
               spread_tone.pucker < rounded_tone.pucker - 0.35,
           "same-level bright formant energy selects a wider spread-vowel shape");
}

void test_current_frame_residual_is_bounded_and_premultiplied() {
    auto item = make_item();
    const auto source_before = item.source.bgra;
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.submit(item), "valid item accepted");
    const auto result = worker.process_latest(item.source.identity,
                                              item.source.identity.captured_at_ns + 10'000'000);
    expect(result.has_residual(), "valid exact-frame work produces a residual");
    expect(item.source.bgra == source_before, "source frame remains immutable");
    expect(result.residual.source_frame == item.source.identity,
           "residual preserves exact source identity");
    expect(result.residual.track == item.track, "residual preserves exact actor/track epoch");
    expect(result.residual.residual_lease.lease_nonce_low !=
               result.residual.source_lease.lease_nonce_low &&
               result.residual.residual_lease.width == result.residual.width &&
               result.residual.residual_lease.height == result.residual.height &&
               result.residual.residual_lease.owner_process_id ==
                   item.source.lease.intended_consumer_process_id,
           "residual has a distinct, reverse-direction protocol lease");
    expect(result.residual.normalized_bounds.width <= 0.45 &&
               result.residual.normalized_bounds.height <= 0.35 &&
               result.residual.normalized_bounds.width * result.residual.normalized_bounds.height <= 0.12,
           "residual is bounded to the hard mouth region ceilings");

    bool saw_transparent = false;
    bool saw_strong_core = false;
    for (std::size_t offset = 0; offset < result.residual.premultiplied_bgra.size(); offset += 4U) {
        const auto alpha = result.residual.premultiplied_bgra[offset + 3U];
        saw_transparent = saw_transparent || alpha == 0U;
        saw_strong_core = saw_strong_core || alpha >= 224U;
        expect(result.residual.premultiplied_bgra[offset + 0U] <= alpha &&
                   result.residual.premultiplied_bgra[offset + 1U] <= alpha &&
                   result.residual.premultiplied_bgra[offset + 2U] <= alpha,
               "every residual pixel obeys premultiplied alpha");
    }
    expect(saw_transparent && saw_strong_core,
           "mouth mask has a transparent exterior and strongly covered core");

    const auto composited = composite_over_source(item.source, result.residual);
    expect(composited != item.source.bgra, "reference compositor changes the mouth presentation");
    expect(item.source.bgra == source_before, "compositing helper still leaves source untouched");
    const auto patch_left = static_cast<std::uint32_t>(std::llround(
        result.residual.normalized_bounds.x * item.source.lease.width));
    const auto patch_top = static_cast<std::uint32_t>(std::llround(
        result.residual.normalized_bounds.y * item.source.lease.height));
    std::size_t changed_inside = 0U;
    for (std::uint32_t y = 0; y < item.source.lease.height; ++y) {
        for (std::uint32_t x = 0; x < item.source.lease.width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const bool inside = x >= patch_left && y >= patch_top &&
                                x < patch_left + result.residual.width &&
                                y < patch_top + result.residual.height;
            const bool changed = !std::equal(composited.begin() + static_cast<std::ptrdiff_t>(offset),
                                             composited.begin() + static_cast<std::ptrdiff_t>(offset + 4U),
                                             item.source.bgra.begin() +
                                                 static_cast<std::ptrdiff_t>(offset));
            expect(inside || !changed, "pixels outside the mouth rectangle remain byte-identical");
            changed_inside += inside && changed ? 1U : 0U;
        }
    }
    expect(changed_inside > 0U, "at least one pixel changes inside the mouth rectangle");
}

void test_pcm_fallback_preserves_source_colour_envelope() {
    auto item = make_item();
    for (std::size_t offset = 0; offset < item.source.bgra.size(); offset += 4U) {
        item.source.bgra[offset + 0U] = 80U;
        item.source.bgra[offset + 1U] = 110U;
        item.source.bgra[offset + 2U] = 150U;
        item.source.bgra[offset + 3U] = 255U;
    }

    const auto residual = compose_current_frame_residual(
        item.source, item.track, item.tracking,
        coefficients_for_viseme(Viseme::open_vowel, 1.0),
        item.source.identity.captured_at_ns + 4'000'000);
    expect(!residual.premultiplied_bgra.empty(),
           "procedural fallback produces a residual for valid tracked lips");
    const auto composited = composite_over_source(item.source, residual);

    std::uint8_t minimum_blue = 255U;
    std::uint8_t minimum_green = 255U;
    std::uint8_t minimum_red = 255U;
    std::uint8_t maximum_blue = 0U;
    std::uint8_t maximum_green = 0U;
    std::uint8_t maximum_red = 0U;
    for (std::size_t offset = 0; offset < composited.size(); offset += 4U) {
        minimum_blue = std::min(minimum_blue, composited[offset + 0U]);
        minimum_green = std::min(minimum_green, composited[offset + 1U]);
        minimum_red = std::min(minimum_red, composited[offset + 2U]);
        maximum_blue = std::max(maximum_blue, composited[offset + 0U]);
        maximum_green = std::max(maximum_green, composited[offset + 1U]);
        maximum_red = std::max(maximum_red, composited[offset + 2U]);
    }

    expect(minimum_blue >= 46U && minimum_green >= 63U && minimum_red >= 86U,
           "procedural PCM motion does not paint a detached near-black cavity");
    expect(maximum_blue <= 96U && maximum_green <= 126U && maximum_red <= 166U,
           "procedural PCM motion does not invent a bright teeth strip");
}

void test_pcm_fallback_keeps_cavity_below_upper_lip() {
    auto item = make_item();
    for (std::size_t offset = 0; offset < item.source.bgra.size(); offset += 4U) {
        item.source.bgra[offset + 0U] = 80U;
        item.source.bgra[offset + 1U] = 110U;
        item.source.bgra[offset + 2U] = 150U;
        item.source.bgra[offset + 3U] = 255U;
    }

    constexpr std::uint32_t mouth_left = 138U;
    constexpr std::uint32_t mouth_right = 181U;
    for (std::uint32_t y = 110U; y <= 118U; ++y) {
        for (std::uint32_t x = mouth_left; x <= mouth_right; ++x) {
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            item.source.bgra[offset + 0U] = 80U;
            item.source.bgra[offset + 1U] = 72U;
            item.source.bgra[offset + 2U] = 132U;
        }
    }
    for (std::uint32_t y = 120U; y <= 124U; ++y) {
        for (std::uint32_t x = mouth_left; x <= mouth_right; ++x) {
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            item.source.bgra[offset + 0U] = 90U;
            item.source.bgra[offset + 1U] = 82U;
            item.source.bgra[offset + 2U] = 145U;
        }
    }
    for (std::uint32_t x = mouth_left; x <= mouth_right; ++x) {
        const auto offset = static_cast<std::size_t>(119U) * item.source.lease.stride_bytes +
                            static_cast<std::size_t>(x) * 4U;
        item.source.bgra[offset + 0U] = 36U;
        item.source.bgra[offset + 1U] = 30U;
        item.source.bgra[offset + 2U] = 48U;
    }

    const auto residual = compose_current_frame_residual(
        item.source, item.track, item.tracking,
        coefficients_for_viseme(Viseme::open_vowel, 1.0),
        item.source.identity.captured_at_ns + 4'000'000);
    const auto composited = composite_over_source(item.source, residual);

    std::size_t darkened_upper_lip_pixels = 0U;
    for (std::uint32_t y = 111U; y <= 117U; ++y) {
        for (std::uint32_t x = 145U; x <= 174U; ++x) {
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const auto source_luma =
                (static_cast<std::uint32_t>(item.source.bgra[offset + 2U]) * 299U +
                 static_cast<std::uint32_t>(item.source.bgra[offset + 1U]) * 587U +
                 static_cast<std::uint32_t>(item.source.bgra[offset + 0U]) * 114U) / 1'000U;
            const auto output_luma =
                (static_cast<std::uint32_t>(composited[offset + 2U]) * 299U +
                 static_cast<std::uint32_t>(composited[offset + 1U]) * 587U +
                 static_cast<std::uint32_t>(composited[offset + 0U]) * 114U) / 1'000U;
            darkened_upper_lip_pixels += source_luma > output_luma + 10U ? 1U : 0U;
        }
    }
    expect(darkened_upper_lip_pixels == 0U,
           "procedural cavity begins at the source contact seam, not inside the upper lip");

    std::size_t changed_lower_mouth_pixels = 0U;
    for (std::uint32_t y = 119U; y <= 135U; ++y) {
        for (std::uint32_t x = 145U; x <= 174U; ++x) {
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const auto channel_delta =
                std::abs(static_cast<int>(composited[offset + 0U]) -
                         static_cast<int>(item.source.bgra[offset + 0U])) +
                std::abs(static_cast<int>(composited[offset + 1U]) -
                         static_cast<int>(item.source.bgra[offset + 1U])) +
                std::abs(static_cast<int>(composited[offset + 2U]) -
                         static_cast<int>(item.source.bgra[offset + 2U]));
            changed_lower_mouth_pixels += channel_delta > 15 ? 1U : 0U;
        }
    }
    expect(changed_lower_mouth_pixels >= 20U,
           "upper-lip protection still permits visible lower-jaw motion");
}

void test_full_contour_visemes_have_distinct_geometry() {
    auto item = make_item();
    auto& mouth = item.tracking.mouth_landmarks;
    mouth.schema_version = 2U;
    mouth.contour_points = static_cast<std::uint32_t>(mouth.contour.size());
    mouth.contour = {{
        {0.442, 0.646, 0.96}, {0.458, 0.624, 0.96},
        {0.482, 0.612, 0.96}, {0.518, 0.612, 0.96},
        {0.542, 0.624, 0.96}, {0.558, 0.646, 0.96},
        {0.542, 0.671, 0.96}, {0.518, 0.682, 0.96},
        {0.482, 0.682, 0.96}, {0.458, 0.671, 0.96},
        {0.442, 0.649, 0.96}, {0.465, 0.641, 0.96},
        {0.500, 0.638, 0.96}, {0.535, 0.641, 0.96},
        {0.558, 0.649, 0.96}, {0.535, 0.658, 0.96},
        {0.500, 0.662, 0.96}, {0.465, 0.658, 0.96},
    }};
    mouth.left_corner = mouth.contour[10U];
    mouth.right_corner = mouth.contour[14U];
    mouth.upper_lip_center = {0.500, 0.640, 0.96};
    mouth.lower_lip_center = {0.500, 0.659, 0.96};

    const auto render = [&](const Viseme viseme) {
        return compose_current_frame_residual(
            item.source, item.track, item.tracking,
            coefficients_for_viseme(viseme, 1.0),
            item.source.identity.captured_at_ns + 4'000'000);
    };
    const auto open = render(Viseme::open_vowel);
    const auto rounded = render(Viseme::rounded);
    const auto spread = render(Viseme::spread_vowel);
    const auto bilabial = render(Viseme::bilabial);
    expect(!open.premultiplied_bgra.empty() && !rounded.premultiplied_bgra.empty() &&
               !spread.premultiplied_bgra.empty() &&
               !bilabial.premultiplied_bgra.empty(),
           "all principal full-contour viseme families produce bounded residuals");
    expect(digest(open.premultiplied_bgra) != digest(rounded.premultiplied_bgra) &&
               digest(rounded.premultiplied_bgra) != digest(spread.premultiplied_bgra) &&
               digest(spread.premultiplied_bgra) != digest(bilabial.premultiplied_bgra),
           "open, rounded, spread, and bilabial visemes remain visually separable");

    const auto opaque_extent = [](const ResidualPatch& patch) {
        std::uint32_t minimum_x = patch.width;
        std::uint32_t minimum_y = patch.height;
        std::uint32_t maximum_x{};
        std::uint32_t maximum_y{};
        bool found = false;
        for (std::uint32_t y = 0; y < patch.height; ++y) {
            for (std::uint32_t x = 0; x < patch.width; ++x) {
                const auto alpha = patch.premultiplied_bgra[
                    static_cast<std::size_t>(y) * patch.stride_bytes +
                    static_cast<std::size_t>(x) * 4U + 3U];
                if (alpha < 64U) continue;
                found = true;
                minimum_x = std::min(minimum_x, x);
                minimum_y = std::min(minimum_y, y);
                maximum_x = std::max(maximum_x, x);
                maximum_y = std::max(maximum_y, y);
            }
        }
        return std::pair{
            found ? maximum_x - minimum_x + 1U : 0U,
            found ? maximum_y - minimum_y + 1U : 0U,
        };
    };
    const auto open_extent = opaque_extent(open);
    const auto rounded_extent = opaque_extent(rounded);
    expect(open_extent.first > 0U && open_extent.second > 0U &&
               rounded_extent.first > 0U && rounded_extent.second > 0U,
           "fixed skin support contains both open and rounded articulation");
}

void test_atlas_residual_preserves_source_lips_and_binds_to_current_frame() {
    const auto item = make_item();
    const auto closed = make_atlas_patch(30U, 50U, 90U);
    const auto open = make_atlas_patch(80U, 140U, 210U);
    const auto closed_coefficients = coefficients_for_viseme(Viseme::silence);
    const auto open_coefficients = coefficients_for_viseme(Viseme::open_vowel, 1.0);
    const auto closed_result = compose_atlas_residual(
        item.source, item.track, item.tracking, closed,
        closed_coefficients,
        item.source.identity.captured_at_ns + 4'000'000);
    const auto open_result = compose_atlas_residual(
        item.source, item.track, item.tracking, open,
        open_coefficients,
        item.source.identity.captured_at_ns + 4'000'000);
    expect(!closed_result.premultiplied_bgra.empty() &&
               !open_result.premultiplied_bgra.empty(),
           "valid atlas states produce current-frame residuals");
    expect(closed_result.source_frame == item.source.identity &&
               closed_result.track == item.track &&
               closed_result.normalized_bounds.x <= item.tracking.mouth_bounds.x &&
               closed_result.normalized_bounds.right() >= item.tracking.mouth_bounds.right() &&
               closed_result.normalized_bounds.y <= item.tracking.mouth_bounds.y &&
               closed_result.normalized_bounds.bottom() >= item.tracking.mouth_bounds.bottom(),
           "atlas residual retains exact frame, track, and bounded mouth geometry");
    expect(digest(closed_result.premultiplied_bgra) !=
               digest(open_result.premultiplied_bgra),
           "closed and open coefficient geometry remain visually distinct");
    for (std::size_t offset = 0; offset < open_result.premultiplied_bgra.size(); offset += 4U) {
        const auto alpha = open_result.premultiplied_bgra[offset + 3U];
        expect(open_result.premultiplied_bgra[offset + 0U] <= alpha &&
                   open_result.premultiplied_bgra[offset + 1U] <= alpha &&
                   open_result.premultiplied_bgra[offset + 2U] <= alpha,
               "source-preserving atlas pixels preserve premultiplied alpha");
    }

    const auto composited = composite_over_source(item.source, open_result);
    const auto left = static_cast<std::uint32_t>(std::llround(
        open_result.normalized_bounds.x * item.source.lease.width));
    const auto top = static_cast<std::uint32_t>(std::llround(
        open_result.normalized_bounds.y * item.source.lease.height));
    for (std::uint32_t y = 0; y < item.source.lease.height; ++y) {
        for (std::uint32_t x = 0; x < item.source.lease.width; ++x) {
            const bool inside = x >= left && y >= top &&
                                x < left + open_result.width && y < top + open_result.height;
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const bool changed = !std::equal(
                composited.begin() + static_cast<std::ptrdiff_t>(offset),
                composited.begin() + static_cast<std::ptrdiff_t>(offset + 4U),
                item.source.bgra.begin() + static_cast<std::ptrdiff_t>(offset));
            expect(inside || !changed,
                   "atlas compositing leaves every pixel outside the mouth rectangle identical");
            if (inside) {
                const auto patch_offset =
                    static_cast<std::size_t>(y - top) * open_result.stride_bytes +
                    static_cast<std::size_t>(x - left) * 4U;
                expect(open_result.premultiplied_bgra[patch_offset + 3U] != 0U || !changed,
                       "atlas compositing leaves zero-alpha current-frame pixels byte-identical");
            }
        }
    }

    const double source_width = static_cast<double>(item.source.lease.width);
    const double source_height = static_cast<double>(item.source.lease.height);
    const double left_corner_x = item.tracking.mouth_landmarks.left_corner.x * source_width;
    const double left_corner_y = item.tracking.mouth_landmarks.left_corner.y * source_height;
    const double right_corner_x = item.tracking.mouth_landmarks.right_corner.x * source_width;
    const double right_corner_y = item.tracking.mouth_landmarks.right_corner.y * source_height;
    const double delta_x = right_corner_x - left_corner_x;
    const double delta_y = right_corner_y - left_corner_y;
    const double mouth_width = std::hypot(delta_x, delta_y);
    const double mouth_center_x = (left_corner_x + right_corner_x) * 0.5;
    const double mouth_center_y =
        (item.tracking.mouth_landmarks.upper_lip_center.y +
         item.tracking.mouth_landmarks.lower_lip_center.y) * 0.5 * source_height;
    const double roll = std::atan2(delta_y, delta_x);
    const double cosine = std::cos(roll);
    const double sine = std::sin(roll);
    const double canonical_width_pixels = mouth_width * 1.34;
    const double canonical_height_pixels = canonical_width_pixels * 0.625;
    std::size_t atlas_pixels = 0U;
    std::size_t outer_lip_pixels = 0U;
    std::size_t atlas_pixels_outside_canonical_patch = 0U;
    for (std::uint32_t y = 0U; y < open_result.height; ++y) {
        for (std::uint32_t x = 0U; x < open_result.width; ++x) {
            const double frame_x = static_cast<double>(left + x) + 0.5;
            const double frame_y = static_cast<double>(top + y) + 0.5;
            const double frame_delta_x = frame_x - mouth_center_x;
            const double frame_delta_y = frame_y - mouth_center_y;
            const double canonical_x =
                (cosine * frame_delta_x + sine * frame_delta_y) /
                (canonical_width_pixels * 0.5);
            const double canonical_y =
                (-sine * frame_delta_x + cosine * frame_delta_y) /
                (canonical_height_pixels * 0.5);
            const auto offset = static_cast<std::size_t>(y) * open_result.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const bool atlas_pixel = open_result.premultiplied_bgra[offset + 3U] > 0U;
            atlas_pixels += atlas_pixel ? 1U : 0U;
            const bool outside_oral_aperture =
                std::abs(canonical_x) >= 0.81 || canonical_y <= -0.25 ||
                canonical_y >= 0.29;
            outer_lip_pixels += atlas_pixel && outside_oral_aperture ? 1U : 0U;
            const bool outside_canonical_patch =
                std::abs(canonical_x) > 1.01 || std::abs(canonical_y) > 1.01;
            atlas_pixels_outside_canonical_patch +=
                atlas_pixel && outside_canonical_patch ? 1U : 0U;
        }
    }
    expect(atlas_pixels > 0U,
           "the selected observation contributes a bounded photographed mouth texture");
    expect(outer_lip_pixels > 0U,
           "identity-bound atlas texture includes the lips around the oral aperture");
    expect(atlas_pixels_outside_canonical_patch == 0U,
           "atlas alpha cannot modify pixels outside its canonical mouth patch");

    auto malformed = open;
    malformed.premultiplied_bgra[0] = 255U;
    malformed.premultiplied_bgra[3] = 0U;
    expect(compose_atlas_residual(item.source, item.track, item.tracking, malformed,
                                  open_coefficients,
                                  item.source.identity.captured_at_ns + 4'000'000)
               .premultiplied_bgra.empty(),
           "atlas compositor rejects non-premultiplied enrollment artifacts");
}

void test_observed_patch_extraction_preserves_real_source_pixels() {
    const auto item = make_item();
    const auto patch = extract_canonical_mouth_patch(item.source, item.tracking, 96U, 60U);
    expect(patch.width == 96U && patch.height == 60U &&
               patch.stride_bytes == 96U * 4U &&
               patch.enrolled_pose.yaw == item.tracking.pose.yaw,
           "observed mouth extraction records canonical geometry and enrolled pose");
    bool saw_transparent = false;
    bool saw_opaque = false;
    bool saw_source_colour = false;
    for (std::size_t offset = 0U; offset < patch.premultiplied_bgra.size(); offset += 4U) {
        const auto alpha = patch.premultiplied_bgra[offset + 3U];
        saw_transparent = saw_transparent || alpha == 0U;
        saw_opaque = saw_opaque || alpha >= 250U;
        saw_source_colour = saw_source_colour ||
            patch.premultiplied_bgra[offset + 0U] !=
                patch.premultiplied_bgra[offset + 1U] ||
            patch.premultiplied_bgra[offset + 1U] !=
                patch.premultiplied_bgra[offset + 2U];
        expect(patch.premultiplied_bgra[offset + 0U] <= alpha &&
                   patch.premultiplied_bgra[offset + 1U] <= alpha &&
                   patch.premultiplied_bgra[offset + 2U] <= alpha,
               "observed atlas extraction remains premultiplied");
    }
    expect(saw_transparent && saw_opaque,
           "observed atlas extraction has a feathered curved support");
    expect(saw_source_colour,
           "observed atlas extraction retains source colour instead of a synthetic cavity");

    auto wrong_frame = item.tracking;
    ++wrong_frame.frame.sequence;
    expect(extract_canonical_mouth_patch(item.source, wrong_frame).premultiplied_bgra.empty(),
           "observed atlas extraction rejects tracking from another frame");
}

void test_worker_uses_identity_bound_atlas_and_clears_on_cancel() {
    auto item = make_item();
    item.drive.viseme_strength = 1.0;
    auto atlas = make_character_atlas(item);
    const auto expected = compose_atlas_residual(
        item.source, item.track, item.tracking,
        atlas.states[2U].appearance,
        coefficients_for_viseme(Viseme::open_vowel, 1.0),
        item.source.identity.captured_at_ns + 10'000'000);

    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.install_atlas(atlas), "valid identity atlas installs atomically");
    expect(worker.submit(item), "atlas-backed work enters the bounded queue");
    const auto result = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(result.has_residual(), "identity atlas produces a current-frame residual");
    expect(digest(result.residual.premultiplied_bgra) ==
               digest(expected.premultiplied_bgra),
           "exact provider viseme selects the matching observed atlas state");
    expect(result.residual.coefficients.jaw_open > 0.8,
           "atlas residual keeps the authoritative drive coefficients");

    auto wrong_generation = atlas;
    ++wrong_generation.cancellation_generation;
    expect(!worker.install_atlas(std::move(wrong_generation)),
           "cross-generation atlas installation is rejected");
    auto malformed = atlas;
    malformed.states[1U].appearance.premultiplied_bgra[0U] = 255U;
    malformed.states[1U].appearance.premultiplied_bgra[3U] = 0U;
    expect(!worker.install_atlas(std::move(malformed)),
           "non-premultiplied atlas installation is rejected");

    auto mismatched_representation = atlas;
    mismatched_representation.states[0].appearance.representation =
        MouthPatchRepresentation::normalized_oral_interior_v1;
    expect(!worker.install_atlas(mismatched_representation),
           "oral-normalized pixels cannot enter a legacy full-lip atlas");
    mismatched_representation.schema_version = 2U;
    expect(!worker.install_atlas(mismatched_representation),
           "mixed oral and full-lip representations are rejected");
    auto unknown_representation = atlas;
    unknown_representation.states[0].appearance.representation =
        static_cast<MouthPatchRepresentation>(255U);
    expect(!worker.install_atlas(unknown_representation),
           "unknown representation is rejected before rendering");

    const auto next_generation = item.track.cancellation_generation + 1U;
    expect(worker.cancel_to(next_generation),
           "cancellation advances and synchronously destroys the installed atlas");
    item.track.cancellation_generation = next_generation;
    item.tracking.track = item.track;
    item.drive.clock.stream_generation = next_generation;
    expect(worker.submit(item), "new-generation work remains usable after atlas cancellation");
    const auto fallback = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(fallback.has_residual() &&
               digest(fallback.residual.premultiplied_bgra) !=
                   digest(result.residual.premultiplied_bgra),
           "cancelled atlas pixels cannot leak into the new generation");
}

void test_worker_never_applies_an_atlas_to_another_actor() {
    auto item = make_item();
    auto atlas = make_character_atlas(item);
    ReferenceMouthWorker atlas_worker(item.track.cancellation_generation);
    expect(atlas_worker.install_atlas(atlas), "actor-bound atlas installs");

    ++item.track.actor_id;
    item.tracking.track = item.track;
    expect(atlas_worker.submit(item), "other actor work remains valid input");
    const auto guarded = atlas_worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);

    ReferenceMouthWorker baseline(item.track.cancellation_generation);
    expect(baseline.submit(item), "other actor baseline work enters the queue");
    const auto expected_fallback = baseline.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(guarded.has_residual() && expected_fallback.has_residual() &&
               digest(guarded.residual.premultiplied_bgra) ==
                   digest(expected_fallback.residual.premultiplied_bgra),
           "an actor mismatch never renders identity pixels from the installed atlas");
}

void test_timed_viseme_does_not_wait_for_atlas_dwell() {
    auto item = make_item();
    item.drive.viseme = Viseme::open_vowel;
    item.drive.viseme_strength = 1.0;
    auto atlas = make_character_atlas(item);
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.install_atlas(atlas), "timed cue test atlas installs");
    expect(worker.submit(item), "first atlas state enters the queue");
    const auto first = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(first.has_residual(), "first atlas state renders");

    const auto advance = [](WorkItem value) {
        ++value.source.identity.sequence;
        value.source.identity.captured_at_ns += 33'000'000;
        value.source.lease.expires_at_ns = value.source.identity.captured_at_ns + 500'000'000;
        value.tracking.frame = value.source.identity;
        value.tracking.measured_at_ns = value.source.identity.captured_at_ns + 2'000'000;
        value.drive.clock.first_sample_index += value.drive.clock.sample_count;
        value.drive.clock.playback_sample_index = value.drive.clock.first_sample_index;
        value.drive.clock.playback_at_ns = value.source.identity.captured_at_ns;
        value.deadline_ns = value.source.identity.captured_at_ns + 50'000'000;
        return value;
    };
    item = advance(item);
    item.drive.viseme = Viseme::spread_vowel;
    const auto spread_coefficients = coefficients_for_viseme(Viseme::spread_vowel, 1.0);
    expect(worker.submit(item), "first competing atlas state enters the queue");
    const auto held = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(held.has_residual() &&
               held.residual.coefficients.smile_left >
                   first.residual.coefficients.smile_left &&
               held.residual.coefficients.smile_left < spread_coefficients.smile_left,
           "a clock-bound speech cue starts moving toward its shape on its first frame");

    item = advance(item);
    expect(worker.submit(item), "second competing atlas state enters the queue");
    const auto switched = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(switched.has_residual() &&
               switched.residual.coefficients.smile_left >
                   held.residual.coefficients.smile_left &&
               digest(switched.residual.premultiplied_bgra) !=
                   digest(held.residual.premultiplied_bgra),
           "a sustained speech cue continues converging without a frame-count dwell");
}

void test_timed_atlas_transition_is_continuous_on_its_first_frame() {
    auto item = make_item(240U, 3'000'000'000);
    item.drive.viseme = Viseme::open_vowel;
    item.drive.viseme_strength = 1.0;
    auto atlas = make_character_atlas(item);
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.install_atlas(atlas), "transition test atlas installs");
    expect(worker.submit(item), "transition origin enters the queue");
    const auto origin = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(origin.has_residual(), "transition origin renders");

    ++item.source.identity.sequence;
    item.source.identity.captured_at_ns += 33'000'000;
    item.source.lease.lease_nonce_low = item.source.identity.sequence;
    item.source.lease.expires_at_ns = item.source.identity.captured_at_ns + 100'000'000;
    item.tracking.frame = item.source.identity;
    item.tracking.measured_at_ns = item.source.identity.captured_at_ns + 2'000'000;
    item.drive.clock.first_sample_index += item.drive.clock.sample_count;
    item.drive.clock.playback_sample_index = item.drive.clock.first_sample_index;
    item.drive.clock.playback_at_ns = item.source.identity.captured_at_ns;
    item.drive.viseme = Viseme::spread_vowel;
    item.deadline_ns = item.source.identity.captured_at_ns + 50'000'000;

    const auto spread_coefficients = coefficients_for_viseme(Viseme::spread_vowel, 1.0);
    const auto hard_switched = compose_atlas_residual(
        item.source, item.track, item.tracking, atlas.states[3U].appearance,
        spread_coefficients, item.source.identity.captured_at_ns + 10'000'000);
    const auto held_open = compose_atlas_residual(
        item.source, item.track, item.tracking, atlas.states[2U].appearance,
        spread_coefficients, item.source.identity.captured_at_ns + 10'000'000);
    expect(worker.submit(item), "transition destination enters the queue");
    const auto transition = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(transition.has_residual(), "transition destination renders");
    expect(digest(transition.residual.premultiplied_bgra) !=
               digest(hard_switched.premultiplied_bgra),
           "the first transition frame is not a hard texture switch");
    expect(digest(transition.residual.premultiplied_bgra) !=
               digest(held_open.premultiplied_bgra),
           "the first transition frame advances toward the new articulation");
}

void test_bilabial_closure_remains_immediate_during_coarticulation() {
    auto item = make_item(250U, 3'500'000'000);
    // Model a gameplay frame captured during unrelated source dialogue: the
    // source mouth is visibly open before the replacement speech reaches M/B/P.
    for (std::uint32_t y = 111U; y <= 121U; ++y) {
        for (std::uint32_t x = 147U; x <= 173U; ++x) {
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            item.source.bgra[offset + 0U] = 7U;
            item.source.bgra[offset + 1U] = 5U;
            item.source.bgra[offset + 2U] = 11U;
            item.source.bgra[offset + 3U] = 255U;
        }
    }
    item.drive.viseme = Viseme::open_vowel;
    item.drive.viseme_strength = 1.0;
    auto atlas = make_character_atlas(item);
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.install_atlas(atlas), "bilabial transition atlas installs");
    expect(worker.submit(item), "pre-bilabial vowel enters the queue");
    const auto vowel = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(vowel.has_residual(), "pre-bilabial vowel renders");

    ++item.source.identity.sequence;
    item.source.identity.captured_at_ns += 33'000'000;
    item.source.lease.lease_nonce_low = item.source.identity.sequence;
    item.source.lease.expires_at_ns = item.source.identity.captured_at_ns + 100'000'000;
    item.tracking.frame = item.source.identity;
    item.tracking.measured_at_ns = item.source.identity.captured_at_ns + 2'000'000;
    item.drive.clock.first_sample_index += item.drive.clock.sample_count;
    item.drive.clock.playback_sample_index = item.drive.clock.first_sample_index;
    item.drive.clock.playback_at_ns = item.source.identity.captured_at_ns;
    item.drive.viseme = Viseme::bilabial;
    item.deadline_ns = item.source.identity.captured_at_ns + 50'000'000;
    const auto expected_coefficients = coefficients_for_viseme(Viseme::bilabial, 1.0);
    const auto expected_current_frame_closure = compose_current_frame_residual(
        item.source, item.track, item.tracking, expected_coefficients,
        item.source.identity.captured_at_ns + 10'000'000);
    expect(worker.submit(item), "bilabial closure enters the queue");
    const auto closure = worker.process_latest(
        item.source.identity, item.source.identity.captured_at_ns + 10'000'000);
    expect(closure.has_residual() && closure.residual.coefficients.jaw_open <= 0.04 &&
               closure.residual.coefficients.lip_close >= 0.88,
           "bilabial coefficients reach exact contact on their first frame");
    expect(closure.has_residual() &&
               digest(closure.residual.premultiplied_bgra) ==
                   digest(expected_current_frame_closure.premultiplied_bgra),
           "bilabial contact uses the current-frame closure instead of an atlas texture");
    expect(closure.has_residual() &&
               composite_over_source(item.source, closure.residual) != item.source.bgra,
           "bilabial contact actively closes an already-open source mouth");
}

void test_atlas_coarticulation_is_time_based_and_segment_bound() {
    const auto advance = [](WorkItem value, const Nanoseconds elapsed) {
        ++value.source.identity.sequence;
        value.source.identity.captured_at_ns += elapsed;
        value.source.lease.lease_nonce_low = value.source.identity.sequence;
        value.source.lease.expires_at_ns = value.source.identity.captured_at_ns + 100'000'000;
        value.tracking.frame = value.source.identity;
        value.tracking.measured_at_ns = value.source.identity.captured_at_ns + 2'000'000;
        value.drive.clock.first_sample_index += value.drive.clock.sample_count;
        value.drive.clock.playback_sample_index = value.drive.clock.first_sample_index;
        value.drive.clock.playback_at_ns = value.source.identity.captured_at_ns;
        value.deadline_ns = value.source.identity.captured_at_ns + 50'000'000;
        return value;
    };
    auto origin = make_item(260U, 4'000'000'000);
    origin.drive.viseme = Viseme::open_vowel;
    origin.drive.viseme_strength = 1.0;
    const auto atlas = make_character_atlas(origin);

    ReferenceMouthWorker one_step(origin.track.cancellation_generation);
    expect(one_step.install_atlas(atlas), "one-step timing atlas installs");
    expect(one_step.submit(origin), "one-step origin enters the queue");
    (void)one_step.process_latest(
        origin.source.identity, origin.source.identity.captured_at_ns + 10'000'000);
    auto at_32_ms = advance(origin, 32'000'000);
    at_32_ms.drive.viseme = Viseme::spread_vowel;
    expect(one_step.submit(at_32_ms), "one-step transition enters the queue");
    const auto one_step_result = one_step.process_latest(
        at_32_ms.source.identity, at_32_ms.source.identity.captured_at_ns + 10'000'000);

    ReferenceMouthWorker two_steps(origin.track.cancellation_generation);
    expect(two_steps.install_atlas(atlas), "two-step timing atlas installs");
    expect(two_steps.submit(origin), "two-step origin enters the queue");
    (void)two_steps.process_latest(
        origin.source.identity, origin.source.identity.captured_at_ns + 10'000'000);
    auto at_16_ms = advance(origin, 16'000'000);
    at_16_ms.drive.viseme = Viseme::spread_vowel;
    expect(two_steps.submit(at_16_ms), "first half-step transition enters the queue");
    (void)two_steps.process_latest(
        at_16_ms.source.identity, at_16_ms.source.identity.captured_at_ns + 10'000'000);
    auto second_16_ms = advance(at_16_ms, 16'000'000);
    expect(two_steps.submit(second_16_ms), "second half-step transition enters the queue");
    const auto two_step_result = two_steps.process_latest(
        second_16_ms.source.identity,
        second_16_ms.source.identity.captured_at_ns + 10'000'000);

    std::uint8_t maximum_pixel_delta{};
    if (one_step_result.has_residual() && two_step_result.has_residual() &&
        one_step_result.residual.premultiplied_bgra.size() ==
            two_step_result.residual.premultiplied_bgra.size()) {
        for (std::size_t index = 0U;
             index < one_step_result.residual.premultiplied_bgra.size(); ++index) {
            const auto first = one_step_result.residual.premultiplied_bgra[index];
            const auto second = two_step_result.residual.premultiplied_bgra[index];
            maximum_pixel_delta = std::max<std::uint8_t>(
                maximum_pixel_delta,
                static_cast<std::uint8_t>(first > second ? first - second : second - first));
        }
    }
    expect(one_step_result.has_residual() && two_step_result.has_residual() &&
               std::abs(one_step_result.residual.coefficients.smile_left -
                        two_step_result.residual.coefficients.smile_left) < 1e-9 &&
               maximum_pixel_delta <= 2U,
           "coarticulation follows elapsed playback time rather than frame count");

    auto new_segment = advance(second_16_ms, 33'000'000);
    ++new_segment.drive.clock.segment_id;
    new_segment.drive.clock.first_sample_index = 0U;
    new_segment.drive.clock.playback_sample_index = 0U;
    new_segment.drive.viseme = Viseme::rounded;
    const auto rounded_coefficients = coefficients_for_viseme(Viseme::rounded, 1.0);
    const auto reset_expected = compose_atlas_residual(
        new_segment.source, new_segment.track, new_segment.tracking,
        atlas.states[1U].appearance, rounded_coefficients,
        new_segment.source.identity.captured_at_ns + 10'000'000);
    expect(two_steps.submit(new_segment), "new-segment articulation enters the queue");
    const auto reset = two_steps.process_latest(
        new_segment.source.identity, new_segment.source.identity.captured_at_ns + 10'000'000);
    expect(reset.has_residual() &&
               digest(reset.residual.premultiplied_bgra) ==
                   digest(reset_expected.premultiplied_bgra),
           "a new audio segment cannot inherit the preceding texture blend");
}

void test_queue_depth_one() {
    auto older = make_item(80U);
    auto newer = make_item(81U);
    ReferenceMouthWorker worker(older.track.cancellation_generation);
    expect(worker.submit(std::move(older)), "older work accepted");
    expect(worker.submit(newer), "newer work accepted");
    expect(worker.stats().replaced_before_processing == 1U, "queue replaces its only pending slot");
    const auto result = worker.process_latest(newer.source.identity,
                                              newer.source.identity.captured_at_ns + 10'000'000);
    expect(result.has_residual() && result.residual.source_frame.sequence == 81U,
           "only newest frame can produce output");
    expect(!worker.has_pending_work(), "processing drains the queue");
}

void test_exact_binding_and_no_retained_visual() {
    auto item = make_item();
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.submit(item), "binding test item accepted");
    auto advanced = item.source.identity;
    ++advanced.sequence;
    const auto mismatch = worker.process_latest(advanced,
                                                item.source.identity.captured_at_ns + 10'000'000);
    expect(mismatch.disposition == Disposition::bypass_wrong_frame && !mismatch.has_residual(),
           "advanced current frame rejects old residual");
    const auto empty = worker.process_latest(advanced,
                                             item.source.identity.captured_at_ns + 11'000'000);
    expect(empty.disposition == Disposition::bypass_no_work && !empty.has_residual(),
           "rejected work is consumed and never retained");

    item = make_item();
    item.tracking.track.track_epoch += 1U;
    expect(worker.submit(item), "mismatched track item enters queue for authoritative validation");
    const auto wrong_track = worker.process_latest(item.source.identity,
                                                   item.source.identity.captured_at_ns + 10'000'000);
    expect(wrong_track.disposition == Disposition::bypass_wrong_frame,
           "wrong track epoch is rejected with no visual");
}

void test_cancellation_generation() {
    auto item = make_item();
    ReferenceMouthWorker worker(item.track.cancellation_generation);
    expect(worker.submit(item), "pre-cancel work accepted");
    expect(worker.cancel_to(item.track.cancellation_generation + 1U), "generation advances");
    expect(!worker.has_pending_work(), "generation cancellation synchronously clears pending work");
    expect(!worker.submit(item), "old-generation work is refused");
    const auto empty = worker.process_latest(item.source.identity,
                                             item.source.identity.captured_at_ns + 10'000'000);
    expect(empty.disposition == Disposition::bypass_no_work, "cancellation leaves untouched fallback");
}

void test_safety_gates() {
    const auto run = [](WorkItem item, const Nanoseconds now_ns) {
        const auto generation = item.track.cancellation_generation;
        const auto identity = item.source.identity;
        ReferenceMouthWorker worker(generation);
        (void)worker.submit(std::move(item));
        return worker.process_latest(identity, now_ns).disposition;
    };

    auto occluded = make_item();
    occluded.tracking.mouth_occluded = true;
    expect(run(occluded, occluded.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_occluded,
           "mouth occlusion bypasses");

    auto pose = make_item();
    pose.tracking.pose.yaw = 36.0;
    expect(run(pose, pose.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_pose,
           "excessive pose bypasses");

    auto confidence = make_item();
    confidence.tracking.landmark_confidence = 0.81;
    expect(run(confidence, confidence.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_low_confidence,
           "low landmark confidence bypasses");

    auto containment = make_item();
    containment.tracking.mouth_bounds.x = 0.1;
    expect(run(containment, containment.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "mouth outside selected face bypasses");

    auto feathered_edge = make_item();
    feathered_edge.tracking.face_bounds.height = 0.49;
    expect(run(feathered_edge, feathered_edge.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::residual_ready,
           "bounded mask feather may cross the face box when all semantic lip points remain inside");

    auto landmark_outside_face = feathered_edge;
    landmark_outside_face.tracking.mouth_landmarks.lower_lip_center.y = 0.695;
    expect(run(landmark_outside_face,
               landmark_outside_face.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "semantic lip points outside the face remain rejected even inside the padded mask");

    auto landmark_schema = make_item();
    landmark_schema.tracking.mouth_landmarks.schema_version = 2U;
    expect(run(landmark_schema, landmark_schema.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "unknown landmark adapter schema bypasses");

    auto landmark_confidence = make_item();
    landmark_confidence.tracking.mouth_landmarks.left_corner.confidence = 0.4;
    expect(run(landmark_confidence,
               landmark_confidence.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "low-confidence semantic landmark bypasses");

    auto qualified_semantic_confidence = make_item();
    qualified_semantic_confidence.tracking.mouth_landmarks.lower_lip_center.confidence = 0.56;
    expect(run(qualified_semantic_confidence,
               qualified_semantic_confidence.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::residual_ready,
           "visible semantic point is accepted alongside qualified aggregate confidence");

    auto hidden_semantic_point = make_item();
    hidden_semantic_point.tracking.mouth_landmarks.lower_lip_center.confidence = 0.54;
    expect(run(hidden_semantic_point,
               hidden_semantic_point.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "semantic point below the provider visibility floor still bypasses");

    auto huge = make_item();
    huge.tracking.face_bounds = {0.0, 0.0, 1.0, 1.0};
    huge.tracking.mouth_bounds = {0.2, 0.3, 0.5, 0.5};
    expect(run(huge, huge.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_unsafe_bounds,
           "oversized mouth residual bypasses");

    auto stale = make_item();
    expect(run(stale, stale.source.identity.captured_at_ns + 81'000'000) ==
               Disposition::bypass_deadline,
           "expired per-frame deadline bypasses before compositing");

    auto stale_source = make_item();
    stale_source.deadline_ns = stale_source.source.identity.captured_at_ns + 250'000'000;
    stale_source.source.lease.expires_at_ns =
        stale_source.source.identity.captured_at_ns + 250'000'000;
    expect(run(stale_source, stale_source.source.identity.captured_at_ns + 151'000'000) ==
               Disposition::bypass_stale_frame,
           "source age hard limit bypasses even with a later caller deadline");

    auto stale_tracking = make_item();
    stale_tracking.deadline_ns = stale_tracking.source.identity.captured_at_ns + 250'000'000;
    stale_tracking.source.lease.expires_at_ns =
        stale_tracking.source.identity.captured_at_ns + 250'000'000;
    stale_tracking.tracking.measured_at_ns = stale_tracking.source.identity.captured_at_ns - 150'000'001;
    expect(run(stale_tracking,
               stale_tracking.source.identity.captured_at_ns + 1'000'000) ==
               Disposition::bypass_invalid_tracking,
           "stale landmark evidence bypasses independently of source freshness");

    auto expired = make_item();
    expired.source.lease.expires_at_ns = expired.source.identity.captured_at_ns + 5'000'000;
    expect(run(expired, expired.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_expired_lease,
           "expired source lease bypasses");

    auto audio = make_item();
    audio.drive.clock.playback_at_ns += 81'000'000;
    expect(run(audio, audio.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "audio outside the source-frame skew budget bypasses");

    auto future_audio = make_item();
    future_audio.drive.clock.playback_at_ns =
        future_audio.source.identity.captured_at_ns + 11'000'000;
    expect(run(future_audio, future_audio.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "an audio timestamp later than processing time fails closed");

    auto wrong_audio_interval = make_item();
    wrong_audio_interval.drive.clock.playback_sample_index =
        wrong_audio_interval.drive.clock.sample_count;
    expect(run(wrong_audio_interval,
               wrong_audio_interval.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "a playback cursor outside the bound sample interval fails closed");

    auto overflowing_audio_interval = make_item();
    overflowing_audio_interval.drive.clock.first_sample_index =
        std::numeric_limits<std::uint64_t>::max() - 10U;
    overflowing_audio_interval.drive.clock.sample_count = 20U;
    overflowing_audio_interval.drive.clock.playback_sample_index =
        overflowing_audio_interval.drive.clock.first_sample_index;
    expect(run(overflowing_audio_interval,
               overflowing_audio_interval.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "an overflowing sample interval fails closed");

    auto malformed_drive = make_item();
    malformed_drive.drive.kind = static_cast<DriveKind>(255U);
    expect(run(malformed_drive,
               malformed_drive.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "unknown drive kind bypasses");

    auto empty_pcm = make_item();
    empty_pcm.drive.kind = DriveKind::pcm_window;
    empty_pcm.drive.interleaved_pcm.clear();
    expect(run(empty_pcm, empty_pcm.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "empty PCM window bypasses");

    auto mismatched_pcm_window = make_item();
    mismatched_pcm_window.drive.kind = DriveKind::pcm_window;
    mismatched_pcm_window.drive.clock.sample_count = 2U;
    mismatched_pcm_window.drive.interleaved_pcm = {0.1F};
    expect(run(mismatched_pcm_window,
               mismatched_pcm_window.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_audio_clock,
           "PCM samples that do not cover the declared interval fail closed");

    auto nan = make_item();
    nan.tracking.face_confidence = std::numeric_limits<double>::quiet_NaN();
    expect(run(nan, nan.source.identity.captured_at_ns + 10'000'000) ==
               Disposition::bypass_invalid_tracking,
           "non-finite tracking evidence bypasses");
}

void test_pcm_path_and_determinism() {
    auto first = make_item();
    first.drive.kind = DriveKind::pcm_window;
    first.drive.interleaved_pcm.resize(480U);
    first.drive.clock.sample_count = 480U;
    for (std::size_t index = 0; index < first.drive.interleaved_pcm.size(); ++index) {
        first.drive.interleaved_pcm[index] = static_cast<float>(
            0.2 * std::sin(static_cast<double>(index) * 0.071));
    }
    auto second = first;
    ReferenceMouthWorker first_worker(first.track.cancellation_generation);
    ReferenceMouthWorker second_worker(second.track.cancellation_generation);
    (void)first_worker.submit(first);
    (void)second_worker.submit(second);
    const auto now = first.source.identity.captured_at_ns + 10'000'000;
    const auto first_result = first_worker.process_latest(first.source.identity, now);
    const auto second_result = second_worker.process_latest(second.source.identity, now);
    expect(first_result.has_residual() && second_result.has_residual(),
           "PCM fallback produces real residuals");
    expect(digest(first_result.residual.premultiplied_bgra) ==
               digest(second_result.residual.premultiplied_bgra),
           "reference output is byte-for-byte deterministic");
}

void test_pcm_smoothing_preserves_attack_and_release() {
    auto loud = make_item(201U, 2'000'000'000);
    loud.drive.kind = DriveKind::pcm_window;
    loud.drive.clock.segment_id = 31U;
    loud.drive.clock.first_sample_index = 0U;
    loud.drive.clock.sample_count = 1'600U;
    loud.drive.clock.playback_sample_index = 0U;
    loud.drive.clock.playback_at_ns = loud.source.identity.captured_at_ns;
    loud.drive.interleaved_pcm.resize(1'600U);
    for (std::size_t index = 0; index < loud.drive.interleaved_pcm.size(); ++index) {
        loud.drive.interleaved_pcm[index] = static_cast<float>(
            0.18 * std::sin(static_cast<double>(index) * 0.081));
    }

    ReferenceMouthWorker worker(loud.track.cancellation_generation);
    expect(worker.submit(loud), "loud smoothing frame accepted");
    const auto loud_result = worker.process_latest(
        loud.source.identity, loud.source.identity.captured_at_ns + 2'000'000);
    expect(loud_result.has_residual() && loud_result.residual.coefficients.jaw_open > 0.5,
           "speech attack opens the jaw promptly");

    auto quiet = loud;
    quiet.source = make_frame(202U, loud.source.identity.captured_at_ns + 33'000'000);
    quiet.track = loud.track;
    quiet.tracking = loud.tracking;
    quiet.tracking.track = quiet.track;
    quiet.tracking.frame = quiet.source.identity;
    quiet.tracking.measured_at_ns = quiet.source.identity.captured_at_ns + 1'000'000;
    quiet.drive.clock.first_sample_index += loud.drive.clock.sample_count;
    quiet.drive.clock.playback_sample_index = quiet.drive.clock.first_sample_index;
    quiet.drive.clock.playback_at_ns = quiet.source.identity.captured_at_ns;
    quiet.drive.interleaved_pcm.assign(1'600U, 0.0F);
    quiet.deadline_ns = quiet.source.identity.captured_at_ns + 50'000'000;
    expect(worker.submit(quiet), "quiet smoothing frame accepted");
    const auto quiet_result = worker.process_latest(
        quiet.source.identity, quiet.source.identity.captured_at_ns + 2'000'000);
    expect(quiet_result.has_residual() &&
               quiet_result.residual.coefficients.jaw_open < 0.01 &&
               quiet_result.residual.coefficients.lip_close > 0.99,
           "exact silence closes immediately instead of smearing a contact boundary");

    auto new_segment = quiet;
    new_segment.source = make_frame(203U, quiet.source.identity.captured_at_ns + 33'000'000);
    new_segment.tracking.frame = new_segment.source.identity;
    new_segment.tracking.measured_at_ns = new_segment.source.identity.captured_at_ns + 1'000'000;
    new_segment.drive.clock.segment_id += 1U;
    new_segment.drive.clock.first_sample_index = 0U;
    new_segment.drive.clock.playback_sample_index = 0U;
    new_segment.drive.clock.playback_at_ns = new_segment.source.identity.captured_at_ns;
    new_segment.deadline_ns = new_segment.source.identity.captured_at_ns + 50'000'000;
    expect(worker.submit(new_segment), "new PCM segment accepted");
    const auto reset_result = worker.process_latest(
        new_segment.source.identity, new_segment.source.identity.captured_at_ns + 2'000'000);
    expect(reset_result.has_residual() && reset_result.residual.coefficients.jaw_open < 0.01,
           "a new segment resets smoothing instead of leaking the prior utterance");
}

void test_hard_safety_limits_cannot_be_relaxed() {
    auto item = make_item();
    item.tracking.pose.yaw = 36.0;
    WorkerPolicy permissive{};
    permissive.maximum_absolute_yaw_degrees = 180.0;
    permissive.minimum_face_confidence = 0.0;
    permissive.minimum_landmark_confidence = 0.0;
    permissive.minimum_visibility_ratio = 0.0;
    permissive.maximum_mouth_width_fraction = 1.0;
    permissive.maximum_mouth_height_fraction = 1.0;
    permissive.maximum_mouth_area_fraction = 1.0;
    permissive.maximum_source_age_ns = 1'000'000'000;
    permissive.maximum_tracking_age_ns = 1'000'000'000;
    permissive.maximum_audio_skew_ns = 1'000'000'000;
    ReferenceMouthWorker worker(item.track.cancellation_generation, permissive);
    (void)worker.submit(item);
    const auto result = worker.process_latest(item.source.identity,
                                              item.source.identity.captured_at_ns + 10'000'000);
    expect(result.disposition == Disposition::bypass_pose,
           "configuration cannot relax the hard pose ceiling");
}

void test_public_compositor_rejects_malformed_direct_calls() {
    const auto item = make_item();
    auto invalid_tracking = item.tracking;
    invalid_tracking.mouth_bounds.x = -0.1;
    const auto invalid = compose_current_frame_residual(
        item.source, item.track, invalid_tracking,
        coefficients_for_viseme(Viseme::open_vowel), item.source.identity.captured_at_ns + 1);
    expect(invalid.premultiplied_bgra.empty(),
           "direct compositor rejects an out-of-range normalized ROI");

    auto truncated = ResidualPatch{};
    truncated.source_frame = item.source.identity;
    truncated.normalized_bounds = item.tracking.mouth_bounds;
    truncated.width = 20U;
    truncated.height = 10U;
    truncated.stride_bytes = 80U;
    truncated.premultiplied_bgra.resize(4U);
    expect(composite_over_source(item.source, truncated) == item.source.bgra,
           "preview compositor rejects a truncated residual buffer");
}

} // namespace

int main() {
    test_viseme_and_audio_drives();
    test_current_frame_residual_is_bounded_and_premultiplied();
    test_pcm_fallback_preserves_source_colour_envelope();
    test_pcm_fallback_keeps_cavity_below_upper_lip();
    test_full_contour_visemes_have_distinct_geometry();
    test_atlas_residual_preserves_source_lips_and_binds_to_current_frame();
    test_observed_patch_extraction_preserves_real_source_pixels();
    test_worker_uses_identity_bound_atlas_and_clears_on_cancel();
    test_worker_never_applies_an_atlas_to_another_actor();
    test_timed_viseme_does_not_wait_for_atlas_dwell();
    test_timed_atlas_transition_is_continuous_on_its_first_frame();
    test_bilabial_closure_remains_immediate_during_coarticulation();
    test_atlas_coarticulation_is_time_based_and_segment_bound();
    test_queue_depth_one();
    test_exact_binding_and_no_retained_visual();
    test_cancellation_generation();
    test_safety_gates();
    test_pcm_path_and_determinism();
    test_pcm_smoothing_preserves_attack_and_release();
    test_hard_safety_limits_cannot_be_relaxed();
    test_public_compositor_rejects_malformed_direct_calls();

    if (failures != 0) {
        std::cerr << failures << " mouth-worker test assertion(s) failed\n";
        return 1;
    }
    std::cout << "PASS: deterministic current-frame mouth worker (all safety gates)\n";
    return 0;
}
