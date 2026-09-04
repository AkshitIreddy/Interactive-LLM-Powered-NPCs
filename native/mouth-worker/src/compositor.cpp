#include "npc/mouth_worker/compositor.hpp"

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <limits>

namespace npc::mouth {
namespace {

[[nodiscard]] double unit(const double value) noexcept {
    return std::clamp(std::isfinite(value) ? value : 0.0, 0.0, 1.0);
}

[[nodiscard]] double smooth_unit(const double value) noexcept {
    const double t = unit(value);
    return t * t * (3.0 - 2.0 * t);
}

[[nodiscard]] std::uint8_t byte_from_unit(const double value) noexcept {
    return static_cast<std::uint8_t>(std::lround(unit(value) * 255.0));
}

[[nodiscard]] double sample_channel(const CpuFrame& frame,
                                    const double x,
                                    const double y,
                                    const std::size_t channel) noexcept {
    const double maximum_x = static_cast<double>(frame.lease.width - 1U);
    const double maximum_y = static_cast<double>(frame.lease.height - 1U);
    const double clipped_x = std::clamp(x, 0.0, maximum_x);
    const double clipped_y = std::clamp(y, 0.0, maximum_y);
    const auto x0 = static_cast<std::uint32_t>(std::floor(clipped_x));
    const auto y0 = static_cast<std::uint32_t>(std::floor(clipped_y));
    const auto x1 = std::min(x0 + 1U, frame.lease.width - 1U);
    const auto y1 = std::min(y0 + 1U, frame.lease.height - 1U);
    const double fx = clipped_x - static_cast<double>(x0);
    const double fy = clipped_y - static_cast<double>(y0);
    const auto at = [&](const std::uint32_t px, const std::uint32_t py) {
        const auto offset = static_cast<std::size_t>(py) * frame.lease.stride_bytes +
                            static_cast<std::size_t>(px) * 4U + channel;
        return static_cast<double>(frame.bgra[offset]);
    };
    const double top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
    const double bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
    return top * (1.0 - fy) + bottom * fy;
}

[[nodiscard]] MouthCoefficients clamp_coefficients(MouthCoefficients value) noexcept {
    value.jaw_open = unit(value.jaw_open);
    value.lip_close = unit(value.lip_close);
    value.funnel = unit(value.funnel);
    value.pucker = unit(value.pucker);
    value.smile_left = unit(value.smile_left);
    value.smile_right = unit(value.smile_right);
    value.upper_lip_raise = unit(value.upper_lip_raise);
    value.lower_lip_depress = unit(value.lower_lip_depress);
    return value;
}

[[nodiscard]] bool normalized_rect(const NormalizedRect& rect) noexcept {
    return std::isfinite(rect.x) && std::isfinite(rect.y) &&
           std::isfinite(rect.width) && std::isfinite(rect.height) &&
           rect.x >= 0.0 && rect.y >= 0.0 && rect.width > 0.0 && rect.height > 0.0 &&
           rect.right() <= 1.0 && rect.bottom() <= 1.0;
}

[[nodiscard]] bool valid_atlas_patch(const CanonicalMouthPatch& patch) noexcept {
    if (patch.width < 2U || patch.height < 2U || patch.width > 512U || patch.height > 512U ||
        patch.stride_bytes < patch.width * 4U) {
        return false;
    }
    const auto required = static_cast<std::uint64_t>(patch.stride_bytes) * patch.height;
    if (required > static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max()) ||
        patch.premultiplied_bgra.size() < static_cast<std::size_t>(required)) {
        return false;
    }
    for (std::uint32_t y = 0; y < patch.height; ++y) {
        for (std::uint32_t x = 0; x < patch.width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const auto alpha = patch.premultiplied_bgra[offset + 3U];
            if (patch.premultiplied_bgra[offset + 0U] > alpha ||
                patch.premultiplied_bgra[offset + 1U] > alpha ||
                patch.premultiplied_bgra[offset + 2U] > alpha) {
                return false;
            }
        }
    }
    return true;
}

void initialize_residual_metadata(ResidualPatch& patch,
                                  const CpuFrame& source,
                                  const TrackBinding& track,
                                  const NormalizedRect normalized_bounds,
                                  const std::uint32_t width,
                                  const std::uint32_t height,
                                  const Nanoseconds produced_at_ns) {
    patch.track = track;
    patch.source_frame = source.identity;
    patch.normalized_bounds = normalized_bounds;
    patch.source_lease = source.lease;
    patch.source_lease.native_handle_value = 0;
    patch.width = width;
    patch.height = height;
    patch.stride_bytes = width * 4U;
    patch.produced_at_ns = produced_at_ns;
    patch.residual_lease.schema_version = 1U;
    patch.residual_lease.transport = LeaseTransport::cpu_reference;
    patch.residual_lease.lease_nonce_high = source.lease.lease_nonce_high ^ 0x4d4f555448504154ULL;
    patch.residual_lease.lease_nonce_low = source.lease.lease_nonce_low ^ 0x4348000000000001ULL;
    patch.residual_lease.owner_process_id = source.lease.intended_consumer_process_id;
    patch.residual_lease.intended_consumer_process_id = source.lease.owner_process_id;
    patch.residual_lease.adapter_luid_low = source.lease.adapter_luid_low;
    patch.residual_lease.adapter_luid_high = source.lease.adapter_luid_high;
    patch.residual_lease.width = width;
    patch.residual_lease.height = height;
    patch.residual_lease.stride_bytes = patch.stride_bytes;
    patch.residual_lease.format = PixelFormat::bgra8_unorm_premultiplied;
    patch.residual_lease.expires_at_ns = std::min(source.lease.expires_at_ns,
                                                 produced_at_ns + 80'000'000);
    patch.premultiplied_bgra.assign(
        static_cast<std::size_t>(patch.stride_bytes) * height, 0U);
}

} // namespace

