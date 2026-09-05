#include "npc/mouth_worker/compositor.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <iostream>
#include <string_view>
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

[[nodiscard]] CpuFrame make_speaking_frame() {
    constexpr std::uint32_t width = 200U;
    constexpr std::uint32_t height = 120U;
    CpuFrame frame{};
    frame.lease.schema_version = 1U;
    frame.lease.transport = LeaseTransport::cpu_reference;
    frame.lease.lease_nonce_low = 1U;
    frame.lease.owner_process_id = 10U;
    frame.lease.intended_consumer_process_id = 11U;
    frame.lease.width = width;
    frame.lease.height = height;
    frame.lease.stride_bytes = width * 4U;
    frame.lease.expires_at_ns = 2'000'000'000;
    frame.identity = {1U, 1U, 1U, 1'000'000'000};
    frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) * height);
    for (std::uint32_t y = 0U; y < height; ++y) {
        for (std::uint32_t x = 0U; x < width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * frame.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            std::array<std::uint8_t, 3U> colour{122U, 157U, 194U};
            const double ellipse = std::pow((static_cast<double>(x) - 100.0) / 40.0, 2.0) +
                                   std::pow((static_cast<double>(y) - 60.0) / 12.0, 2.0);
            if (ellipse <= 1.0) colour = {72U, 58U, 142U};
            const double cavity = std::pow((static_cast<double>(x) - 100.0) / 32.0, 2.0) +
                                  std::pow((static_cast<double>(y) - 60.5) / 4.2, 2.0);
            if (cavity <= 1.0) colour = {22U, 18U, 28U};
            frame.bgra[offset + 0U] = colour[0U];
            frame.bgra[offset + 1U] = colour[1U];
            frame.bgra[offset + 2U] = colour[2U];
            frame.bgra[offset + 3U] = 255U;
        }
    }
    return frame;
}

[[nodiscard]] CpuFrame make_vertical_ramp_frame() {
    auto frame = make_speaking_frame();
    for (std::uint32_t y = 0U; y < frame.lease.height; ++y) {
        for (std::uint32_t x = 0U; x < frame.lease.width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * frame.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const auto value = static_cast<std::uint8_t>(y * 2U);
            frame.bgra[offset + 0U] = value;
            frame.bgra[offset + 1U] = value;
            frame.bgra[offset + 2U] = value;
            frame.bgra[offset + 3U] = 255U;
        }
    }
    return frame;
}

[[nodiscard]] TrackingEvidence make_tracking(const CpuFrame& frame) {
    TrackingEvidence tracking{};
    tracking.track = {1U, 2U, 3U, 4U};
    tracking.frame = frame.identity;
    tracking.face_bounds = {0.2, 0.1, 0.6, 0.8};
    tracking.mouth_bounds = {0.20, 0.32, 0.60, 0.40};
    tracking.mouth_landmarks.schema_version = 2U;
    tracking.mouth_landmarks.provider_instance_id = 44U;
    tracking.mouth_landmarks.contour_points = mouth_contour_point_count;
    tracking.mouth_landmarks.contour = {{
        {0.36, 0.467, 1.0}, {0.43, 0.433, 1.0}, {0.50, 0.417, 1.0},
        {0.57, 0.433, 1.0}, {0.64, 0.467, 1.0}, {0.64, 0.550, 1.0},
        {0.57, 0.583, 1.0}, {0.50, 0.600, 1.0}, {0.43, 0.583, 1.0},
        {0.36, 0.550, 1.0}, {0.30, 0.500, 1.0}, {0.40, 0.483, 1.0},
        {0.50, 0.475, 1.0}, {0.60, 0.483, 1.0}, {0.70, 0.500, 1.0},
        {0.60, 0.525, 1.0}, {0.50, 0.533, 1.0}, {0.40, 0.525, 1.0},
    }};
    tracking.mouth_landmarks.left_corner = tracking.mouth_landmarks.contour[10U];
    tracking.mouth_landmarks.right_corner = tracking.mouth_landmarks.contour[14U];
    tracking.mouth_landmarks.upper_lip_center = {0.50, 0.475, 1.0};
    tracking.mouth_landmarks.lower_lip_center = {0.50, 0.533, 1.0};
    tracking.face_confidence = 1.0;
    tracking.landmark_confidence = 1.0;
    tracking.visibility_ratio = 1.0;
    tracking.measured_at_ns = frame.identity.captured_at_ns + 1'000'000;
    return tracking;
}

