#include "npc/mouth_worker/current_pixel_compositor.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <numeric>
#include <utility>
#include <vector>

namespace npc::mouth {
namespace {

constexpr std::size_t curve_points = 11U;
using Curve = std::array<double, curve_points>;

struct Point {
  double x{};
  double y{};
};
struct Curves {
  Curve x{};
  Curve outer_upper{};
  Curve outer_lower{};
  Curve inner_upper{};
  Curve inner_lower{};
  Point center{};
  Point horizontal{};
  Point vertical{};
  double width{};
};

[[nodiscard]] double unit(const double value) noexcept {
  return std::clamp(value, 0.0, 1.0);
}

[[nodiscard]] double smooth_unit(const double value) noexcept {
  const double x = unit(value);
  return x * x * (3.0 - 2.0 * x);
}

[[nodiscard]] bool
finite_coefficients(const MouthCoefficients &value) noexcept {
  return std::isfinite(value.jaw_open) && std::isfinite(value.lip_close) &&
         std::isfinite(value.funnel) && std::isfinite(value.pucker) &&
         std::isfinite(value.smile_left) && std::isfinite(value.smile_right) &&
         std::isfinite(value.upper_lip_raise) &&
         std::isfinite(value.lower_lip_depress);
}

[[nodiscard]] MouthCoefficients clamped(MouthCoefficients value) noexcept {
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

[[nodiscard]] bool normalized_rect(const NormalizedRect &value) noexcept {
  return std::isfinite(value.x) && std::isfinite(value.y) &&
         std::isfinite(value.width) && std::isfinite(value.height) &&
         value.x >= 0.0 && value.y >= 0.0 && value.width > 0.0 &&
         value.height > 0.0 && value.right() <= 1.0 && value.bottom() <= 1.0;
}

[[nodiscard]] bool silence(const MouthCoefficients &value) noexcept {
  const double other = std::max(
      {value.jaw_open, value.funnel, value.pucker, value.smile_left,
       value.smile_right, value.upper_lip_raise, value.lower_lip_depress});
  return other <= 1.0e-6 &&
         (value.lip_close <= 1.0e-6 || value.lip_close >= 1.0 - 1.0e-6);
}

[[nodiscard]] CurrentPixelMouthShape
derived_shape(const MouthCoefficients &value) noexcept {
  CurrentPixelMouthShape result{};
  result.aperture = unit(value.jaw_open) * 0.20;
  const double smile = (unit(value.smile_left) + unit(value.smile_right)) * 0.5;
  result.width_scale =
      std::clamp(1.0 - unit(value.funnel) * 0.10 + smile * 0.06, 0.70, 1.12);
  result.contact =
      value.lip_close >= 0.88 && value.pucker >= 0.04 && value.jaw_open <= 0.05;
  result.articulation_strength = silence(value) ? 0.0 : 1.0;
  return result;
}

[[nodiscard]] double projection(const Point point, const Point origin,
                                const Point axis) noexcept {
  return (point.x - origin.x) * axis.x + (point.y - origin.y) * axis.y;
}

[[nodiscard]] Point to_local(const Point point, const Curves &curves) noexcept {
  return {projection(point, curves.center, curves.horizontal),
          projection(point, curves.center, curves.vertical)};
}

[[nodiscard]] Point to_frame(const double x, const double y,
                             const Curves &curves) noexcept {
  return {curves.center.x + curves.horizontal.x * x + curves.vertical.x * y,
          curves.center.y + curves.horizontal.y * x + curves.vertical.y * y};
}

template <std::size_t Size>
[[nodiscard]] bool resample(const std::array<Point, Size> &frame,
                            const Curves &basis, const Curve &xs,
                            Curve &output) noexcept {
  std::array<Point, Size> local{};
  for (std::size_t i = 0; i < Size; ++i)
    local[i] = to_local(frame[i], basis);
  std::sort(local.begin(), local.end(),
            [](const Point a, const Point b) { return a.x < b.x; });
  for (std::size_t i = 1; i < Size; ++i) {
    if (!std::isfinite(local[i].x) || !std::isfinite(local[i].y) ||
        local[i].x - local[i - 1U].x < 1.0e-4)
      return false;
  }
  for (std::size_t column = 0; column < xs.size(); ++column) {
    const double x = xs[column];
    if (x <= local.front().x) {
      output[column] = local.front().y;
      continue;
    }
    if (x >= local.back().x) {
      output[column] = local.back().y;
      continue;
    }
    const auto upper = std::upper_bound(
        local.begin(), local.end(), x,
        [](const double value, const Point point) { return value < point.x; });
    const std::size_t right = static_cast<std::size_t>(upper - local.begin());
    const auto &a = local[right - 1U];
    const auto &b = local[right];
    const double amount = (x - a.x) / (b.x - a.x);
    output[column] = a.y + (b.y - a.y) * amount;
  }
  return true;
}

[[nodiscard]] bool openseeface_curves(const TrackingEvidence &tracking,
                                      const double frame_width,
                                      const double frame_height,
                                      Curves &result) noexcept {
  const auto &mouth = tracking.mouth_landmarks;
  if (mouth.schema_version < 2U ||
      mouth.contour_points != mouth_contour_point_count)
    return false;
  const auto point = [&](const std::size_t index) {
    const auto &value = mouth.contour[index];
    return Point{value.x * frame_width, value.y * frame_height};
  };
  const Point raw_58 = point(10U);
  const Point raw_62 = point(14U);
  const Point semantic_left{mouth.left_corner.x * frame_width,
                            mouth.left_corner.y * frame_height};
  if (!std::isfinite(semantic_left.x) || !std::isfinite(semantic_left.y))
    return false;
  const auto squared_distance = [](const Point first, const Point second) {
    const double dx = first.x - second.x;
    const double dy = first.y - second.y;
    return dx * dx + dy * dy;
  };
  const bool raw_58_is_left = squared_distance(raw_58, semantic_left) <=
                              squared_distance(raw_62, semantic_left);
  const Point left = raw_58_is_left ? raw_58 : raw_62;
  const Point right = raw_58_is_left ? raw_62 : raw_58;
  const double dx = right.x - left.x;
  const double dy = right.y - left.y;
  result.width = std::hypot(dx, dy);
  if (!std::isfinite(result.width) || result.width <= 1.0e-6)
    return false;
  result.center = {(left.x + right.x) * 0.5, (left.y + right.y) * 0.5};
  result.horizontal = {dx / result.width, dy / result.width};
  result.vertical = {-result.horizontal.y, result.horizontal.x};
  for (std::size_t i = 0; i < curve_points; ++i) {
    result.x[i] =
        -result.width * 0.5 + result.width * static_cast<double>(i) /
                                  static_cast<double>(curve_points - 1U);
  }
  const std::array<Point, 7U> outer_upper =
      raw_58_is_left
          ? std::array<Point, 7U>{raw_58,    point(0U), point(1U), point(2U),
                                  point(3U), point(4U), raw_62}
          : std::array<Point, 7U>{raw_62,    point(4U), point(3U), point(2U),
                                  point(1U), point(0U), raw_58};
  const std::array<Point, 7U> outer_lower =
      raw_58_is_left
          ? std::array<Point, 7U>{raw_58,    point(9U), point(8U), point(7U),
                                  point(6U), point(5U), raw_62}
          : std::array<Point, 7U>{raw_62,    point(5U), point(6U), point(7U),
                                  point(8U), point(9U), raw_58};
  const std::array<Point, 5U> inner_upper =
      raw_58_is_left ? std::array<Point, 5U>{raw_58, point(11U), point(12U),
                                             point(13U), raw_62}
                     : std::array<Point, 5U>{raw_62, point(13U), point(12U),
                                             point(11U), raw_58};
  const std::array<Point, 5U> inner_lower =
      raw_58_is_left ? std::array<Point, 5U>{raw_58, point(17U), point(16U),
                                             point(15U), raw_62}
                     : std::array<Point, 5U>{raw_62, point(15U), point(16U),
                                             point(17U), raw_58};
  return resample(outer_upper, result, result.x, result.outer_upper) &&
         resample(outer_lower, result, result.x, result.outer_lower) &&
         resample(inner_upper, result, result.x, result.inner_upper) &&
         resample(inner_lower, result, result.x, result.inner_lower);
}

[[nodiscard]] int reflect101(int value, const int limit) noexcept {
  if (limit <= 1)
    return 0;
  while (value < 0 || value >= limit) {
    value = value < 0 ? -value : 2 * limit - value - 2;
  }
  return value;
}

[[nodiscard]] double raw_channel(const CpuFrame &frame, int x, int y,
                                 const std::size_t channel) noexcept {
  x = reflect101(x, static_cast<int>(frame.lease.width));
  y = reflect101(y, static_cast<int>(frame.lease.height));
  return frame.bgra[static_cast<std::size_t>(y) * frame.lease.stride_bytes +
                    static_cast<std::size_t>(x) * 4U + channel];
}

[[nodiscard]] double cubic_weight(const double x) noexcept {
  constexpr double a = -0.75;
  const double v = std::abs(x);
  if (v <= 1.0)
    return (a + 2.0) * v * v * v - (a + 3.0) * v * v + 1.0;
  if (v < 2.0)
    return a * v * v * v - 5.0 * a * v * v + 8.0 * a * v - 4.0 * a;
  return 0.0;
}

[[nodiscard]] double sample_cubic(const CpuFrame &frame, const double x,
                                  const double y,
                                  const std::size_t channel) noexcept {
  const int floor_x = static_cast<int>(std::floor(x));
  const int floor_y = static_cast<int>(std::floor(y));
  double value = 0.0;
  double weight = 0.0;
  for (int oy = -1; oy <= 2; ++oy) {
    const double wy = cubic_weight(y - static_cast<double>(floor_y + oy));
    for (int ox = -1; ox <= 2; ++ox) {
      const double w = wy * cubic_weight(x - static_cast<double>(floor_x + ox));
      value += raw_channel(frame, floor_x + ox, floor_y + oy, channel) * w;
      weight += w;
    }
  }
  return std::clamp(value / std::max(1.0e-9, weight), 0.0, 255.0);
}

[[nodiscard]] double luma(const CpuFrame &frame, const double x,
                          const double y) noexcept {
  return raw_channel(frame, static_cast<int>(std::lround(x)),
                     static_cast<int>(std::lround(y)), 0U) *
             0.114 +
         raw_channel(frame, static_cast<int>(std::lround(x)),
                     static_cast<int>(std::lround(y)), 1U) *
             0.587 +
         raw_channel(frame, static_cast<int>(std::lround(x)),
                     static_cast<int>(std::lround(y)), 2U) *
             0.299;
}

[[nodiscard]] double blurred_luma(const CpuFrame &frame,
                                  const Point point) noexcept {
  constexpr std::array<double, 3U> kernel{0.25, 0.5, 0.25};
  double value = 0.0;
  for (int oy = -1; oy <= 1; ++oy)
    for (int ox = -1; ox <= 1; ++ox) {
      value += luma(frame, point.x + ox, point.y + oy) *
               kernel[static_cast<std::size_t>(ox + 1)] *
               kernel[static_cast<std::size_t>(oy + 1)];
    }
  return value;
}

[[nodiscard]] double directional_edge(const CpuFrame &frame,
                                      const Curves &curves, const double x,
                                      const double y) noexcept {
  const Point center = to_frame(x, y, curves);
  const Point before{center.x - curves.vertical.x,
                     center.y - curves.vertical.y};
  const Point after{center.x + curves.vertical.x, center.y + curves.vertical.y};
  return (blurred_luma(frame, after) - blurred_luma(frame, before)) * 0.5;
}

[[nodiscard]] double percentile(std::vector<double> values,
                                const double fraction) {
  if (values.empty())
    return 0.0;
  const std::size_t index = static_cast<std::size_t>(
      std::floor(unit(fraction) * static_cast<double>(values.size() - 1U)));
  std::nth_element(values.begin(),
                   values.begin() + static_cast<std::ptrdiff_t>(index),
                   values.end());
  return values[index];
}

struct Trace {
  Curve y{};
  double contrast{};
};

[[nodiscard]] Trace trace_edge(const CpuFrame &frame, const Curves &curves,
                               const Curve &reference, const Curve &low,
                               const Curve &high, const double polarity) {
  const double minimum = *std::min_element(low.begin(), low.end()) - 0.5;
  const double maximum = *std::max_element(high.begin(), high.end()) + 0.5;
  const std::size_t levels_count = static_cast<std::size_t>(
      std::max(2.0, std::floor((maximum - minimum) / 0.25) + 1.0));
  std::vector<double> levels(levels_count);
  for (std::size_t i = 0; i < levels_count; ++i)
    levels[i] = minimum + i * 0.25;
  std::vector<double> edge(curve_points * levels_count);
  std::vector<double> positives;
  for (std::size_t c = 0; c < curve_points; ++c)
    for (std::size_t r = 0; r < levels_count; ++r) {
      const double response =
          directional_edge(frame, curves, curves.x[c], levels[r]) * polarity;
      edge[c * levels_count + r] = response;
      positives.push_back(std::max(0.0, response));
    }
  const double scale = std::max(1.0, percentile(positives, 0.90));
  std::vector<double> cost(levels_count, 1.0e12);
  std::vector<std::vector<std::size_t>> back(
      curve_points - 1U, std::vector<std::size_t>(levels_count));
  const auto score = [&](const std::size_t c, const std::size_t r) {
    if (levels[r] < low[c] || levels[r] > high[c])
      return 1.0e9;
    double value = -edge[c * levels_count + r] / scale +
                   0.002 * std::pow(levels[r] - reference[c], 2.0);
    if (c == 0U || c + 1U == curve_points)
      value += 20.0 * std::pow(levels[r] - reference[c], 2.0);
    return value;
  };
  for (std::size_t r = 0; r < levels_count; ++r)
    cost[r] = score(0U, r);
  for (std::size_t c = 1U; c < curve_points; ++c) {
    std::vector<double> next(levels_count, 1.0e12);
    const double slope = reference[c] - reference[c - 1U];
    for (std::size_t r = 0; r < levels_count; ++r) {
      for (std::size_t parent = 0; parent < levels_count; ++parent) {
        const double candidate =
            cost[parent] +
            0.003 * std::pow(levels[r] - levels[parent] - slope, 2.0);
        if (candidate < next[r]) {
          next[r] = candidate;
          back[c - 1U][r] = parent;
        }
      }
      next[r] += score(c, r);
    }
    cost = std::move(next);
  }
  Trace result{};
  std::size_t selected = static_cast<std::size_t>(
      std::min_element(cost.begin(), cost.end()) - cost.begin());
  for (std::size_t reverse = curve_points; reverse-- > 0U;) {
    result.y[reverse] = levels[selected];
    if (reverse > 0U)
      selected = back[reverse - 1U][selected];
  }
  std::vector<double> middle;
  for (std::size_t c = 3U; c < 8U; ++c) {
    const auto nearest = static_cast<std::size_t>(
        std::clamp(std::lround((result.y[c] - minimum) / 0.25), 0L,
                   static_cast<long>(levels_count - 1U)));
    middle.push_back(edge[c * levels_count + nearest]);
  }
  result.contrast = percentile(middle, 0.5);
  return result;
}

[[nodiscard]] bool refine_edges(const CpuFrame &source, Curves &curves,
                                const CurrentPixelCompositorPolicy &policy,
                                CurrentPixelCompositorEvidence &evidence) {
  Curve upper_low{}, upper_high{}, lower_low{}, lower_high{};
  for (std::size_t i = 0; i < curve_points; ++i) {
    upper_low[i] = curves.outer_upper[i] - curves.width * 0.10;
    upper_high[i] = std::min(curves.inner_upper[i] + curves.width * 0.08,
                             curves.outer_upper[i] + curves.width * 0.20);
    lower_low[i] = std::max(curves.inner_lower[i] + curves.width * 0.025,
                            curves.outer_lower[i] - curves.width * 0.20);
    lower_high[i] = curves.outer_lower[i] + curves.width * 0.10;
    if (upper_low[i] > upper_high[i] || lower_low[i] > lower_high[i])
      return false;
  }
  const Trace upper = trace_edge(source, curves, curves.outer_upper, upper_low,
                                 upper_high, -1.0);
  const Trace lower = trace_edge(source, curves, curves.outer_lower, lower_low,
                                 lower_high, 1.0);
  if (std::min(upper.contrast, lower.contrast) < policy.minimum_edge_contrast)
    return false;
  for (std::size_t i = 2U; i < 9U; ++i)
    if (lower.y[i] - upper.y[i] < 2.0)
      return false;
  evidence.source_edge_upper_contrast = upper.contrast;
  evidence.source_edge_lower_contrast = lower.contrast;
  curves.outer_upper = upper.y;
  curves.outer_lower = lower.y;
  double margin = std::numeric_limits<double>::max();
  for (std::size_t i = 4U; i < 7U; ++i)
    margin = std::min(margin, std::min(curves.inner_upper[i] - upper.y[i],
                                       lower.y[i] - curves.inner_lower[i]));
  evidence.source_edge_inner_margin_pixels = margin;
  const bool contact = margin < 0.75;
  evidence.source_edge_contact = contact;
  if (contact) {
    Curve seam{};
    for (std::size_t c = 0; c < curve_points; ++c) {
      const double ridge =
          upper.y.front() +
          (upper.y.back() - upper.y.front()) * static_cast<double>(c) /
              static_cast<double>(curve_points - 1U) +
          curves.width * 0.02 *
              std::max(0.0,
                       1.0 - std::pow(curves.x[c] / (curves.width * 0.5), 2.0));
      double best = 1.0e12;
      double chosen = ridge;
      for (std::size_t level = 0; level < 25U; ++level) {
        const double offset =
            -curves.width * 0.06 +
            curves.width * 0.12 * static_cast<double>(level) / 24.0;
        const double candidate = ridge + offset;
        if (candidate < upper.y[c] + 0.8 || candidate > lower.y[c] - 0.8)
          continue;
        const Point sample = to_frame(curves.x[c], candidate, curves);
        const double score =
            blurred_luma(source, sample) * 0.07 + 0.4 * offset * offset;
        if (score < best) {
          best = score;
          chosen = candidate;
        }
      }
      seam[c] = chosen;
    }
    seam.front() = upper.y.front();
    seam.back() = upper.y.back();
    curves.inner_upper = seam;
    curves.inner_lower = seam;
  }
  evidence.source_edge_refined = true;
  return true;
}

[[nodiscard]] double curve_value(const Curve &xs, const Curve &ys,
                                 const double x) noexcept {
  if (x <= xs.front())
    return ys.front();
  if (x >= xs.back())
    return ys.back();
  const double position = (x - xs.front()) / (xs.back() - xs.front()) *
                          static_cast<double>(curve_points - 1U);
  const std::size_t left = std::min(
      static_cast<std::size_t>(std::floor(position)), curve_points - 2U);
  const double amount = position - static_cast<double>(left);
  return ys[left] + (ys[left + 1U] - ys[left]) * amount;
}

[[nodiscard]] bool valid_oral_patch(const CanonicalMouthPatch &patch,
                                    bool &transparent) noexcept {
  transparent = true;
  if (patch.representation !=
          MouthPatchRepresentation::normalized_oral_strip_v1 ||
      patch.width == 0U || patch.height == 0U ||
      patch.stride_bytes != patch.width * 4U ||
      patch.premultiplied_bgra.size() !=
          static_cast<std::size_t>(patch.stride_bytes) * patch.height)
    return false;
  for (std::size_t i = 0; i < patch.premultiplied_bgra.size(); i += 4U) {
    const auto alpha = patch.premultiplied_bgra[i + 3U];
    transparent = transparent && alpha == 0U;
    if (patch.premultiplied_bgra[i] > alpha ||
        patch.premultiplied_bgra[i + 1U] > alpha ||
        patch.premultiplied_bgra[i + 2U] > alpha)
      return false;
  }
  return transparent || (std::isfinite(patch.reference_context_mean) &&
                         patch.reference_context_mean > 0.0 &&
                         patch.reference_context_mean <= 255.0);
}

[[nodiscard]] std::array<double, 4U>
sample_oral(const CanonicalMouthPatch &patch, const double u,
            const double v) noexcept {
  const double x = unit(u) * static_cast<double>(patch.width - 1U);
  const double y = unit(v) * static_cast<double>(patch.height - 1U);
  const auto x0 = static_cast<std::uint32_t>(std::floor(x));
  const auto y0 = static_cast<std::uint32_t>(std::floor(y));
  const auto x1 = std::min(x0 + 1U, patch.width - 1U);
  const auto y1 = std::min(y0 + 1U, patch.height - 1U);
  const double fx = x - x0, fy = y - y0;
  const std::array<double, 4U> weights{(1 - fx) * (1 - fy), fx * (1 - fy),
                                       (1 - fx) * fy, fx * fy};
  const std::array<std::size_t, 4U> offsets{
      static_cast<std::size_t>(y0) * patch.stride_bytes + x0 * 4U,
      static_cast<std::size_t>(y0) * patch.stride_bytes + x1 * 4U,
      static_cast<std::size_t>(y1) * patch.stride_bytes + x0 * 4U,
      static_cast<std::size_t>(y1) * patch.stride_bytes + x1 * 4U};
  std::array<double, 4U> result{};
  for (std::size_t s = 0; s < 4U; ++s)
    for (std::size_t c = 0; c < 4U; ++c)
      result[c] += patch.premultiplied_bgra[offsets[s] + c] * weights[s];
  return result;
}

void initialize(ResidualPatch &patch, const CpuFrame &source,
                const TrackBinding &track, const std::uint32_t left,
                const std::uint32_t top, const std::uint32_t width,
                const std::uint32_t height, const Nanoseconds produced) {
  patch.track = track;
  patch.source_frame = source.identity;
  patch.normalized_bounds = {static_cast<double>(left) / source.lease.width,
                             static_cast<double>(top) / source.lease.height,
                             static_cast<double>(width) / source.lease.width,
                             static_cast<double>(height) / source.lease.height};
  patch.source_lease = source.lease;
  patch.source_lease.native_handle_value = 0U;
  patch.width = width;
  patch.height = height;
  patch.stride_bytes = width * 4U;
  patch.produced_at_ns = produced;
  patch.residual_lease.schema_version = 1U;
  patch.residual_lease.transport = LeaseTransport::cpu_reference;
  patch.residual_lease.lease_nonce_high =
      source.lease.lease_nonce_high ^ 0x4d4f555448504154ULL;
  patch.residual_lease.lease_nonce_low =
      source.lease.lease_nonce_low ^ 0x4348000000000001ULL;
  patch.residual_lease.owner_process_id =
      source.lease.intended_consumer_process_id;
  patch.residual_lease.intended_consumer_process_id =
      source.lease.owner_process_id;
  patch.residual_lease.adapter_luid_low = source.lease.adapter_luid_low;
  patch.residual_lease.adapter_luid_high = source.lease.adapter_luid_high;
  patch.residual_lease.width = width;
  patch.residual_lease.height = height;
  patch.residual_lease.stride_bytes = patch.stride_bytes;
  patch.residual_lease.format = PixelFormat::bgra8_unorm_premultiplied;
  patch.residual_lease.expires_at_ns =
      std::min(source.lease.expires_at_ns, produced + 80'000'000);
  patch.premultiplied_bgra.assign(
      static_cast<std::size_t>(patch.stride_bytes) * height, 0U);
}

} // namespace

MouthCoefficients
current_pixel_coefficients_for_viseme(const Viseme viseme,
                                      const double strength) noexcept {
  const double amount = unit(strength);
  MouthCoefficients result{};
  if (viseme == Viseme::silence) {
    result.lip_close = 1.0;
    return result;
  }
  double aperture = .045, width = 1.025;
  switch (viseme) {
  case Viseme::bilabial:
    aperture = 0;
    width = 1;
    result.lip_close = 1 - .05 * amount;
    result.pucker = .22 * amount;
    break;
  case Viseme::labiodental:
  case Viseme::dental:
    aperture = .035;
    break;
  case Viseme::alveolar:
  case Viseme::postalveolar:
  case Viseme::palatal:
    aperture = .045;
    break;
  case Viseme::velar:
  case Viseme::open_vowel:
    aperture = .20;
    width = 1;
    break;
  case Viseme::rounded:
    aperture = .15;
    width = .90;
    break;
  case Viseme::spread_vowel:
    aperture = .115;
    width = 1.06;
    break;
  case Viseme::silence:
    break;
  }
  result.jaw_open = unit(aperture / .20) * amount;
  if (width < 1.0)
    result.funnel = unit((1.0 - width) / .10) * amount;
  if (width > 1.0)
    result.smile_left = result.smile_right = unit((width - 1.0) / .06) * amount;
  return result;
}

ResidualPatch compose_current_pixel_residual(
    const CpuFrame &source, const TrackBinding &track,
    const TrackingEvidence &tracking, const CanonicalMouthPatch *oral_patch,
    const MouthCoefficients &raw_coefficients, const Nanoseconds produced_at_ns,
    CurrentPixelCompositorPolicy policy,
    CurrentPixelCompositorEvidence *evidence_out) {
  ResidualPatch empty{};
  CurrentPixelCompositorEvidence evidence{};
  if (!valid_cpu_frame(source) || track != tracking.track ||
      source.identity != tracking.frame ||
      !normalized_rect(tracking.mouth_bounds) ||
      !finite_coefficients(raw_coefficients) ||
      !std::isfinite(policy.minimum_mouth_width_pixels) ||
      policy.minimum_mouth_width_pixels < 4.0 ||
      !std::isfinite(policy.minimum_edge_contrast) ||
      policy.minimum_edge_contrast < 0.0)
    return empty;
  const MouthCoefficients coefficients = clamped(raw_coefficients);
  CurrentPixelMouthShape shape =
      policy.shape_override.value_or(derived_shape(coefficients));
  if (!std::isfinite(shape.aperture) || !std::isfinite(shape.width_scale) ||
      !std::isfinite(shape.articulation_strength) || shape.aperture < 0.0 ||
      shape.width_scale < 0.7 || shape.width_scale > 1.3 ||
      shape.articulation_strength < 0.0 || shape.articulation_strength > 1.0)
    return empty;
  Curves curves{};
  if (!openseeface_curves(tracking, source.lease.width, source.lease.height,
                          curves) ||
      curves.width < policy.minimum_mouth_width_pixels)
    return empty;
  evidence.mouth_width_pixels = curves.width;
  if (policy.refine_source_edges &&
      !refine_edges(source, curves, policy, evidence))
    return empty;
  bool oral_transparent = true;
  if (oral_patch && !valid_oral_patch(*oral_patch, oral_transparent))
    return empty;
  const CanonicalMouthPatch *oral =
      oral_patch && !oral_transparent ? oral_patch : nullptr;
  shape.contact =
      shape.contact || (shape.aperture < .02 && !silence(coefficients) &&
                        shape.articulation_strength > .01);
  if (shape.contact)
    shape.articulation_strength = 1.0;

  // Use the same rotated support rectangle as the accepted Python proof.
  // The smooth field is identically zero at this boundary, which prevents a
  // rectangular residual seam even when the mouth has substantial roll.
  constexpr double horizontal_radius_in_widths = 1.04;
  constexpr double vertical_radius_in_widths = 0.74;
  double minimum_x = std::numeric_limits<double>::max();
  double minimum_y = std::numeric_limits<double>::max();
  double maximum_x = std::numeric_limits<double>::lowest();
  double maximum_y = std::numeric_limits<double>::lowest();
  for (const double horizontal_sign : {-1.0, 1.0}) {
    for (const double vertical_sign : {-1.0, 1.0}) {
      const Point corner = to_frame(
          horizontal_sign * curves.width * horizontal_radius_in_widths,
          vertical_sign * curves.width * vertical_radius_in_widths, curves);
      minimum_x = std::min(minimum_x, corner.x);
      minimum_y = std::min(minimum_y, corner.y);
      maximum_x = std::max(maximum_x, corner.x);
      maximum_y = std::max(maximum_y, corner.y);
    }
  }
  const double face_left = tracking.face_bounds.x * source.lease.width;
  const double face_top = tracking.face_bounds.y * source.lease.height;
  const double face_right = tracking.face_bounds.right() * source.lease.width;
  const double face_bottom =
      tracking.face_bounds.bottom() * source.lease.height;
  if (!normalized_rect(tracking.face_bounds) || minimum_x < face_left - 2.0 ||
      minimum_y < face_top - 2.0 || maximum_x > face_right + 2.0 ||
      maximum_y > face_bottom + 2.0 || minimum_x < 0.0 || minimum_y < 0.0 ||
      maximum_x >= source.lease.width || maximum_y >= source.lease.height) {
    return empty;
  }
  const auto left =
      static_cast<std::uint32_t>(std::max(0.0, std::floor(minimum_x - 2.0)));
  const auto top =
      static_cast<std::uint32_t>(std::max(0.0, std::floor(minimum_y - 2.0)));
  const auto right = static_cast<std::uint32_t>(std::min(
      static_cast<double>(source.lease.width), std::ceil(maximum_x + 2.0)));
  const auto bottom = static_cast<std::uint32_t>(std::min(
      static_cast<double>(source.lease.height), std::ceil(maximum_y + 2.0)));
  if (right <= left || bottom <= top || right > source.lease.width ||
      bottom > source.lease.height) {
    return empty;
  }
  ResidualPatch patch{};
  initialize(patch, source, track, left, top, right - left, bottom - top,
             produced_at_ns);
  patch.coefficients = coefficients;
  if (shape.articulation_strength <= .01) {
    if (evidence_out)
      *evidence_out = evidence;
    return patch;
  }

  const std::size_t count =
      static_cast<std::size_t>(patch.width) * patch.height;
  std::vector<double> map_x(count), map_y(count), target_upper(count),
      target_lower(count);
  std::vector<std::uint8_t> moved(count);
  const double centre_gap =
      std::max(.12, curves.inner_lower[5] - curves.inner_upper[5]);
  evidence.source_gap_pixels = centre_gap;
  const double desired =
      shape.contact ? 0.0 : std::max(.55, curves.width * shape.aperture);
  const double cap =
      oral ? curves.width * .22 : std::max(1.8, centre_gap * 2.25);
  evidence.target_gap_pixels =
      shape.contact ? 0.0
                    : centre_gap + shape.articulation_strength *
                                       (std::min(desired, cap) - centre_gap);
  evidence.contact_occludes_cavity = shape.contact;
  auto mapped_at = [&](const std::uint32_t px, const std::uint32_t py,
                       const std::size_t index) {
    // OpenCV remap, used by v10, treats integer coordinates as pixel
    // centres. Matching that convention avoids a systematic half-pixel
    // source shift and keeps identity samples byte-exact.
    const Point frame{static_cast<double>(left + px),
                      static_cast<double>(top + py)};
    const Point local = to_local(frame, curves);
    const double tx = local.x;
    const double ty = local.y;
    const double hf = smooth_unit((.85 - std::abs(tx) / curves.width) / .26);
    const double sx =
        tx / (1 + shape.articulation_strength * (shape.width_scale - 1) * hf);
    double uo = curve_value(curves.x, curves.outer_upper, sx);
    double lo = curve_value(curves.x, curves.outer_lower, sx);
    double ui = curve_value(curves.x, curves.inner_upper, sx);
    double li = curve_value(curves.x, curves.inner_lower, sx);
    if (li - ui < -.75)
      return false;
    const double sg = std::max(.12, li - ui), seam = ui * .68 + li * .32;
    ui = seam - .32 * sg;
    li = seam + .68 * sg;
    uo = std::min(uo, ui - .25);
    lo = std::max(lo, li + .25);
    const double bell =
        std::pow(std::max(0.0, 1 - std::pow(sx / (curves.width * .46), 2)), .8);
    const double tg = std::max(shape.contact ? 0.0 : .12,
                               sg + shape.articulation_strength *
                                        (std::min(desired, cap) * bell - sg));
    const double tui = seam - .32 * tg, tli = seam + .68 * tg;
    const double tuo = tui - (ui - uo), tlo = tli + (lo - li);
    const std::array<double, 6U> target{-curves.width * .72, tuo, tui, tli, tlo,
                                        curves.width * .72};
    std::array<double, 6U> original{-curves.width * .72, uo, ui, li, lo,
                                    curves.width * .72};
    if (shape.contact) {
      original[2] = std::max(uo + .1, ui - .75);
      original[3] = std::min(lo - .1, li + .75);
    }
    double sy = ty;
    for (std::size_t strip = 0; strip < 5U; ++strip) {
      if (shape.contact && strip == 2U)
        continue;
      if (target[strip + 1] - target[strip] < .01 ||
          original[strip + 1] - original[strip] < .01)
        return false;
      if (ty >= target[strip] && ty < target[strip + 1]) {
        const double a =
            unit((ty - target[strip]) / (target[strip + 1] - target[strip]));
        sy = original[strip] + a * (original[strip + 1] - original[strip]);
      }
    }
    const double fade = smooth_unit((.80 - std::abs(tx) / curves.width) / .22) *
                        smooth_unit((.71 - std::abs(ty) / curves.width) / .20);
    const Point source_local{tx + (sx - tx) * fade, ty + (sy - ty) * fade};
    const Point sample = to_frame(source_local.x, source_local.y, curves);
    map_x[index] = sample.x;
    map_y[index] = sample.y;
    target_upper[index] = tui;
    target_lower[index] = tli;
    moved[index] =
        std::hypot(sample.x - frame.x, sample.y - frame.y) > .025 ? 1U : 0U;
    return true;
  };
  for (std::uint32_t y = 0; y < patch.height; ++y)
    for (std::uint32_t x = 0; x < patch.width; ++x) {
      const std::size_t i = static_cast<std::size_t>(y) * patch.width + x;
      if (!mapped_at(x, y, i))
        return empty;
    }
  double minimum = std::numeric_limits<double>::max(), surface_min = minimum;
  auto value = [&](const std::vector<double> &field, int x, int y) {
    x = std::clamp(x, 0, static_cast<int>(patch.width) - 1);
    y = std::clamp(y, 0, static_cast<int>(patch.height) - 1);
    return field[static_cast<std::size_t>(y) * patch.width +
                 static_cast<std::size_t>(x)];
  };
  for (int y = 0; y < static_cast<int>(patch.height); ++y)
    for (int x = 0; x < static_cast<int>(patch.width); ++x) {
      const double ddx_x =
          (value(map_x, x + 1, y) - value(map_x, x - 1, y)) /
          (x == 0 || x + 1 == static_cast<int>(patch.width) ? 1.0 : 2.0);
      const double ddy_x =
          (value(map_x, x, y + 1) - value(map_x, x, y - 1)) /
          (y == 0 || y + 1 == static_cast<int>(patch.height) ? 1.0 : 2.0);
      const double ddx_y =
          (value(map_y, x + 1, y) - value(map_y, x - 1, y)) /
          (x == 0 || x + 1 == static_cast<int>(patch.width) ? 1.0 : 2.0);
      const double ddy_y =
          (value(map_y, x, y + 1) - value(map_y, x, y - 1)) /
          (y == 0 || y + 1 == static_cast<int>(patch.height) ? 1.0 : 2.0);
      const double determinant = ddx_x * ddy_y - ddy_x * ddx_y;
      minimum = std::min(minimum, determinant);
      const std::size_t i = static_cast<std::size_t>(y) * patch.width + x;
      const Point local = to_local(
          {static_cast<double>(left + x), static_cast<double>(top + y)},
          curves);
      if (!oral || local.y < target_upper[i] - 1.1 ||
          local.y > target_lower[i] + 1.1)
        surface_min = std::min(surface_min, determinant);
    }
  evidence.minimum_inverse_jacobian = minimum;
  evidence.minimum_lip_surface_inverse_jacobian = surface_min;
  if (!std::isfinite(minimum) || minimum <= 0 || !std::isfinite(surface_min) ||
      surface_min < .05)
    return empty;
  double current_context = 0;
  std::size_t context_count = 0;
  if (oral) {
    const int radius = std::max(2, static_cast<int>(curves.width * .75));
    const int cx = static_cast<int>(curves.center.x),
              cy = static_cast<int>(curves.center.y);
    for (int y = std::max(0, cy - radius);
         y < std::min(static_cast<int>(source.lease.height), cy + radius); ++y)
      for (int x = std::max(0, cx - radius);
           x < std::min(static_cast<int>(source.lease.width), cx + radius);
           ++x) {
        for (std::size_t c = 0; c < 3U; ++c)
          current_context += raw_channel(source, x, y, c);
        context_count += 3U;
      }
  }
  const double gain =
      oral ? std::clamp(
                 (current_context / std::max<std::size_t>(1U, context_count)) /
                     oral->reference_context_mean,
                 .6, 1.4)
           : 1;
  const double inner_left = std::max(
      to_local({tracking.mouth_landmarks.contour[10U].x * source.lease.width,
                tracking.mouth_landmarks.contour[10U].y * source.lease.height},
               curves)
          .x,
      curves.x.front());
  const double inner_right = std::min(
      to_local({tracking.mouth_landmarks.contour[14U].x * source.lease.width,
                tracking.mouth_landmarks.contour[14U].y * source.lease.height},
               curves)
          .x,
      curves.x.back());
  for (std::uint32_t y = 0; y < patch.height; ++y) {
    for (std::uint32_t x = 0; x < patch.width; ++x) {
      const std::size_t i = static_cast<std::size_t>(y) * patch.width + x;
      const std::size_t offset =
          static_cast<std::size_t>(y) * patch.stride_bytes + x * 4U;
      std::array<double, 3U> base{};
      if (moved[i]) {
        for (std::size_t channel = 0; channel < 3U; ++channel) {
          base[channel] = sample_cubic(source, map_x[i], map_y[i], channel);
        }
      }

      double oral_alpha = 0.0;
      std::array<double, 3U> oral_colour{};
      if (oral && !shape.contact) {
        const Point local = to_local(
            {static_cast<double>(left + x), static_cast<double>(top + y)},
            curves);
        const double horizontal_falloff =
            smooth_unit((.85 - std::abs(local.x) / curves.width) / .26);
        // Oral U follows the pre-fade inverse source column, exactly
        // as v10 does; it is independent of vertical lip deformation.
        const double source_x = local.x / (1.0 + shape.articulation_strength *
                                                     (shape.width_scale - 1.0) *
                                                     horizontal_falloff);
        const double u =
            (source_x - inner_left) / std::max(1.0, inner_right - inner_left);
        const double v = (local.y - target_upper[i]) /
                         std::max(.01, target_lower[i] - target_upper[i]);
        const double distance =
            std::min(local.y - target_upper[i], target_lower[i] - local.y);
        const double support = unit((distance - .65) / .8);
        if (u > 0.0 && u < 1.0 && v > 0.0 && v < 1.0 && support > 0.0) {
          const auto sampled_oral = sample_oral(*oral, u, v);
          const double patch_alpha = sampled_oral[3U] / 255.0;
          oral_alpha = patch_alpha * support;
          if (patch_alpha > 1.0e-9) {
            for (std::size_t channel = 0; channel < 3U; ++channel) {
              oral_colour[channel] = std::clamp(
                  sampled_oral[channel] / patch_alpha * gain, 0.0, 255.0);
            }
          }
        }
      }

      if (oral_alpha > 1.0e-6)
        evidence.oral_reference_used = true;
      if (moved[i]) {
        for (std::size_t channel = 0; channel < 3U; ++channel) {
          const double blended_channel = base[channel] * (1.0 - oral_alpha) +
                                         oral_colour[channel] * oral_alpha;
          patch.premultiplied_bgra[offset + channel] =
              static_cast<std::uint8_t>(
                  std::clamp(std::lround(blended_channel), 0L, 255L));
        }
        patch.premultiplied_bgra[offset + 3U] = 255U;
      } else if (oral_alpha > 1.0e-6) {
        for (std::size_t channel = 0; channel < 3U; ++channel) {
          patch.premultiplied_bgra[offset + channel] =
              static_cast<std::uint8_t>(std::clamp(
                  std::lround(oral_colour[channel] * oral_alpha), 0L, 255L));
        }
        patch.premultiplied_bgra[offset + 3U] = static_cast<std::uint8_t>(
            std::clamp(std::lround(oral_alpha * 255.0), 0L, 255L));
      }
    }
  }
  if (evidence_out)
    *evidence_out = evidence;
  return patch;
}

} // namespace npc::mouth