MouthCoefficients coefficients_for_viseme(const Viseme viseme, const double strength) noexcept {
    MouthCoefficients value{};
    switch (viseme) {
    case Viseme::silence:
        value.lip_close = 1.0;
        break;
    case Viseme::bilabial:
        value.lip_close = 0.95;
        value.pucker = 0.22;
        break;
    case Viseme::labiodental:
        value.lip_close = 0.48;
        value.lower_lip_depress = 0.18;
        break;
    case Viseme::dental:
        value.jaw_open = 0.24;
        value.upper_lip_raise = 0.28;
        break;
    case Viseme::alveolar:
        value.jaw_open = 0.32;
        value.smile_left = 0.12;
        value.smile_right = 0.12;
        break;
    case Viseme::postalveolar:
        value.jaw_open = 0.36;
        value.funnel = 0.3;
        break;
    case Viseme::palatal:
        value.jaw_open = 0.42;
        value.smile_left = 0.24;
        value.smile_right = 0.24;
        break;
    case Viseme::velar:
        value.jaw_open = 0.5;
        value.lower_lip_depress = 0.2;
        break;
    case Viseme::rounded:
        value.jaw_open = 0.38;
        value.funnel = 0.82;
        value.pucker = 0.76;
        break;
    case Viseme::open_vowel:
        value.jaw_open = 0.92;
        value.upper_lip_raise = 0.32;
        value.lower_lip_depress = 0.68;
        break;
    case Viseme::spread_vowel:
        value.jaw_open = 0.52;
        value.smile_left = 0.76;
        value.smile_right = 0.76;
        break;
    }

    const double mix = unit(strength);
    value.jaw_open *= mix;
    value.funnel *= mix;
    value.pucker *= mix;
    value.smile_left *= mix;
    value.smile_right *= mix;
    value.upper_lip_raise *= mix;
    value.lower_lip_depress *= mix;
    value.lip_close = (1.0 - mix) + value.lip_close * mix;
    return clamp_coefficients(value);
}