[[nodiscard]] std::array<std::uint8_t, 3U> pixel(
    const std::vector<std::uint8_t>& bytes,
    const CpuFrame& frame,
    const std::uint32_t x,
    const std::uint32_t y) {
    const auto offset = static_cast<std::size_t>(y) * frame.lease.stride_bytes +
                        static_cast<std::size_t>(x) * 4U;
    return {bytes[offset], bytes[offset + 1U], bytes[offset + 2U]};
}

[[nodiscard]] CanonicalMouthPatch make_colour_patch(
    const MouthPatchRepresentation representation,
    const bool oral_slot_only) {
    constexpr std::uint32_t width = 64U;
    constexpr std::uint32_t height = 40U;
    CanonicalMouthPatch patch{};
    patch.width = width;
    patch.height = height;
    patch.stride_bytes = width * 4U;
    patch.representation = representation;
    patch.premultiplied_bgra.assign(
        static_cast<std::size_t>(patch.stride_bytes) * height, 0U);
    for (std::uint32_t y = 0U; y < height; ++y) {
        for (std::uint32_t x = 0U; x < width; ++x) {
            const double canonical_x =
                (static_cast<double>(x) + 0.5) / width * 2.0 - 1.0;
            const double canonical_y =
                (static_cast<double>(y) + 0.5) / height * 2.0 - 1.0;
            if (oral_slot_only &&
                (canonical_x < -0.67 || canonical_x > 0.67 ||
                 canonical_y < -0.24 || canonical_y > 0.20)) {
                continue;
            }
            const auto offset = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            patch.premultiplied_bgra[offset + 0U] = 18U;
            patch.premultiplied_bgra[offset + 1U] = 228U;
            patch.premultiplied_bgra[offset + 2U] = 24U;
            patch.premultiplied_bgra[offset + 3U] = 255U;
        }
    }
    return patch;
}

void test_fixed_skin_ring_replaces_contracted_source_corners() {
    const auto source = make_speaking_frame();
    const auto tracking = make_tracking(source);
    const auto track = tracking.track;
    const auto residual = compose_current_frame_residual(
        source, track, tracking, coefficients_for_viseme(Viseme::rounded, 1.0),
        source.identity.captured_at_ns + 4'000'000);
    const auto output = composite_over_source(source, residual);
    expect(!residual.premultiplied_bgra.empty(),
           "rounded full-contour articulation produces a residual");

    std::size_t replaced_old_corner_pixels = 0U;
    for (std::uint32_t y = 56U; y <= 64U; ++y) {
        for (const std::uint32_t x : {62U, 63U, 64U, 136U, 137U, 138U}) {
            const auto before = pixel(source.bgra, source, x, y);
            const auto after = pixel(output, source, x, y);
            const int delta = std::abs(static_cast<int>(after[0U]) - before[0U]) +
                              std::abs(static_cast<int>(after[1U]) - before[1U]) +
                              std::abs(static_cast<int>(after[2U]) - before[2U]);
            replaced_old_corner_pixels += delta >= 20 ? 1U : 0U;
        }
    }
    expect(replaced_old_corner_pixels >= 8U,
           "fixed skin annulus overwrites the old wide lip corners during O contraction");

    for (std::uint32_t y = 0U; y < source.lease.height; ++y) {
        for (std::uint32_t x = 0U; x < source.lease.width; ++x) {
            if (x >= 42U && x <= 158U && y >= 39U && y <= 81U) continue;
            expect(pixel(output, source, x, y) == pixel(source.bgra, source, x, y),
                   "fixed skin ring leaves every exterior pixel byte-identical");
        }
    }
}

