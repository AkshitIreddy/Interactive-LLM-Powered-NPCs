#include "npc/mouth_worker/compositor.hpp"

#include <algorithm>
#include <array>
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

[[nodiscard]] double smoother_unit(const double value) noexcept {
    const double t = unit(value);
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

struct PixelPoint {
    double x{};
    double y{};
};

// A stable three-control parabola is preferable to a high-order fit here:
// landmark trackers occasionally reorder neighbouring confidence peaks, while
// this curve always stays pinned to both mouth corners and the measured centre.
[[nodiscard]] double bowed_curve_y(const PixelPoint left,
                                   const PixelPoint centre,
                                   const PixelPoint right,
                                   const double x) noexcept {
    const double width = std::max(1.0e-6, right.x - left.x);
    const double u = unit((x - left.x) / width);
    const double centre_u = std::clamp((centre.x - left.x) / width, 0.2, 0.8);
    const double line_y = left.y + (right.y - left.y) * u;
    const double line_at_centre = left.y + (right.y - left.y) * centre_u;
    const double denominator = std::max(0.16, 4.0 * centre_u * (1.0 - centre_u));
    return line_y + (centre.y - line_at_centre) * 4.0 * u * (1.0 - u) / denominator;
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

[[nodiscard]] double sample_channel_soft_x(const CpuFrame& frame,
                                           const double x,
                                           const double y,
                                           const std::size_t channel) noexcept {
    return (sample_channel(frame, x - 2.0, y, channel) +
            sample_channel(frame, x - 1.0, y, channel) * 2.0 +
            sample_channel(frame, x, y, channel) * 3.0 +
            sample_channel(frame, x + 1.0, y, channel) * 2.0 +
            sample_channel(frame, x + 2.0, y, channel)) / 9.0;
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

struct ContourWarpGeometry {
    std::array<PixelPoint, mouth_contour_point_count> source{};
    std::array<PixelPoint, mouth_contour_point_count> destination{};
    PixelPoint mouth_center{};
    PixelPoint inner_center{};
    PixelPoint horizontal_axis{};
    PixelPoint vertical_axis{};
    double mouth_width{};
    double horizontal_scale{1.0};
    double opening_correction{};
    double activity{};
};

[[nodiscard]] bool has_full_contour(const TrackingEvidence& tracking) noexcept {
    return tracking.mouth_landmarks.schema_version >= 2U &&
           tracking.mouth_landmarks.contour_points == mouth_contour_point_count;
}

[[nodiscard]] double projection(const PixelPoint point,
                                const PixelPoint origin,
                                const PixelPoint axis) noexcept {
    return (point.x - origin.x) * axis.x + (point.y - origin.y) * axis.y;
}

[[nodiscard]] ContourWarpGeometry contour_warp_geometry(
    const TrackingEvidence& tracking,
    const MouthCoefficients& coefficients,
    const double source_width,
    const double source_height) noexcept {
    ContourWarpGeometry geometry{};
    for (std::size_t index = 0U; index < geometry.source.size(); ++index) {
        geometry.source[index] = {
            tracking.mouth_landmarks.contour[index].x * source_width,
            tracking.mouth_landmarks.contour[index].y * source_height,
        };
        geometry.destination[index] = geometry.source[index];
    }
    const auto& left_corner = geometry.source[10U];
    const auto& right_corner = geometry.source[14U];
    const double dx = right_corner.x - left_corner.x;
    const double dy = right_corner.y - left_corner.y;
    geometry.mouth_width = std::hypot(dx, dy);
    geometry.mouth_center = {
        (left_corner.x + right_corner.x) * 0.5,
        (left_corner.y + right_corner.y) * 0.5,
    };
    if (geometry.mouth_width <= 1.0e-6) {
        return geometry;
    }
    geometry.horizontal_axis = {dx / geometry.mouth_width, dy / geometry.mouth_width};
    geometry.vertical_axis = {-geometry.horizontal_axis.y, geometry.horizontal_axis.x};

    const double smile = (coefficients.smile_left + coefficients.smile_right) * 0.5;
    const double rounding = std::max(coefficients.funnel, coefficients.pucker);
    const double rounded_weight = smoother_unit((rounding - 0.34) / 0.46);
    geometry.horizontal_scale = std::clamp(
        1.0 + smile * 0.16 - coefficients.funnel * 0.13 - coefficients.pucker * 0.25,
        0.64, 1.17);
    for (std::size_t index = 0U; index < geometry.destination.size(); ++index) {
        const double horizontal = projection(
            geometry.source[index], geometry.mouth_center, geometry.horizontal_axis);
        const double vertical = projection(
            geometry.source[index], geometry.mouth_center, geometry.vertical_axis);
        // Rounded speech cannot contract only the inner contour: the shared
        // corners then outrun the outer lip mesh and fold the centre into a
        // crinkled horizontal slit. Contract both contours together for an O,
        // while retaining the gentler outer-surface motion used by non-rounded
        // articulation.
        const double outer_scale_share = 0.30 + rounded_weight * 0.66;
        const double point_scale = index < 10U
            ? 1.0 + (geometry.horizontal_scale - 1.0) * outer_scale_share
            : geometry.horizontal_scale;
        geometry.destination[index] = {
            geometry.mouth_center.x + geometry.horizontal_axis.x * horizontal * point_scale +
                geometry.vertical_axis.x * vertical,
            geometry.mouth_center.y + geometry.horizontal_axis.y * horizontal * point_scale +
                geometry.vertical_axis.y * vertical,
        };
    }

    const auto mean_vertical = [&](const std::array<std::size_t, 3U>& indices) noexcept {
        double value = 0.0;
        for (const auto index : indices) {
            value += projection(
                geometry.destination[index], geometry.mouth_center, geometry.vertical_axis);
        }
        return value / static_cast<double>(indices.size());
    };
    constexpr std::array<std::size_t, 3U> upper_inner{11U, 12U, 13U};
    constexpr std::array<std::size_t, 3U> lower_inner{15U, 16U, 17U};
    const double measured_gap = std::max(
        0.0, mean_vertical(lower_inner) - mean_vertical(upper_inner));
    const double opening_strength = unit(
        coefficients.jaw_open * (1.0 - coefficients.lip_close * 0.92) +
        coefficients.lower_lip_depress * 0.16);
    const double opening_ease = smoother_unit(opening_strength * 1.08);
    // Jaw-only opening produced a six-pixel slit for the canonical rounded
    // viseme. A coordinated rounded aperture needs extra vertical separation
    // as its width contracts; this keeps O/U readable without changing the
    // public coefficient mapping.
    const double rounded_gap = rounded_weight * (0.118 + opening_ease * 0.038);
    const double articulation_gap = geometry.mouth_width *
        (0.018 + opening_ease * 0.177 + rounded_gap);
    const double close_weight = smoother_unit(
        (coefficients.lip_close - 0.48) / 0.42) *
        (1.0 - smoother_unit(opening_strength * 4.0));
    const double closed_gap = geometry.mouth_width * 0.006;
    const double open_target = std::max(measured_gap, articulation_gap);
    const double target_gap = open_target * (1.0 - close_weight) +
                              closed_gap * close_weight;
    // Signed correction matters when the captured actor is already speaking:
    // a bilabial cue must be able to close the current aperture rather than
    // retaining whatever mouth shape happened to be in the newest frame.
    geometry.opening_correction = target_gap - measured_gap;
    const auto shift_vertical = [&](const std::size_t index, const double amount) noexcept {
        geometry.destination[index].x += geometry.vertical_axis.x * amount;
        geometry.destination[index].y += geometry.vertical_axis.y * amount;
    };
    for (const auto index : upper_inner) {
        shift_vertical(index, -geometry.opening_correction * 0.34);
    }
    for (const auto index : lower_inner) {
        shift_vertical(index, geometry.opening_correction * 0.66);
    }
    for (const auto index : std::array<std::size_t, 5U>{0U, 1U, 2U, 3U, 4U}) {
        shift_vertical(index, -geometry.opening_correction * 0.10);
    }
    for (const auto index : std::array<std::size_t, 5U>{5U, 6U, 7U, 8U, 9U}) {
        shift_vertical(index, geometry.opening_correction * 0.48);
    }

    geometry.inner_center = {};
    for (std::size_t index = 10U; index < 18U; ++index) {
        geometry.inner_center.x += geometry.destination[index].x;
        geometry.inner_center.y += geometry.destination[index].y;
    }
    geometry.inner_center.x /= 8.0;
    geometry.inner_center.y /= 8.0;

    geometry.activity = unit(std::max({
        opening_strength,
        std::abs(geometry.horizontal_scale - 1.0) * 3.2,
        std::abs(geometry.opening_correction) /
            std::max(1.0, geometry.mouth_width) * 5.0,
    }));
    return geometry;
}

template <std::size_t Size>
[[nodiscard]] bool point_inside_polygon(
    const PixelPoint point,
    const std::array<PixelPoint, Size>& polygon) noexcept {
    bool inside = false;
    for (std::size_t current = 0U, previous = Size - 1U; current < Size;
         previous = current++) {
        const auto& first = polygon[current];
        const auto& second = polygon[previous];
        const bool crosses = (first.y > point.y) != (second.y > point.y);
        if (!crosses) continue;
        const double vertical_span = second.y - first.y;
        if (std::abs(vertical_span) <= 1.0e-9) continue;
        const double intersection_x = (second.x - first.x) * (point.y - first.y) /
                                          vertical_span +
                                      first.x;
        if (point.x < intersection_x) inside = !inside;
    }
    return inside;
}

[[nodiscard]] double squared_segment_distance(const PixelPoint point,
                                               const PixelPoint first,
                                               const PixelPoint second) noexcept {
    const double dx = second.x - first.x;
    const double dy = second.y - first.y;
    const double length_squared = dx * dx + dy * dy;
    const double amount = length_squared <= 1.0e-9
        ? 0.0
        : unit(((point.x - first.x) * dx + (point.y - first.y) * dy) / length_squared);
    const double nearest_x = first.x + dx * amount;
    const double nearest_y = first.y + dy * amount;
    const double distance_x = point.x - nearest_x;
    const double distance_y = point.y - nearest_y;
    return distance_x * distance_x + distance_y * distance_y;
}

template <std::size_t Size>
[[nodiscard]] double polygon_edge_distance(
    const PixelPoint point,
    const std::array<PixelPoint, Size>& polygon) noexcept {
    double squared = std::numeric_limits<double>::max();
    for (std::size_t index = 0U; index < Size; ++index) {
        squared = std::min(squared, squared_segment_distance(
            point, polygon[index], polygon[(index + 1U) % Size]));
    }
    return std::sqrt(squared);
}

[[nodiscard]] std::array<PixelPoint, 12U> outer_contour(
    const std::array<PixelPoint, mouth_contour_point_count>& contour) noexcept {
    // OpenSeeFace's 66-point layout omits the two dedicated outer mouth
    // corners. Points 48..57 are the five upper and five lower outer-lip
    // samples; points 58 and 62 are shared by the outer and inner contours.
    return {
        contour[10U], contour[0U], contour[1U], contour[2U], contour[3U],
        contour[4U], contour[14U], contour[5U], contour[6U], contour[7U],
        contour[8U], contour[9U],
    };
}

[[nodiscard]] std::array<PixelPoint, 8U> inner_contour(
    const std::array<PixelPoint, mouth_contour_point_count>& contour) noexcept {
    std::array<PixelPoint, 8U> result{};
    std::copy_n(contour.begin() + 10U, result.size(), result.begin());
    return result;
}

void render_contour_warp(const CpuFrame& source,
                         const ContourWarpGeometry& geometry,
                         const std::uint32_t left,
                         const std::uint32_t top,
    ResidualPatch& patch) noexcept {
    if (geometry.activity <= 1.0e-6 || geometry.mouth_width <= 1.0e-6) return;
    constexpr std::array<std::size_t, 12U> outer_indices{
        10U, 0U, 1U, 2U, 3U, 4U, 14U, 5U, 6U, 7U, 8U, 9U,
    };
    constexpr std::size_t anchor_offset = mouth_contour_point_count;
    std::array<PixelPoint, mouth_contour_point_count + outer_indices.size()> source_mesh{};
    std::array<PixelPoint, mouth_contour_point_count + outer_indices.size()> destination_mesh{};
    std::copy(geometry.source.begin(), geometry.source.end(), source_mesh.begin());
    std::copy(geometry.destination.begin(), geometry.destination.end(), destination_mesh.begin());
    std::array<PixelPoint, outer_indices.size()> anchor_ring{};
    for (std::size_t index = 0U; index < outer_indices.size(); ++index) {
        const auto& outer = geometry.source[outer_indices[index]];
        const double horizontal = projection(
            outer, geometry.mouth_center, geometry.horizontal_axis);
        double vertical = projection(
            outer, geometry.mouth_center, geometry.vertical_axis) * 1.72;
        const double minimum_vertical = geometry.mouth_width * 0.075;
        if (index > 0U && index < 6U) {
            vertical = std::min(vertical, -minimum_vertical);
        } else if (index > 6U) {
            vertical = std::max(vertical, minimum_vertical);
        }
        const PixelPoint anchor{
            geometry.mouth_center.x + geometry.horizontal_axis.x * horizontal * 1.34 +
                geometry.vertical_axis.x * vertical,
            geometry.mouth_center.y + geometry.horizontal_axis.y * horizontal * 1.34 +
                geometry.vertical_axis.y * vertical,
        };
        anchor_ring[index] = anchor;
        source_mesh[anchor_offset + index] = anchor;
        destination_mesh[anchor_offset + index] = anchor;
    }
    // A fixed lip mesh is stable for OpenSeeFace's ordered 10-point outer and
    // 8-point inner contours. A second fixed ring anchors current-frame skin
    // around the mouth. Its annulus overwrites the old lip silhouette when a
    // rounded viseme contracts, preventing doubled corners without painting a
    // generated face patch or moving any pixel beyond this bounded ring.
    constexpr std::array<std::array<std::size_t, 3U>, 16U> lip_triangles{{
        // Upper lip, traversing the shared left corner to shared right corner.
        {{10U, 0U, 11U}}, {{0U, 1U, 11U}}, {{1U, 12U, 11U}},
        {{1U, 2U, 12U}}, {{2U, 3U, 12U}}, {{3U, 13U, 12U}},
        {{3U, 4U, 13U}}, {{4U, 14U, 13U}},
        // Lower lip, traversing the same corners through the five lower points.
        {{10U, 17U, 9U}}, {{9U, 17U, 8U}}, {{8U, 17U, 16U}},
        {{8U, 16U, 7U}}, {{7U, 16U, 6U}}, {{6U, 16U, 15U}},
        {{6U, 15U, 5U}}, {{5U, 15U, 14U}},
    }};
    const double activity = smoother_unit(geometry.activity * 2.2);
    const auto render_triangle = [&](const std::array<std::size_t, 3U>& triangle) {
        const auto& first = destination_mesh[triangle[0U]];
        const auto& second = destination_mesh[triangle[1U]];
        const auto& third = destination_mesh[triangle[2U]];
        const double denominator =
            (second.y - third.y) * (first.x - third.x) +
            (third.x - second.x) * (first.y - third.y);
        if (std::abs(denominator) <= 1.0e-6) return;
        const auto minimum_x = static_cast<std::int32_t>(std::floor(
            std::min({first.x, second.x, third.x}) - static_cast<double>(left)));
        const auto maximum_x = static_cast<std::int32_t>(std::ceil(
            std::max({first.x, second.x, third.x}) - static_cast<double>(left)));
        const auto minimum_y = static_cast<std::int32_t>(std::floor(
            std::min({first.y, second.y, third.y}) - static_cast<double>(top)));
        const auto maximum_y = static_cast<std::int32_t>(std::ceil(
            std::max({first.y, second.y, third.y}) - static_cast<double>(top)));
        for (std::int32_t patch_y = std::max(0, minimum_y);
             patch_y <= std::min<std::int32_t>(patch.height - 1U, maximum_y);
             ++patch_y) {
            for (std::int32_t patch_x = std::max(0, minimum_x);
                 patch_x <= std::min<std::int32_t>(patch.width - 1U, maximum_x);
                 ++patch_x) {
                const PixelPoint point{
                    static_cast<double>(left + patch_x) + 0.5,
                    static_cast<double>(top + patch_y) + 0.5,
                };
                const double first_weight =
                    ((second.y - third.y) * (point.x - third.x) +
                     (third.x - second.x) * (point.y - third.y)) / denominator;
                const double second_weight =
                    ((third.y - first.y) * (point.x - third.x) +
                     (first.x - third.x) * (point.y - third.y)) / denominator;
                const double third_weight = 1.0 - first_weight - second_weight;
                if (first_weight < -1.0e-6 || second_weight < -1.0e-6 ||
                    third_weight < -1.0e-6) {
                    continue;
                }
                const auto& source_first = source_mesh[triangle[0U]];
                const auto& source_second = source_mesh[triangle[1U]];
                const auto& source_third = source_mesh[triangle[2U]];
                const double sample_x = source_first.x * first_weight +
                    source_second.x * second_weight + source_third.x * third_weight;
                const double sample_y = source_first.y * first_weight +
                    source_second.y * second_weight + source_third.y * third_weight;
                const double edge_distance = polygon_edge_distance(point, anchor_ring);
                const double alpha = smoother_unit(edge_distance / 1.4) * activity;
                if (alpha <= 1.0e-6) continue;
                const auto output = static_cast<std::size_t>(patch_y) * patch.stride_bytes +
                                    static_cast<std::size_t>(patch_x) * 4U;
                for (std::size_t channel = 0U; channel < 3U; ++channel) {
                    patch.premultiplied_bgra[output + channel] =
                        static_cast<std::uint8_t>(std::clamp(std::lround(
                            sample_channel(source, sample_x, sample_y, channel) * alpha),
                            0L, 255L));
                }
                patch.premultiplied_bgra[output + 3U] = byte_from_unit(alpha);
            }
        }
    };
    for (const auto& triangle : lip_triangles) {
        render_triangle(triangle);
    }
    for (std::size_t index = 0U; index < outer_indices.size(); ++index) {
        const std::size_t next = (index + 1U) % outer_indices.size();
        render_triangle({{
            anchor_offset + index,
            outer_indices[index],
            outer_indices[next],
        }});
        render_triangle({{
            anchor_offset + index,
            outer_indices[next],
            anchor_offset + next,
        }});
    }
}

struct PremultipliedPatchSample {
    std::array<double, 3U> colour{};
    double alpha{};
};

[[nodiscard]] PremultipliedPatchSample sample_canonical_patch(
    const CanonicalMouthPatch& patch,
    const double canonical_x,
    const double canonical_y) noexcept {
    const double sample_x = (canonical_x + 1.0) * 0.5 *
                                static_cast<double>(patch.width) - 0.5;
    const double sample_y = (canonical_y + 1.0) * 0.5 *
                                static_cast<double>(patch.height) - 0.5;
    const double clamped_x = std::clamp(
        sample_x, 0.0, static_cast<double>(patch.width - 1U));
    const double clamped_y = std::clamp(
        sample_y, 0.0, static_cast<double>(patch.height - 1U));
    const auto x0 = static_cast<std::uint32_t>(std::floor(clamped_x));
    const auto y0 = static_cast<std::uint32_t>(std::floor(clamped_y));
    const auto x1 = std::min(x0 + 1U, patch.width - 1U);
    const auto y1 = std::min(y0 + 1U, patch.height - 1U);
    const double fraction_x = clamped_x - static_cast<double>(x0);
    const double fraction_y = clamped_y - static_cast<double>(y0);
    const std::array<double, 4U> weights{
        (1.0 - fraction_x) * (1.0 - fraction_y),
        fraction_x * (1.0 - fraction_y),
        (1.0 - fraction_x) * fraction_y,
        fraction_x * fraction_y,
    };
    const std::array<std::size_t, 4U> offsets{
        static_cast<std::size_t>(y0) * patch.stride_bytes +
            static_cast<std::size_t>(x0) * 4U,
        static_cast<std::size_t>(y0) * patch.stride_bytes +
            static_cast<std::size_t>(x1) * 4U,
        static_cast<std::size_t>(y1) * patch.stride_bytes +
            static_cast<std::size_t>(x0) * 4U,
        static_cast<std::size_t>(y1) * patch.stride_bytes +
            static_cast<std::size_t>(x1) * 4U,
    };
    PremultipliedPatchSample result{};
    for (std::size_t sample = 0U; sample < offsets.size(); ++sample) {
        for (std::size_t channel = 0U; channel < result.colour.size(); ++channel) {
            result.colour[channel] +=
                static_cast<double>(patch.premultiplied_bgra[offsets[sample] + channel]) *
                weights[sample];
        }
        result.alpha +=
            static_cast<double>(patch.premultiplied_bgra[offsets[sample] + 3U]) / 255.0 *
            weights[sample];
    }
    return result;
}

void overlay_patch_sample(ResidualPatch& destination,
                          const std::uint32_t x,
                          const std::uint32_t y,
                          const PremultipliedPatchSample& sample,
                          const double support) noexcept {
    const double alpha = sample.alpha * support;
    if (alpha <= 1.0e-6) return;
    const auto output = static_cast<std::size_t>(y) * destination.stride_bytes +
                        static_cast<std::size_t>(x) * 4U;
    const double inverse_alpha = 1.0 - alpha;
    for (std::size_t channel = 0U; channel < sample.colour.size(); ++channel) {
        const double foreground = sample.colour[channel] * support;
        const double value = foreground +
                             destination.premultiplied_bgra[output + channel] *
                                 inverse_alpha;
        destination.premultiplied_bgra[output + channel] =
            static_cast<std::uint8_t>(std::clamp(std::lround(value), 0L, 255L));
    }
    const double output_alpha = alpha * 255.0 +
        destination.premultiplied_bgra[output + 3U] * inverse_alpha;
    destination.premultiplied_bgra[output + 3U] =
        static_cast<std::uint8_t>(std::clamp(std::lround(output_alpha), 0L, 255L));
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
    double sum = 0.0;
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
        sum += mono;
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

    // Energy determines how strongly the mouth articulates, but it cannot tell
    // an /oo/ from an /ee/.  Extract a tiny deterministic spectral descriptor so
    // provider-less/live PCM still selects visibly different broad mouth shapes.
    // Seven Goertzel bins cost O(7N), allocate nothing, and are intentionally not
    // an ASR model: exact provider phoneme/viseme timing remains the preferred
    // drive whenever it is available.
    constexpr std::array<double, 7> analysis_frequencies{
        260.0, 520.0, 780.0, 1'100.0, 1'600.0, 2'300.0, 3'400.0};
    std::array<double, analysis_frequencies.size()> q1{};
    std::array<double, analysis_frequencies.size()> q2{};
    std::array<double, analysis_frequencies.size()> goertzel_coefficients{};
    std::array<bool, analysis_frequencies.size()> enabled{};
    constexpr double pi = 3.14159265358979323846;
    for (std::size_t band = 0; band < analysis_frequencies.size(); ++band) {
        enabled[band] = analysis_frequencies[band] < static_cast<double>(sample_rate) * 0.47;
        if (enabled[band]) {
            goertzel_coefficients[band] =
                2.0 * std::cos(2.0 * pi * analysis_frequencies[band] /
                               static_cast<double>(sample_rate));
        }
    }

    const auto total_frames = interleaved_pcm.size() / channels;
    const auto analysis_frames = std::min<std::size_t>(total_frames, 2'048U);
    const auto first_analysis_frame = total_frames - analysis_frames;
    const double dc = sum / static_cast<double>(finite_samples);
    std::size_t window_index = 0U;
    double previous_analysis_sample = 0.0;
    for (std::size_t frame_index = first_analysis_frame;
         frame_index < total_frames;
         ++frame_index) {
        const std::size_t frame = frame_index * channels;
        double mono = 0.0;
        std::size_t used_channels = 0U;
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
        mono = mono / static_cast<double>(used_channels) - dc;
        // Speech formants, rather than a speaker's fundamental pitch, should
        // choose the mouth shape. A gentle pre-emphasis keeps a low male F0
        // from making nearly every frame look like an exaggerated /oo/.
        const double emphasized = mono - previous_analysis_sample * 0.86;
        previous_analysis_sample = mono;
        const double phase = analysis_frames > 1U
            ? static_cast<double>(window_index) /
                  static_cast<double>(analysis_frames - 1U)
            : 0.5;
        const double windowed = emphasized * (0.5 - 0.5 * std::cos(2.0 * pi * phase));
        for (std::size_t band = 0; band < analysis_frequencies.size(); ++band) {
            if (!enabled[band]) {
                continue;
            }
            const double q0 = windowed + goertzel_coefficients[band] * q1[band] - q2[band];
            q2[band] = q1[band];
            q1[band] = q0;
        }
        ++window_index;
    }

    std::array<double, analysis_frequencies.size()> band_energy{};
    double spectral_energy = 1.0e-12;
    for (std::size_t band = 0; band < analysis_frequencies.size(); ++band) {
        if (!enabled[band]) {
            continue;
        }
        band_energy[band] = std::max(
            0.0,
            q1[band] * q1[band] + q2[band] * q2[band] -
                goertzel_coefficients[band] * q1[band] * q2[band]);
        spectral_energy += band_energy[band];
    }
    const auto ratio = [&](const std::size_t first, const std::size_t last) noexcept {
        double energy = 0.0;
        for (std::size_t band = first; band <= last; ++band) {
            energy += band_energy[band];
        }
        return energy / spectral_energy;
    };
    const double low_ratio = ratio(0U, 1U);
    const double open_mid_ratio = ratio(2U, 3U);
    const double bright_ratio = ratio(4U, 5U);
    const double fricative_ratio = ratio(6U, 6U);
    const double rounded = smooth_unit((low_ratio - 0.18) / 0.62);
    const double open_vowel = smooth_unit((open_mid_ratio - 0.12) / 0.68);
    const double spread = smooth_unit((bright_ratio - 0.10) / 0.70) *
                          (1.0 - 0.35 * fricative_ratio);
    const double frication = std::max(
        smooth_unit((fricative_ratio - 0.06) / 0.62),
        smooth_unit((crossing_ratio - 0.08) * 3.2));

    MouthCoefficients value{};
    value.jaw_open = open * unit(0.84 + open_vowel * 0.16 + spread * 0.03 - frication * 0.06);
    value.lip_close = unit(1.0 - open * 1.8);
    value.funnel = open * unit(rounded * 0.82 + frication * 0.24);
    value.pucker = open * rounded * (1.0 - frication * 0.55) * 0.58;
    value.smile_left = open * spread * (1.0 - frication * 0.4) * 0.72;
    value.smile_right = value.smile_left;
    value.upper_lip_raise = open * unit(spread * 0.28 + frication * 0.34);
    value.lower_lip_depress = open * unit(0.46 + open_vowel * 0.42 + spread * 0.08);
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

CanonicalMouthPatch extract_canonical_mouth_patch(
    const CpuFrame& source,
    const TrackingEvidence& tracking,
    const std::uint32_t canonical_width,
    const std::uint32_t canonical_height) {
    CanonicalMouthPatch patch{};
    const auto& landmarks = tracking.mouth_landmarks;
    const auto finite_point = [](const NormalizedLandmark& point) {
        return std::isfinite(point.x) && std::isfinite(point.y) &&
               point.x >= 0.0 && point.x <= 1.0 && point.y >= 0.0 && point.y <= 1.0;
    };
    if (!valid_cpu_frame(source) || tracking.frame != source.identity ||
        !finite_point(landmarks.left_corner) || !finite_point(landmarks.right_corner) ||
        !finite_point(landmarks.upper_lip_center) || !finite_point(landmarks.lower_lip_center) ||
        landmarks.left_corner.x >= landmarks.right_corner.x ||
        canonical_width < 16U || canonical_height < 16U ||
        canonical_width > 512U || canonical_height > 512U) {
        return patch;
    }

    const double source_width = static_cast<double>(source.lease.width);
    const double source_height = static_cast<double>(source.lease.height);
    const PixelPoint left_corner{landmarks.left_corner.x * source_width,
                                 landmarks.left_corner.y * source_height};
    const PixelPoint right_corner{landmarks.right_corner.x * source_width,
                                  landmarks.right_corner.y * source_height};
    const double corner_dx = right_corner.x - left_corner.x;
    const double corner_dy = right_corner.y - left_corner.y;
    const double mouth_width = std::hypot(corner_dx, corner_dy);
    if (!std::isfinite(mouth_width) || mouth_width < 4.0 ||
        mouth_width > source_width * 0.55) {
        return patch;
    }
    const double center_x = (left_corner.x + right_corner.x) * 0.5;
    const double center_y = (landmarks.upper_lip_center.y + landmarks.lower_lip_center.y) *
                            0.5 * source_height;
    const double roll = std::atan2(corner_dy, corner_dx);
    const double cosine = std::cos(roll);
    const double sine = std::sin(roll);
    const double crop_width = mouth_width * 1.34;
    const double crop_height = crop_width * 0.625;

    patch.width = canonical_width;
    patch.height = canonical_height;
    patch.stride_bytes = canonical_width * 4U;
    patch.enrolled_pose = tracking.pose;
    patch.premultiplied_bgra.assign(
        static_cast<std::size_t>(patch.stride_bytes) * patch.height, 0U);
    for (std::uint32_t y = 0U; y < patch.height; ++y) {
        for (std::uint32_t x = 0U; x < patch.width; ++x) {
            const double nx = (static_cast<double>(x) + 0.5) /
                                  static_cast<double>(patch.width) * 2.0 - 1.0;
            const double ny = (static_cast<double>(y) + 0.5) /
                                  static_cast<double>(patch.height) * 2.0 - 1.0;
            const double local_x = nx * crop_width * 0.5;
            const double local_y = ny * crop_height * 0.5;
            const double sample_x = center_x + cosine * local_x - sine * local_y;
            const double sample_y = center_y + sine * local_x + cosine * local_y;
            const double superellipse = std::pow(std::abs(nx), 3.4) +
                                        std::pow(std::abs(ny), 2.8);
            const double alpha = smoother_unit((1.0 - superellipse) / 0.22);
            if (alpha <= 0.0 || sample_x < 0.0 || sample_y < 0.0 ||
                sample_x > source_width - 1.0 || sample_y > source_height - 1.0) {
                continue;
            }
            const auto output = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            for (std::size_t channel = 0U; channel < 3U; ++channel) {
                patch.premultiplied_bgra[output + channel] = static_cast<std::uint8_t>(
                    std::clamp(std::lround(sample_channel(source, sample_x, sample_y, channel) *
                                           alpha), 0L, 255L));
            }
            patch.premultiplied_bgra[output + 3U] = byte_from_unit(alpha);
        }
    }
    return patch;
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
    if (has_full_contour(tracking)) {
        const auto geometry = contour_warp_geometry(
            tracking, coefficients, source_width, source_height);
        render_contour_warp(source, geometry, left, top, patch);
        return patch;
    }

    const double smile = (coefficients.smile_left + coefficients.smile_right) * 0.5;
    const double rounding = std::max(coefficients.funnel, coefficients.pucker);
    const double horizontal_scale = std::clamp(
        1.0 + smile * 0.20 - coefficients.funnel * 0.10 -
            coefficients.pucker * 0.14,
        0.74, 1.20);
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
    const bool has_contour = tracking.mouth_landmarks.schema_version >= 2U &&
        tracking.mouth_landmarks.contour_points ==
            tracking.mouth_landmarks.contour.size();
    const auto contour_pixel = [&](const std::size_t point) {
        const auto& landmark = tracking.mouth_landmarks.contour[point];
        return PixelPoint{landmark.x * source_width - static_cast<double>(left),
                          landmark.y * source_height - static_cast<double>(top)};
    };
    const PixelPoint curve_left{left_corner.first, left_corner.second};
    const PixelPoint curve_right{right_corner.first, right_corner.second};
    PixelPoint upper_centre{upper_lip.first, upper_lip.second};
    PixelPoint lower_centre{lower_lip.first, lower_lip.second};
    if (has_contour) {
        upper_centre = {
            (contour_pixel(11U).x + contour_pixel(12U).x + contour_pixel(13U).x) / 3.0,
            (contour_pixel(11U).y + contour_pixel(12U).y + contour_pixel(13U).y) / 3.0,
        };
        lower_centre = {
            (contour_pixel(15U).x + contour_pixel(16U).x + contour_pixel(17U).x) / 3.0,
            (contour_pixel(15U).y + contour_pixel(16U).y + contour_pixel(17U).y) / 3.0,
        };
    }
    const double landmark_mid_y = (upper_centre.y + lower_centre.y) * 0.5;
    const double lip_landmark_span = std::max(1.0, lower_centre.y - upper_centre.y);
    double outer_top_y = upper_centre.y - std::max(1.5, lip_landmark_span * 0.8);
    double outer_bottom_y = lower_centre.y + std::max(1.5, lip_landmark_span * 0.8);
    if (has_contour) {
        outer_top_y = std::numeric_limits<double>::max();
        outer_bottom_y = std::numeric_limits<double>::lowest();
        for (std::size_t point = 0U; point < 10U; ++point) {
            outer_top_y = std::min(outer_top_y, contour_pixel(point).y);
            outer_bottom_y = std::max(outer_bottom_y, contour_pixel(point).y);
        }
    }
    const double outer_lip_span = std::max(2.0, outer_bottom_y - outer_top_y);
    const double corner_span = curve_right.x - curve_left.x;
    const double measured_seam_slope = std::abs(corner_span) > 1.0e-6
        ? std::clamp((curve_right.y - curve_left.y) / corner_span, -0.08, 0.08)
        : 0.0;

    // OpenSeeFace is optimized for stable avatar controls rather than exact
    // source-pixel fitting. Align its contour to the darkest current-frame lip
    // contact within a tightly bounded vertical neighbourhood, then retain the
    // measured bow and roll instead of replacing them with a straight seam.
    double visual_seam_center_y = landmark_mid_y;
    double best_seam_luma = std::numeric_limits<double>::max();
    constexpr int seam_row_candidates = 11;
    constexpr int seam_column_samples = 19;
    for (int row_candidate = 0; row_candidate < seam_row_candidates; ++row_candidate) {
        const double fraction = 0.48 + 0.32 *
            static_cast<double>(row_candidate) /
            static_cast<double>(seam_row_candidates - 1);
        const double candidate_center_y = has_contour
            ? outer_top_y + outer_lip_span * fraction
            : upper_centre.y + lip_landmark_span * (0.50 + fraction * 0.40);
        double weighted_luma = 0.0;
        double total_weight = 0.0;
        for (int column_sample = 0; column_sample < seam_column_samples; ++column_sample) {
            const double mouth_sample_x = -0.72 + 1.44 *
                static_cast<double>(column_sample) /
                static_cast<double>(seam_column_samples - 1);
            const double candidate_x = mouth_center_x + mouth_sample_x * mouth_half_width;
            const double candidate_y = candidate_center_y +
                (candidate_x - mouth_center_x) * measured_seam_slope;
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
    const PixelPoint legacy_curve_left{
        curve_left.x,
        visual_seam_center_y + (curve_left.x - mouth_center_x) * measured_seam_slope,
    };
    const PixelPoint legacy_curve_right{
        curve_right.x,
        visual_seam_center_y + (curve_right.x - mouth_center_x) * measured_seam_slope,
    };
    const double opening_strength = unit(
        coefficients.jaw_open * (1.0 - coefficients.lip_close * 0.90) +
        coefficients.lower_lip_depress * 0.18);
    const double maximum_added_gap = std::clamp(
        std::min(mouth_half_width * 0.62, static_cast<double>(patch.height) * 0.58),
        3.0, 28.0);
    const double added_gap = smoother_unit(opening_strength * 1.18) * maximum_added_gap;
    const double upper_lip_thickness = std::clamp(
        upper_centre.y - outer_top_y, 1.5, std::max(2.0, mouth_half_width * 0.22));
    const double lower_lip_thickness = std::clamp(
        outer_bottom_y - lower_centre.y, 1.5, std::max(2.0, mouth_half_width * 0.25));
    const double articulation_activity = unit(
        std::max(opening_strength, std::abs(horizontal_scale - 1.0) * 3.2));

    for (std::uint32_t y = 0; y < patch.height; ++y) {
        for (std::uint32_t x = 0; x < patch.width; ++x) {
            const double pixel_x = static_cast<double>(x) + 0.5;
            const double pixel_y = static_cast<double>(y) + 0.5;
            const double target_half_width = mouth_half_width * horizontal_scale;
            const double target_x = (pixel_x - mouth_center_x) / target_half_width;
            if (std::abs(target_x) >= 1.14 || articulation_activity <= 1.0e-5) {
                continue;
            }
            const double source_patch_x = mouth_center_x +
                (pixel_x - mouth_center_x) / horizontal_scale;
            PixelPoint legacy_seam_centre{mouth_center_x, visual_seam_center_y};
            const double visual_contact_curve = bowed_curve_y(
                legacy_curve_left, legacy_seam_centre, legacy_curve_right,
                source_patch_x);
            // Tracker inner-lip points describe articulation well but can sit
            // several pixels inside an upper lip on moustached or shaded faces.
            // The darkest current-frame contact curve is the only safe place to
            // begin a synthetic cavity. Preserve that upper surface exactly and
            // use only a tiny fraction of a measured pre-existing aperture.
            const double tracked_source_gap = has_contour
                ? std::max(0.0,
                    bowed_curve_y(curve_left, lower_centre, curve_right,
                                   source_patch_x) -
                    bowed_curve_y(curve_left, upper_centre, curve_right,
                                   source_patch_x))
                : 0.0;
            const double source_upper_curve = visual_contact_curve;
            const double source_lower_curve = visual_contact_curve +
                std::min(1.0, tracked_source_gap * 0.12);
            const double ellipse = std::sqrt(std::max(0.0, 1.0 - target_x * target_x));
            const double rounded_taper = std::pow(ellipse, 1.48 + rounding * 0.30);
            const double local_added_gap = added_gap * rounded_taper;
            const double upper_share = 0.0;
            const double lower_share = 0.86 + coefficients.lower_lip_depress * 0.10;
            const double target_upper_curve = source_upper_curve -
                local_added_gap * upper_share;
            const double target_lower_curve = source_lower_curve +
                local_added_gap * lower_share;
            const double target_gap = std::max(0.0, target_lower_curve - target_upper_curve);
            const double upper_effect_top = source_upper_curve - upper_lip_thickness * 1.55 -
                                            local_added_gap * 0.10;
            const double lower_effect_bottom = source_lower_curve + lower_lip_thickness * 2.25 +
                                               local_added_gap * 1.22;
            if (pixel_y <= upper_effect_top || pixel_y >= lower_effect_bottom) {
                continue;
            }
            const double horizontal_feather =
                smoother_unit((1.14 - std::abs(target_x)) / 0.18);
            const double edge_distance = std::min(pixel_y - upper_effect_top,
                                                  lower_effect_bottom - pixel_y);
            const double vertical_feather = smoother_unit(
                edge_distance / std::max(1.0, (upper_lip_thickness + lower_lip_thickness) * 0.42));
            const double alpha = horizontal_feather * vertical_feather *
                                 smoother_unit(articulation_activity * 2.2) * 0.985;
            const bool in_cavity = target_gap > 1.0 &&
                pixel_y > target_upper_curve && pixel_y < target_lower_curve;
            double source_patch_y = pixel_y;
            if (in_cavity) {
                const double cavity_position = unit(
                    (pixel_y - target_upper_curve) / std::max(1.0, target_gap));
                source_patch_y = source_upper_curve +
                    (source_lower_curve - source_upper_curve) * cavity_position;
            } else if (pixel_y <= target_upper_curve) {
                const double displacement = target_upper_curve - source_upper_curve;
                const double distance = target_upper_curve - pixel_y;
                const double falloff = smoother_unit(
                    (upper_lip_thickness * 1.65 - distance) /
                    std::max(1.0, upper_lip_thickness * 1.65));
                source_patch_y -= displacement * falloff;
            } else if (pixel_y >= target_lower_curve) {
                const double displacement = target_lower_curve - source_lower_curve;
                const double distance = pixel_y - target_lower_curve;
                const double falloff = smoother_unit(
                    (lower_lip_thickness * 2.25 + local_added_gap * 0.44 - distance) /
                    std::max(1.0, lower_lip_thickness * 2.25 + local_added_gap * 0.44));
                source_patch_y -= displacement * falloff;
            }
            // Horizontal articulation must never turn moustache or upper-lip
            // texture into a dark cavity. Keep that source surface at its exact
            // current-frame x coordinate; shape the aperture and lower jaw.
            const double surface_sample_x = pixel_y <= target_upper_curve
                ? pixel_x
                : source_patch_x;
            const double sample_x = static_cast<double>(left) +
                                     surface_sample_x - 0.5;
            const double sample_y = static_cast<double>(top) +
                                     source_patch_y - 0.5;
            const auto output = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const double seam_sample_x = static_cast<double>(left) + source_patch_x - 0.5;
            const double source_seam_y = (source_upper_curve + source_lower_curve) * 0.5;
            double cavity_sample_y = static_cast<double>(top) + source_seam_y - 0.5;
            if (in_cavity) {
                // Expand the darkest real contact texel into a curved oral
                // interior, preserving the source lighting and local beard/lip
                // colour. A soft tongue tint and conditional enamel reflection
                // add depth without replacing the source identity texture.
                const double search_radius = std::max(
                    1.0, (upper_lip_thickness + lower_lip_thickness) * 0.44);
                double darkest_luma = std::numeric_limits<double>::max();
                for (int candidate = -3; candidate <= 3; ++candidate) {
                    const double candidate_y = static_cast<double>(top) + source_seam_y - 0.5 +
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
            const double cavity_v = in_cavity
                ? unit((pixel_y - target_upper_curve) / std::max(1.0, target_gap))
                : 0.0;
            const double cavity_edge = in_cavity
                ? smoother_unit(std::min(pixel_y - target_upper_curve,
                                         target_lower_curve - pixel_y) / 2.1)
                : 0.0;
            const double centrality = smoother_unit((0.94 - std::abs(target_x)) / 0.26);
            const double tooth_bottom = 0.30 +
                0.055 * (1.0 - target_x * target_x) +
                0.010 * std::cos(target_x * 3.0 * 3.14159265358979323846);
            const double teeth_strength = has_contour && in_cavity && target_gap >= 5.0
                ? smoother_unit((opening_strength - 0.14) / 0.30) *
                    (1.0 - rounding * 0.82) * centrality *
                    smoother_unit((cavity_v - 0.10) / 0.085) *
                    smoother_unit((tooth_bottom - cavity_v) / 0.075) * 0.96
                : 0.0;
            const double tongue_strength = has_contour && in_cavity
                ? smoother_unit((opening_strength - 0.34) / 0.42) * centrality *
                    smoother_unit((cavity_v - 0.58) / 0.15) * 0.28
                : 0.0;
            const double skin_sample_y = static_cast<double>(top) + outer_top_y -
                upper_lip_thickness * 0.65 - 0.5;
            const double skin_blue = sample_channel_soft_x(
                source, seam_sample_x, skin_sample_y, 0U);
            const double skin_green = sample_channel_soft_x(
                source, seam_sample_x, skin_sample_y, 1U);
            const double skin_red = sample_channel_soft_x(
                source, seam_sample_x, skin_sample_y, 2U);
            const double skin_luma = skin_blue * 0.114 + skin_green * 0.587 +
                                     skin_red * 0.299;
            const double enamel_luma = std::clamp(skin_luma * 0.55 + 110.0,
                                                  154.0, 228.0);
            for (std::size_t channel = 0; channel < 3U; ++channel) {
                double value = sample_channel(source, sample_x, sample_y, channel);
                if (in_cavity) {
                    const double darkest_source_value = sample_channel_soft_x(
                        source, seam_sample_x, cavity_sample_y, channel);
                    const double oral_scale = has_contour
                        ? (channel == 2U ? 0.76 : (channel == 1U ? 0.61 : 0.57))
                        : 0.72;
                    const double depth = has_contour
                        ? 0.94 - 0.16 * std::sin(
                            cavity_v * 3.14159265358979323846)
                        : 1.0;
                    double oral_value = darkest_source_value * oral_scale * depth;
                    const double lower_lip_value = sample_channel_soft_x(
                        source, seam_sample_x,
                        static_cast<double>(top) + source_lower_curve +
                            lower_lip_thickness * 0.45 - 0.5,
                        channel);
                    const double tongue_scale = channel == 2U ? 0.78 :
                                                (channel == 1U ? 0.54 : 0.58);
                    oral_value = oral_value * (1.0 - tongue_strength) +
                                 lower_lip_value * tongue_scale * tongue_strength;
                    const double enamel_value = enamel_luma *
                        (channel == 2U ? 1.02 : (channel == 1U ? 0.96 : 0.86));
                    oral_value = oral_value * (1.0 - teeth_strength) +
                                 enamel_value * teeth_strength;
                    value = value * (1.0 - cavity_edge) + oral_value * cavity_edge;
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
                                     const CanonicalMouthPatch& observed_state,
                                     const MouthCoefficients& raw_coefficients,
                                     const Nanoseconds produced_at_ns) {
    ResidualPatch patch{};
    if (!valid_cpu_frame(source) || track != tracking.track || source.identity != tracking.frame ||
        !normalized_rect(tracking.mouth_bounds) || !valid_atlas_patch(observed_state)) {
        return patch;
    }
    const auto coefficients = clamp_coefficients(raw_coefficients);
    if (observed_state.representation !=
            MouthPatchRepresentation::full_lip_observation_v1 &&
        observed_state.representation !=
            MouthPatchRepresentation::normalized_oral_interior_v1) {
        return {};
    }
    const double source_width = static_cast<double>(source.lease.width);
    const double source_height = static_cast<double>(source.lease.height);
    if (observed_state.representation ==
        MouthPatchRepresentation::full_lip_observation_v1) {
        const auto left = static_cast<std::uint32_t>(std::floor(
            tracking.mouth_bounds.x * source_width));
        const auto top = static_cast<std::uint32_t>(std::floor(
            tracking.mouth_bounds.y * source_height));
        const auto right = static_cast<std::uint32_t>(std::ceil(
            tracking.mouth_bounds.right() * source_width));
        const auto bottom = static_cast<std::uint32_t>(std::ceil(
            tracking.mouth_bounds.bottom() * source_height));
        if (right <= left || bottom <= top || right > source.lease.width ||
            bottom > source.lease.height) {
            return {};
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

        const auto& landmarks = tracking.mouth_landmarks;
        const double left_corner_x = landmarks.left_corner.x * source_width;
        const double left_corner_y = landmarks.left_corner.y * source_height;
        const double right_corner_x = landmarks.right_corner.x * source_width;
        const double right_corner_y = landmarks.right_corner.y * source_height;
        const double landmark_dx = right_corner_x - left_corner_x;
        const double landmark_dy = right_corner_y - left_corner_y;
        const double mouth_width = std::hypot(landmark_dx, landmark_dy);
        if (!std::isfinite(mouth_width) || mouth_width < 4.0 ||
            mouth_width > source_width * 0.55) {
            return {};
        }
        const PixelPoint center{
            (left_corner_x + right_corner_x) * 0.5,
            (landmarks.upper_lip_center.y + landmarks.lower_lip_center.y) *
                0.5 * source_height,
        };
        const double roll = std::atan2(landmark_dy, landmark_dx);
        const PixelPoint horizontal_axis{std::cos(roll), std::sin(roll)};
        const PixelPoint vertical_axis{-horizontal_axis.y, horizontal_axis.x};
        const double canonical_width = mouth_width * 1.34;
        const double canonical_height = canonical_width * 0.625;
        for (std::uint32_t y = 0U; y < patch.height; ++y) {
            for (std::uint32_t x = 0U; x < patch.width; ++x) {
                const PixelPoint point{
                    static_cast<double>(left + x) + 0.5,
                    static_cast<double>(top + y) + 0.5,
                };
                const double canonical_x = projection(
                    point, center, horizontal_axis) / (canonical_width * 0.5);
                const double canonical_y = projection(
                    point, center, vertical_axis) / (canonical_height * 0.5);
                if (std::abs(canonical_x) > 1.0 ||
                    std::abs(canonical_y) > 1.0) {
                    continue;
                }
                overlay_patch_sample(
                    patch, x, y,
                    sample_canonical_patch(
                        observed_state, canonical_x, canonical_y),
                    1.0);
            }
        }
        return patch;
    }

    // Current-frame pixels own the lip surface.  A piecewise contour warp keeps
    // pores, lipstick, highlights and the exact game lighting attached to the
    // newest frame.  The atlas is allowed to contribute only oral anatomy that
    // a closed source frame does not contain.
    patch = compose_current_frame_residual(
        source, track, tracking, coefficients, produced_at_ns);
    if (patch.premultiplied_bgra.empty()) {
        return {};
    }

    const auto& landmarks = tracking.mouth_landmarks;
    const double left_corner_x = landmarks.left_corner.x * source_width;
    const double left_corner_y = landmarks.left_corner.y * source_height;
    const double right_corner_x = landmarks.right_corner.x * source_width;
    const double right_corner_y = landmarks.right_corner.y * source_height;
    const double landmark_dx = right_corner_x - left_corner_x;
    const double landmark_dy = right_corner_y - left_corner_y;
    const double mouth_width = std::hypot(landmark_dx, landmark_dy);
    if (!std::isfinite(mouth_width) || mouth_width < 4.0 ||
        mouth_width > source_width * 0.55) {
        return {};
    }
    const auto left = static_cast<std::uint32_t>(std::llround(
        patch.normalized_bounds.x * source_width));
    const auto top = static_cast<std::uint32_t>(std::llround(
        patch.normalized_bounds.y * source_height));
    const bool contour_available = has_full_contour(tracking);
    if (!contour_available) {
        // Sparse geometry has no trustworthy hard aperture boundary.  Retain
        // the current-frame-only result rather than pasting a rectangular oral
        // observation into an estimated ellipse.
        return patch;
    }
    const auto contour_geometry = contour_warp_geometry(
        tracking, coefficients, source_width, source_height);
    const auto destination_inner = inner_contour(contour_geometry.destination);
    double inner_minimum_horizontal = std::numeric_limits<double>::max();
    double inner_maximum_horizontal = std::numeric_limits<double>::lowest();
    double inner_minimum_vertical = std::numeric_limits<double>::max();
    double inner_maximum_vertical = std::numeric_limits<double>::lowest();
    for (const auto& point : destination_inner) {
        const double horizontal = projection(
            point, contour_geometry.inner_center, contour_geometry.horizontal_axis);
        const double vertical = projection(
            point, contour_geometry.inner_center, contour_geometry.vertical_axis);
        inner_minimum_horizontal = std::min(inner_minimum_horizontal, horizontal);
        inner_maximum_horizontal = std::max(inner_maximum_horizontal, horizontal);
        inner_minimum_vertical = std::min(inner_minimum_vertical, vertical);
        inner_maximum_vertical = std::max(inner_maximum_vertical, vertical);
    }
    const double inner_width = inner_maximum_horizontal - inner_minimum_horizontal;
    const double inner_height = inner_maximum_vertical - inner_minimum_vertical;
    const double opening_strength = unit(
        coefficients.jaw_open * (1.0 - coefficients.lip_close * 0.92) +
        coefficients.lower_lip_depress * 0.16);
    const double atlas_activity = smoother_unit((opening_strength - 0.018) / 0.16);
    if (atlas_activity <= 1.0e-6 || inner_width < 2.0 || inner_height < 1.0) {
        return patch;
    }

    constexpr double oral_left = -0.67;
    constexpr double oral_top = -0.24;
    constexpr double oral_right = 0.67;
    constexpr double oral_bottom = 0.20;

    for (std::uint32_t y = 0; y < patch.height; ++y) {
        for (std::uint32_t x = 0; x < patch.width; ++x) {
            const double frame_x = static_cast<double>(left + x) + 0.5;
            const double frame_y = static_cast<double>(top + y) + 0.5;
            const PixelPoint frame_point{frame_x, frame_y};
            if (!point_inside_polygon(frame_point, destination_inner)) {
                continue;
            }
            const double edge_distance = polygon_edge_distance(
                frame_point, destination_inner);
            const double oral_support = smoother_unit(edge_distance / 1.15) *
                                        atlas_activity;
            if (oral_support <= 1.0e-6) continue;
            const double horizontal = projection(
                frame_point, contour_geometry.inner_center,
                contour_geometry.horizontal_axis);
            const double vertical = projection(
                frame_point, contour_geometry.inner_center,
                contour_geometry.vertical_axis);
            const double oral_u = unit(
                (horizontal - inner_minimum_horizontal) / inner_width);
            const double oral_v = unit(
                (vertical - inner_minimum_vertical) / inner_height);
            const double canonical_x = oral_left + oral_u * (oral_right - oral_left);
            const double canonical_y = oral_top + oral_v * (oral_bottom - oral_top);
            const double sample_x = (canonical_x + 1.0) * 0.5 *
                                        static_cast<double>(observed_state.width) - 0.5;
            const double sample_y = (canonical_y + 1.0) * 0.5 *
                                        static_cast<double>(observed_state.height) - 0.5;
            const double clamped_x = std::clamp(
                sample_x, 0.0, static_cast<double>(observed_state.width - 1U));
            const double clamped_y = std::clamp(
                sample_y, 0.0, static_cast<double>(observed_state.height - 1U));
            const auto x0 = static_cast<std::uint32_t>(std::floor(clamped_x));
            const auto y0 = static_cast<std::uint32_t>(std::floor(clamped_y));
            const auto x1 = std::min(x0 + 1U, observed_state.width - 1U);
            const auto y1 = std::min(y0 + 1U, observed_state.height - 1U);
            const double fraction_x = clamped_x - static_cast<double>(x0);
            const double fraction_y = clamped_y - static_cast<double>(y0);
            const double weight_00 = (1.0 - fraction_x) * (1.0 - fraction_y);
            const double weight_10 = fraction_x * (1.0 - fraction_y);
            const double weight_01 = (1.0 - fraction_x) * fraction_y;
            const double weight_11 = fraction_x * fraction_y;
            const auto offset_00 = static_cast<std::size_t>(y0) * observed_state.stride_bytes +
                                   static_cast<std::size_t>(x0) * 4U;
            const auto offset_10 = static_cast<std::size_t>(y0) * observed_state.stride_bytes +
                                   static_cast<std::size_t>(x1) * 4U;
            const auto offset_01 = static_cast<std::size_t>(y1) * observed_state.stride_bytes +
                                   static_cast<std::size_t>(x0) * 4U;
            const auto offset_11 = static_cast<std::size_t>(y1) * observed_state.stride_bytes +
                                   static_cast<std::size_t>(x1) * 4U;
            const auto output = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const auto sample = [&](const std::size_t channel) {
                return static_cast<double>(observed_state.premultiplied_bgra[offset_00 + channel]) *
                           weight_00 +
                       static_cast<double>(observed_state.premultiplied_bgra[offset_10 + channel]) *
                           weight_10 +
                       static_cast<double>(observed_state.premultiplied_bgra[offset_01 + channel]) *
                           weight_01 +
                       static_cast<double>(observed_state.premultiplied_bgra[offset_11 + channel]) *
                           weight_11;
            };
            const double observed_alpha = sample(3U) / 255.0 * oral_support;
            if (observed_alpha <= 1.0e-6) {
                continue;
            }
            const double inverse_alpha = 1.0 - observed_alpha;
            for (std::size_t channel = 0; channel < 3U; ++channel) {
                const double observed_premultiplied = sample(channel) * oral_support;
                const double value = observed_premultiplied +
                                     patch.premultiplied_bgra[output + channel] * inverse_alpha;
                patch.premultiplied_bgra[output + channel] = static_cast<std::uint8_t>(
                    std::clamp(std::lround(value), 0L, 255L));
            }
            const double output_alpha = observed_alpha * 255.0 +
                patch.premultiplied_bgra[output + 3U] * inverse_alpha;
            patch.premultiplied_bgra[output + 3U] = static_cast<std::uint8_t>(
                std::clamp(std::lround(output_alpha), 0L, 255L));
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