MouthCoefficients coefficients_from_pcm(const std::span<const float> interleaved_pcm,
                                        const std::uint32_t sample_rate,
                                        const std::uint16_t channels) noexcept {
    if (interleaved_pcm.empty() || sample_rate < 8'000U || channels == 0U) {
        return coefficients_for_viseme(Viseme::silence);
    }

    double sum_squares = 0.0;
    std::uint64_t zero_crossings = 0;
    double previous = 0.0;
    bool have_previous = false;
    std::size_t finite_samples = 0;
    for (std::size_t frame = 0; frame < interleaved_pcm.size(); frame += channels) {
        double mono = 0.0;
        std::size_t used_channels = 0;
        for (std::size_t channel = 0;
             channel < channels && frame + channel < interleaved_pcm.size();
             ++channel) {
            const double sample = static_cast<double>(interleaved_pcm[frame + channel]);
            if (std::isfinite(sample)) {
                mono += std::clamp(sample, -1.0, 1.0);
                ++used_channels;
            }
        }
        if (used_channels == 0U) {
            continue;
        }
        mono /= static_cast<double>(used_channels);
        sum_squares += mono * mono;
        if (have_previous && ((mono >= 0.0) != (previous >= 0.0))) {
            ++zero_crossings;
        }
        previous = mono;
        have_previous = true;
        ++finite_samples;
    }

    if (finite_samples == 0U) {
        return coefficients_for_viseme(Viseme::silence);
    }
    const double rms = std::sqrt(sum_squares / static_cast<double>(finite_samples));
    // Hosted speech is commonly normalized far below full scale.  A linear
    // amplitude mapping made ordinary -30 dBFS speech barely move while a
    // synthetic 0.24-amplitude sine looked correct in tests.  Work in dB so
    // the causal fallback has a useful speech-range gate (-48 dBFS) and reaches
    // a full opening around -18 dBFS without look-ahead or per-speaker state.
    const double rms_db = rms > 1.0e-9 ? 20.0 * std::log10(rms) : -120.0;
    const double open = unit((rms_db + 48.0) / 30.0);
    const double crossing_ratio = finite_samples > 1U
        ? static_cast<double>(zero_crossings) / static_cast<double>(finite_samples - 1U)
        : 0.0;
    const double frication = unit((crossing_ratio - 0.08) * 3.2);

    MouthCoefficients value{};
    value.jaw_open = open;
    value.lip_close = unit(1.0 - open * 1.8);
    value.funnel = open * frication * 0.35;
    value.pucker = open * (1.0 - frication) * 0.18;
    value.smile_left = open * frication * 0.1;
    value.smile_right = value.smile_left;
    value.upper_lip_raise = open * frication * 0.22;
    value.lower_lip_depress = open * 0.42;
    return clamp_coefficients(value);
}

bool valid_cpu_frame(const CpuFrame& frame) noexcept {
    if (frame.lease.schema_version != 1U ||
        frame.lease.transport != LeaseTransport::cpu_reference ||
        frame.lease.format != PixelFormat::bgra8_unorm_premultiplied ||
        frame.lease.width == 0U || frame.lease.height == 0U ||
        frame.lease.stride_bytes < frame.lease.width * 4U) {
        return false;
    }
    const auto required = static_cast<std::uint64_t>(frame.lease.stride_bytes) * frame.lease.height;
    return required <= static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max()) &&
           frame.bgra.size() >= static_cast<std::size_t>(required);
}