void test_bilabial_cue_deforms_an_already_open_source() {
    const auto source = make_speaking_frame();
    const auto tracking = make_tracking(source);
    const auto closed = compose_current_frame_residual(
        source, tracking.track, tracking, coefficients_for_viseme(Viseme::bilabial, 1.0),
        source.identity.captured_at_ns + 4'000'000);
    const auto rounded = compose_current_frame_residual(
        source, tracking.track, tracking, coefficients_for_viseme(Viseme::rounded, 1.0),
        source.identity.captured_at_ns + 4'000'000);
    expect(!closed.premultiplied_bgra.empty(),
           "silence can close a source observation captured mid-speech");
    expect(closed.premultiplied_bgra != rounded.premultiplied_bgra,
           "bilabial closure remains distinct from a rounded aperture");
    const auto closed_frame = composite_over_source(source, closed);
    const auto source_center = pixel(source.bgra, source, 100U, 61U);
    const auto closed_center = pixel(closed_frame, source, 100U, 61U);
    expect(closed_center != source_center,
           "closure replaces the captured open cavity at the mouth centre");
}

void test_silence_preserves_the_current_game_frame() {
    const auto source = make_speaking_frame();
    const auto tracking = make_tracking(source);
    const auto silence = compose_current_frame_residual(
        source, tracking.track, tracking, coefficients_for_viseme(Viseme::silence, 1.0),
        source.identity.captured_at_ns + 4'000'000);
    const auto output = composite_over_source(source, silence);
    expect(!silence.premultiplied_bgra.empty(),
           "silence returns a valid bounded transparent residual");
    expect(std::all_of(
               silence.premultiplied_bgra.begin(), silence.premultiplied_bgra.end(),
               [](const std::uint8_t value) { return value == 0U; }),
           "silence cannot paint over the current game mouth");
    expect(output == source.bgra,
           "silence leaves every current-frame source pixel byte-identical");
}

void test_closed_cue_keeps_uneven_inner_lip_warp_non_folding() {
    const auto source = make_vertical_ramp_frame();
    auto tracking = make_tracking(source);
    tracking.mouth_landmarks.contour[11U].y = 0.495;
    tracking.mouth_landmarks.contour[12U].y = 0.450;
    tracking.mouth_landmarks.contour[13U].y = 0.495;
    tracking.mouth_landmarks.contour[15U].y = 0.505;
    tracking.mouth_landmarks.contour[16U].y = 0.550;
    tracking.mouth_landmarks.contour[17U].y = 0.505;
    const auto closed = compose_current_frame_residual(
        source, tracking.track, tracking, coefficients_for_viseme(Viseme::bilabial, 1.0),
        source.identity.captured_at_ns + 4'000'000);
    const auto output = composite_over_source(source, closed);
    expect(!closed.premultiplied_bgra.empty(),
           "bilabial closure renders for uneven tracked inner-lip gaps");

    // The fixture's side gaps are smaller than its centre gap. A single
    // mean-gap correction crosses both side pairs. Encoding source Y in every
    // channel turns that fold into an abrupt source-row jump in the composed
    // pixels. A valid per-pair closure stays monotonic without that jump.
    for (const std::uint32_t x : {80U, 120U}) {
        int previous = static_cast<int>(pixel(output, source, x, 52U)[0U]);
        for (std::uint32_t y = 53U; y <= 68U; ++y) {
            const int current = static_cast<int>(pixel(output, source, x, y)[0U]);
            expect(current + 2 >= previous,
                   "bilabial closure must not fold lower-lip source pixels above upper-lip pixels");
            expect(current <= previous + 4,
                   "bilabial closure must not stretch folded source rows across the contact seam");
            previous = current;
        }
    }
}

void test_normalized_oral_texture_cannot_replace_lips_or_skin() {
    const auto source = make_speaking_frame();
    const auto tracking = make_tracking(source);
    const auto oral = make_colour_patch(
        MouthPatchRepresentation::normalized_oral_interior_v1, true);
    const auto residual = compose_atlas_residual(
        source, tracking.track, tracking, oral,
        coefficients_for_viseme(Viseme::rounded, 1.0),
        source.identity.captured_at_ns + 4'000'000);
    const auto output = composite_over_source(source, residual);
    expect(!residual.premultiplied_bgra.empty(),
           "normalized oral state renders with full contour evidence");
    std::size_t oral_pixels = 0U;
    std::size_t oral_pixels_outside_aperture = 0U;
    for (std::uint32_t y = 0U; y < source.lease.height; ++y) {
        for (std::uint32_t x = 0U; x < source.lease.width; ++x) {
            const auto value = pixel(output, source, x, y);
            const bool from_oral_atlas = value[1U] > value[0U] + 35U &&
                                         value[1U] > value[2U] + 35U;
            oral_pixels += from_oral_atlas ? 1U : 0U;
            oral_pixels_outside_aperture +=
                from_oral_atlas && (x < 70U || x > 130U || y < 50U || y > 72U)
                    ? 1U
                    : 0U;
        }
    }
    expect(oral_pixels >= 30U,
           "normalized atlas contributes visible oral anatomy");
    expect(oral_pixels_outside_aperture == 0U,
           "normalized atlas pixels cannot replace outer lips or surrounding skin");

    auto unknown = oral;
    unknown.representation = static_cast<MouthPatchRepresentation>(255U);
    expect(compose_atlas_residual(
               source, tracking.track, tracking, unknown,
               coefficients_for_viseme(Viseme::rounded, 1.0),
               source.identity.captured_at_ns + 4'000'000)
               .premultiplied_bgra.empty(),
           "unknown atlas pixel representation is rejected");
}

void test_legacy_full_lip_representation_keeps_original_sampling_contract() {
    const auto source = make_speaking_frame();
    const auto tracking = make_tracking(source);
    const auto legacy = make_colour_patch(
        MouthPatchRepresentation::full_lip_observation_v1, false);
    const auto residual = compose_atlas_residual(
        source, tracking.track, tracking, legacy,
        coefficients_for_viseme(Viseme::open_vowel, 1.0),
        source.identity.captured_at_ns + 4'000'000);
    const auto output = composite_over_source(source, residual);
    expect(!residual.premultiplied_bgra.empty(),
           "legacy full-lip observation still renders through its original branch");
    std::size_t legacy_pixels_above_aperture = 0U;
    for (std::uint32_t y = 44U; y <= 53U; ++y) {
        for (std::uint32_t x = 72U; x <= 128U; ++x) {
            const auto value = pixel(output, source, x, y);
            legacy_pixels_above_aperture +=
                value[1U] > value[0U] + 35U && value[1U] > value[2U] + 35U
                    ? 1U
                    : 0U;
        }
    }
    expect(legacy_pixels_above_aperture > 0U,
           "legacy full-lip pixels retain their full canonical-patch semantics");
}

} // namespace

int main() {
    test_fixed_skin_ring_replaces_contracted_source_corners();
    test_bilabial_cue_deforms_an_already_open_source();
    test_silence_preserves_the_current_game_frame();
    test_closed_cue_keeps_uneven_inner_lip_warp_non_folding();
    test_normalized_oral_texture_cannot_replace_lips_or_skin();
    test_legacy_full_lip_representation_keeps_original_sampling_contract();
    if (failures == 0) {
        std::cout << "compositor render tests passed\n";
        return 0;
    }
    std::cerr << failures << " compositor render assertion(s) failed\n";
    return 1;
}