ResidualPatch compose_current_frame_residual(const CpuFrame& source,
                                             const TrackBinding& track,
                                             const TrackingEvidence& tracking,
                                             const MouthCoefficients& raw_coefficients,
                                             const Nanoseconds produced_at_ns) {
    ResidualPatch patch{};
    if (!valid_cpu_frame(source) || track != tracking.track || source.identity != tracking.frame ||
        !normalized_rect(tracking.mouth_bounds)) {
        return patch;
    }
    const auto coefficients = clamp_coefficients(raw_coefficients);
    const double source_width = static_cast<double>(source.lease.width);
    const double source_height = static_cast<double>(source.lease.height);
    const auto left = static_cast<std::uint32_t>(std::floor(tracking.mouth_bounds.x * source_width));
    const auto top = static_cast<std::uint32_t>(std::floor(tracking.mouth_bounds.y * source_height));
    const auto right = static_cast<std::uint32_t>(std::ceil(tracking.mouth_bounds.right() * source_width));
    const auto bottom = static_cast<std::uint32_t>(std::ceil(tracking.mouth_bounds.bottom() * source_height));
    if (right <= left || bottom <= top || right > source.lease.width || bottom > source.lease.height) {
        return patch;
    }

    const NormalizedRect output_bounds{
        static_cast<double>(left) / source_width,
        static_cast<double>(top) / source_height,
        static_cast<double>(right - left) / source_width,
        static_cast<double>(bottom - top) / source_height,
    };
    patch.coefficients = coefficients;
    initialize_residual_metadata(patch, source, track, output_bounds,
                                 right - left, bottom - top, produced_at_ns);

    const double smile = (coefficients.smile_left + coefficients.smile_right) * 0.5;
    const double horizontal_scale = std::clamp(
        1.0 + smile * 0.14 - coefficients.pucker * 0.16, 0.82, 1.16);
    const auto to_patch_pixel = [&](const NormalizedLandmark& landmark) {
        return std::pair{
            landmark.x * source_width - static_cast<double>(left),
            landmark.y * source_height - static_cast<double>(top),
        };
    };
    const auto left_corner = to_patch_pixel(tracking.mouth_landmarks.left_corner);
    const auto right_corner = to_patch_pixel(tracking.mouth_landmarks.right_corner);
    const auto upper_lip = to_patch_pixel(tracking.mouth_landmarks.upper_lip_center);
    const auto lower_lip = to_patch_pixel(tracking.mouth_landmarks.lower_lip_center);
    const double mouth_center_x = (left_corner.first + right_corner.first) * 0.5;
    const double mouth_half_width = std::clamp(
        std::abs(right_corner.first - left_corner.first) * 0.5, 2.0,
        static_cast<double>(patch.width) * 0.48);
    const double landmark_mid_y = (upper_lip.second + lower_lip.second) * 0.5;
    const double corner_span = right_corner.first - left_corner.first;
    const double measured_seam_slope = std::abs(corner_span) > 1.0e-6
        ? (right_corner.second - left_corner.second) / corner_span
        : 0.0;
    // A single noisy corner can otherwise turn the generated cavity into a
    // diagonal slit.  Keep the measured direction, but cap it to a plausible
    // lip-line slope for this bounded frontal/near-frontal rendering path.
    const double seam_slope = std::clamp(measured_seam_slope, -0.08, 0.08);
    // OpenSeeFace's stable semantic packet intentionally carries only outer-lip
    // anchors. Their geometric midpoint can sit inside the upper lip, so refine
    // the contact seam against current-frame pixels. Search only from the
    // landmark midpoint toward the lower outer lip: this excludes moustache and
    // most upper-lip shadow while retaining the true closed-lip contact line.
    const double lip_landmark_span = std::max(1.0, lower_lip.second - upper_lip.second);
    double visual_seam_center_y = landmark_mid_y;
    double best_seam_luma = std::numeric_limits<double>::max();
    constexpr int seam_row_candidates = 9;
    constexpr int seam_column_samples = 17;
    for (int row_candidate = 0; row_candidate < seam_row_candidates; ++row_candidate) {
        const double fraction = 0.50 + 0.40 *
            static_cast<double>(row_candidate) /
            static_cast<double>(seam_row_candidates - 1);
        const double candidate_center_y = upper_lip.second + lip_landmark_span * fraction;
        double weighted_luma = 0.0;
        double total_weight = 0.0;
        for (int column_sample = 0; column_sample < seam_column_samples; ++column_sample) {
            const double mouth_sample_x = -0.72 + 1.44 *
                static_cast<double>(column_sample) /
                static_cast<double>(seam_column_samples - 1);
            const double candidate_x = mouth_center_x + mouth_sample_x * mouth_half_width;
            const double candidate_y = candidate_center_y +
                (candidate_x - mouth_center_x) * seam_slope;
            const double source_x = static_cast<double>(left) + candidate_x - 0.5;
            const double source_y = static_cast<double>(top) + candidate_y - 0.5;
            const double blue = sample_channel(source, source_x, source_y, 0U);
            const double green = sample_channel(source, source_x, source_y, 1U);
            const double red = sample_channel(source, source_x, source_y, 2U);
            const double weight = 1.0 - std::abs(mouth_sample_x) * 0.32;
            weighted_luma += (blue * 0.114 + green * 0.587 + red * 0.299) * weight;
            total_weight += weight;
        }
        const double candidate_luma = weighted_luma / total_weight;
        if (candidate_luma < best_seam_luma) {
            best_seam_luma = candidate_luma;
            visual_seam_center_y = candidate_center_y;
        }
    }
    const double opening_strength = unit(
        coefficients.jaw_open * (1.0 - coefficients.lip_close * 0.55) +
        coefficients.lower_lip_depress * 0.16);
    // The PCM fallback has no phoneme or identity-specific mouth atlas, so it
    // must remain deliberately conservative.  Move the source lips a few
    // pixels and add only a soft, source-derived seam; never invent teeth or
    // tongue anatomy that cannot be inferred from this frame.
    const double maximum_half_gap = std::clamp(
        std::min(mouth_half_width * 0.255, static_cast<double>(patch.height) * 0.36),
        1.0, 17.0);
    // The lower-jaw-only deformation changes fewer pixels than the former
    // symmetric slit. Give real speech enough downward travel to read clearly
    // while keeping the protected upper lip stationary.
    const double added_half_gap = unit(opening_strength * 1.85) * maximum_half_gap;

    for (std::uint32_t y = 0; y < patch.height; ++y) {
        for (std::uint32_t x = 0; x < patch.width; ++x) {
            const double pixel_x = static_cast<double>(x) + 0.5;
            const double pixel_y = static_cast<double>(y) + 0.5;
            const double mouth_x = (pixel_x - mouth_center_x) / mouth_half_width;
            if (std::abs(mouth_x) >= 1.12) {
                continue;
            }
            const double taper = std::sqrt(std::max(0.0, 1.0 - mouth_x * mouth_x));
            const double half_gap = std::abs(mouth_x) < 1.0 ? added_half_gap * taper : 0.0;
            const double seam_y = visual_seam_center_y +
                (pixel_x - mouth_center_x) * seam_slope;
            const double seam_delta = pixel_y - seam_y;
            // Keep the source upper lip entirely intact. Speech opening is a
            // lower-jaw deformation for this lightweight fallback; even a small
            // symmetric expansion reads as a punched-out upper lip on moustached
            // or strongly shaded faces.
            const double cavity_upper_extent = half_gap * 0.02;
            const double cavity_lower_extent = half_gap * 1.55;
            const double base_lip_radius = std::max(
                2.0, std::min(mouth_half_width * 0.22,
                              static_cast<double>(patch.height) * 0.31));
            const double effect_radius = base_lip_radius + half_gap * 0.95;
            if (std::abs(seam_delta) >= effect_radius) {
                continue;
            }
            const double horizontal_feather =
                smooth_unit((1.12 - std::abs(mouth_x)) / 0.18);
            const double vertical_feather = smooth_unit(
                (effect_radius - std::abs(seam_delta)) /
                std::max(1.0, effect_radius * 0.34));
            // A translucent core helps the warped source lips retain their
            // original lighting and beard/skin texture at the blend boundary.
            const double alpha = horizontal_feather * vertical_feather * 0.96;
            const bool in_cavity = half_gap > 1.0e-6 &&
                seam_delta > -cavity_upper_extent &&
                seam_delta < cavity_lower_extent;
            const double source_patch_x = mouth_center_x +
                (pixel_x - mouth_center_x) / horizontal_scale;
            const double warp_falloff = smooth_unit(
                (effect_radius - std::abs(seam_delta)) /
                std::max(1.0, effect_radius * 0.52));
            const double upper_displacement = 0.0;
            const double lower_displacement = half_gap *
                (1.45 + coefficients.lower_lip_depress * 0.18);
            const double source_patch_y = pixel_y +
                (seam_delta < 0.0 ? upper_displacement : -lower_displacement) *
                    warp_falloff;
            const double sample_x = static_cast<double>(left) +
                                    source_patch_x - 0.5;
            const double sample_y = static_cast<double>(top) +
                                    source_patch_y - 0.5;
            const auto output = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const double seam_sample_x = static_cast<double>(left) +
                source_patch_x - 0.5;
            double cavity_sample_y = static_cast<double>(top) + seam_y - 0.5;
            if (in_cavity) {
                // Find the darkest real lip/seam texel in a narrow vertical
                // neighbourhood. This preserves local colour variation and
                // avoids turning a light lower-lip sample into a flat stripe.
                const double search_radius = std::max(1.0, base_lip_radius * 0.58);
                double darkest_luma = std::numeric_limits<double>::max();
                for (int candidate = -3; candidate <= 3; ++candidate) {
                    const double candidate_y = static_cast<double>(top) + seam_y - 0.5 +
                        search_radius * static_cast<double>(candidate) / 3.0;
                    const double candidate_blue = sample_channel(
                        source, seam_sample_x, candidate_y, 0U);
                    const double candidate_green = sample_channel(
                        source, seam_sample_x, candidate_y, 1U);
                    const double candidate_red = sample_channel(
                        source, seam_sample_x, candidate_y, 2U);
                    const double candidate_luma = candidate_blue * 0.114 +
                        candidate_green * 0.587 + candidate_red * 0.299;
                    if (candidate_luma < darkest_luma) {
                        darkest_luma = candidate_luma;
                        cavity_sample_y = candidate_y;
                    }
                }
            }
            for (std::size_t channel = 0; channel < 3U; ++channel) {
                double value = sample_channel(source, sample_x, sample_y, channel);
                if (in_cavity) {
                    const double darkest_source_value = sample_channel(
                        source, seam_sample_x, cavity_sample_y, channel);
                    const double shadow_value = darkest_source_value * 0.72;
                    const double cavity_feather = std::max(
                        0.55, (cavity_upper_extent + cavity_lower_extent) * 0.28);
                    const double cavity_edge_distance = std::min(
                        seam_delta + cavity_upper_extent,
                        cavity_lower_extent - seam_delta);
                    const double cavity_core = unit(cavity_edge_distance / cavity_feather);
                    const double cavity_mix = cavity_core *
                        smooth_unit((opening_strength - 0.08) / 0.45) * 0.90;
                    value = value * (1.0 - cavity_mix) + shadow_value * cavity_mix;
                }
                patch.premultiplied_bgra[output + channel] =
                    static_cast<std::uint8_t>(std::lround(value * alpha));
            }
            patch.premultiplied_bgra[output + 3U] = byte_from_unit(alpha);
        }
    }
    return patch;
}

ResidualPatch compose_atlas_residual(const CpuFrame& source,
                                     const TrackBinding& track,
                                     const TrackingEvidence& tracking,
                                     const CanonicalMouthPatch& primary,
                                     const CanonicalMouthPatch& secondary,
                                     const double secondary_weight,
                                     const Nanoseconds produced_at_ns) {
    ResidualPatch patch{};
    if (!valid_cpu_frame(source) || track != tracking.track || source.identity != tracking.frame ||
        !normalized_rect(tracking.mouth_bounds) || !valid_atlas_patch(primary) ||
        !valid_atlas_patch(secondary) || primary.width != secondary.width ||
        primary.height != secondary.height || primary.stride_bytes != secondary.stride_bytes ||
        !std::isfinite(secondary_weight) ||
        secondary_weight < 0.0 || secondary_weight > 1.0) {
        return patch;
    }
    const double source_width = static_cast<double>(source.lease.width);
    const double source_height = static_cast<double>(source.lease.height);
    const auto left = static_cast<std::uint32_t>(std::floor(tracking.mouth_bounds.x * source_width));
    const auto top = static_cast<std::uint32_t>(std::floor(tracking.mouth_bounds.y * source_height));
    const auto right = static_cast<std::uint32_t>(std::ceil(
        tracking.mouth_bounds.right() * source_width));
    const auto bottom = static_cast<std::uint32_t>(std::ceil(
        tracking.mouth_bounds.bottom() * source_height));
    if (right <= left || bottom <= top || right > source.lease.width || bottom > source.lease.height) {
        return patch;
    }
    const NormalizedRect output_bounds{
        static_cast<double>(left) / source_width,
        static_cast<double>(top) / source_height,
        static_cast<double>(right - left) / source_width,
        static_cast<double>(bottom - top) / source_height,
    };
    initialize_residual_metadata(patch, source, track, output_bounds,
                                 right - left, bottom - top, produced_at_ns);

    const double landmark_dx = tracking.mouth_landmarks.right_corner.x -
                               tracking.mouth_landmarks.left_corner.x;
    const double landmark_dy = tracking.mouth_landmarks.right_corner.y -
                               tracking.mouth_landmarks.left_corner.y;
    const double roll_radians = std::atan2(landmark_dy, landmark_dx);
    const double cosine = std::cos(roll_radians);
    const double sine = std::sin(roll_radians);
    const double primary_weight = 1.0 - secondary_weight;

    for (std::uint32_t y = 0; y < patch.height; ++y) {
        for (std::uint32_t x = 0; x < patch.width; ++x) {
            const double normalized_x = (static_cast<double>(x) + 0.5) /
                                            static_cast<double>(patch.width) * 2.0 - 1.0;
            const double normalized_y = (static_cast<double>(y) + 0.5) /
                                            static_cast<double>(patch.height) * 2.0 - 1.0;
            const double canonical_x = cosine * normalized_x + sine * normalized_y;
            const double canonical_y = -sine * normalized_x + cosine * normalized_y;
            if (std::abs(canonical_x) > 1.0 || std::abs(canonical_y) > 1.0) {
                continue;
            }
            const double sample_x = (canonical_x + 1.0) * 0.5 *
                                        static_cast<double>(primary.width) - 0.5;
            const double sample_y = (canonical_y + 1.0) * 0.5 *
                                        static_cast<double>(primary.height) - 0.5;
            const double clamped_x = std::clamp(
                sample_x, 0.0, static_cast<double>(primary.width - 1U));
            const double clamped_y = std::clamp(
                sample_y, 0.0, static_cast<double>(primary.height - 1U));
            const auto x0 = static_cast<std::uint32_t>(std::floor(clamped_x));
            const auto y0 = static_cast<std::uint32_t>(std::floor(clamped_y));
            const auto x1 = std::min(x0 + 1U, primary.width - 1U);
            const auto y1 = std::min(y0 + 1U, primary.height - 1U);
            const double fraction_x = clamped_x - static_cast<double>(x0);
            const double fraction_y = clamped_y - static_cast<double>(y0);
            const double weight_00 = (1.0 - fraction_x) * (1.0 - fraction_y);
            const double weight_10 = fraction_x * (1.0 - fraction_y);
            const double weight_01 = (1.0 - fraction_x) * fraction_y;
            const double weight_11 = fraction_x * fraction_y;
            const auto offset_00 = static_cast<std::size_t>(y0) * primary.stride_bytes +
                                   static_cast<std::size_t>(x0) * 4U;
            const auto offset_10 = static_cast<std::size_t>(y0) * primary.stride_bytes +
                                   static_cast<std::size_t>(x1) * 4U;
            const auto offset_01 = static_cast<std::size_t>(y1) * primary.stride_bytes +
                                   static_cast<std::size_t>(x0) * 4U;
            const auto offset_11 = static_cast<std::size_t>(y1) * primary.stride_bytes +
                                   static_cast<std::size_t>(x1) * 4U;
            const auto output = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            for (std::size_t channel = 0; channel < 4U; ++channel) {
                const auto sample = [offset_00, offset_10, offset_01, offset_11,
                                     weight_00, weight_10, weight_01, weight_11, channel](
                                        const CanonicalMouthPatch& state) {
                    return static_cast<double>(state.premultiplied_bgra[offset_00 + channel]) *
                               weight_00 +
                           static_cast<double>(state.premultiplied_bgra[offset_10 + channel]) *
                               weight_10 +
                           static_cast<double>(state.premultiplied_bgra[offset_01 + channel]) *
                               weight_01 +
                           static_cast<double>(state.premultiplied_bgra[offset_11 + channel]) *
                               weight_11;
                };
                const double value = sample(primary) * primary_weight +
                                     sample(secondary) * secondary_weight;
                patch.premultiplied_bgra[output + channel] = static_cast<std::uint8_t>(
                    std::clamp(std::lround(value), 0L, 255L));
            }
        }
    }
    return patch;
}

std::vector<std::uint8_t> composite_over_source(const CpuFrame& source,
                                                const ResidualPatch& residual) {
    auto output = source.bgra;
    if (!valid_cpu_frame(source) || residual.source_frame != source.identity ||
        residual.width == 0U || residual.height == 0U ||
        residual.stride_bytes < residual.width * 4U ||
        residual.premultiplied_bgra.size() <
            static_cast<std::size_t>(residual.stride_bytes) * residual.height) {
        return output;
    }
    const auto left = static_cast<std::uint32_t>(std::llround(
        residual.normalized_bounds.x * static_cast<double>(source.lease.width)));
    const auto top = static_cast<std::uint32_t>(std::llround(
        residual.normalized_bounds.y * static_cast<double>(source.lease.height)));
    if (left + residual.width > source.lease.width || top + residual.height > source.lease.height) {
        return output;
    }
    for (std::uint32_t y = 0; y < residual.height; ++y) {
        for (std::uint32_t x = 0; x < residual.width; ++x) {
            const auto patch_offset = static_cast<std::size_t>(y) * residual.stride_bytes +
                                      static_cast<std::size_t>(x) * 4U;
            const auto frame_offset = static_cast<std::size_t>(top + y) * source.lease.stride_bytes +
                                      static_cast<std::size_t>(left + x) * 4U;
            const std::uint32_t alpha = residual.premultiplied_bgra[patch_offset + 3U];
            const std::uint32_t inverse_alpha = 255U - alpha;
            for (std::size_t channel = 0; channel < 3U; ++channel) {
                const std::uint32_t foreground = residual.premultiplied_bgra[patch_offset + channel];
                const std::uint32_t background = output[frame_offset + channel];
                output[frame_offset + channel] = static_cast<std::uint8_t>(
                    std::min(255U, foreground + (background * inverse_alpha + 127U) / 255U));
            }
            output[frame_offset + 3U] = 255U;
        }
    }
    return output;
}

} // namespace npc::mouth
