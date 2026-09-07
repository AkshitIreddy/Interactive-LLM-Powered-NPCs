#include "npc/mouth_worker/current_mouth_shape_filter.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace npc::mouth {
namespace {

constexpr double time_constant_ns = 25'000'000.0;
constexpr double maximum_local_correction = 0.015;

struct Point {
  double x{};
  double y{};
};

[[nodiscard]] bool finite_landmark(const NormalizedLandmark &point) noexcept {
  return std::isfinite(point.x) && std::isfinite(point.y) &&
         std::isfinite(point.confidence);
}

[[nodiscard]] bool valid_geometry(const TrackingEvidence &current,
                                  const std::uint32_t frame_width,
                                  const std::uint32_t frame_height) noexcept {
  if (frame_width == 0U || frame_height == 0U ||
      current.mouth_landmarks.schema_version < 2U ||
      current.mouth_landmarks.contour_points != mouth_contour_point_count) {
    return false;
  }
  return std::all_of(current.mouth_landmarks.contour.begin(),
                     current.mouth_landmarks.contour.end(), finite_landmark);
}

[[nodiscard]] Point frame_point(const NormalizedLandmark &point,
                                const std::uint32_t width,
                                const std::uint32_t height) noexcept {
  return {point.x * static_cast<double>(width),
          point.y * static_cast<double>(height)};
}

[[nodiscard]] NormalizedLandmark
normalized_point(const Point point, const double confidence,
                 const std::uint32_t width,
                 const std::uint32_t height) noexcept {
  return {point.x / static_cast<double>(width),
          point.y / static_cast<double>(height), confidence};
}

[[nodiscard]] bool valid_bounds(const NormalizedRect &bounds) noexcept {
  return std::isfinite(bounds.x) && std::isfinite(bounds.y) &&
         std::isfinite(bounds.width) && std::isfinite(bounds.height) &&
         bounds.x >= 0.0 && bounds.y >= 0.0 && bounds.width > 0.0 &&
         bounds.height > 0.0 && bounds.right() <= 1.0 && bounds.bottom() <= 1.0;
}

} // namespace

TrackingEvidence
CurrentMouthShapeFilter::filter(const TrackingEvidence &current,
                                const std::uint32_t frame_width,
                                const std::uint32_t frame_height) {
  if (!valid_geometry(current, frame_width, frame_height)) {
    reset();
    return current;
  }

  const Point left = frame_point(current.mouth_landmarks.contour[10U],
                                 frame_width, frame_height);
  const Point right = frame_point(current.mouth_landmarks.contour[14U],
                                  frame_width, frame_height);
  const Point center{(left.x + right.x) * 0.5, (left.y + right.y) * 0.5};
  const double dx = right.x - left.x;
  const double dy = right.y - left.y;
  const double mouth_width = std::hypot(dx, dy);
  if (!std::isfinite(mouth_width) || mouth_width < 1.0) {
    reset();
    return current;
  }
  const Point horizontal{dx / mouth_width, dy / mouth_width};
  const Point vertical{-horizontal.y, horizontal.x};

  State next{};
  next.track = current.track;
  next.measured_at_ns = current.measured_at_ns;
  next.frame_width = frame_width;
  next.frame_height = frame_height;
  for (std::size_t index = 0U; index < mouth_contour_point_count; ++index) {
    const Point point = frame_point(current.mouth_landmarks.contour[index],
                                    frame_width, frame_height);
    const double relative_x = point.x - center.x;
    const double relative_y = point.y - center.y;
    next.contour[index] = {
        (relative_x * horizontal.x + relative_y * horizontal.y) / mouth_width,
        (relative_x * vertical.x + relative_y * vertical.y) / mouth_width};
  }
  const auto current_local = next.contour;

  const bool continuous = previous_.has_value() &&
                          previous_->track == current.track &&
                          previous_->frame_width == frame_width &&
                          previous_->frame_height == frame_height &&
                          current.measured_at_ns > previous_->measured_at_ns;
  if (continuous) {
    const double elapsed_ns =
        static_cast<double>(current.measured_at_ns - previous_->measured_at_ns);
    const double blend = 1.0 - std::exp(-elapsed_ns / time_constant_ns);
    const double retained = 1.0 - std::clamp(blend, 0.0, 1.0);
    for (std::size_t index = 0U; index < mouth_contour_point_count; ++index) {
      const double correction_x = std::clamp(
          (previous_->contour[index].x - next.contour[index].x) * retained,
          -maximum_local_correction, maximum_local_correction);
      const double correction_y = std::clamp(
          (previous_->contour[index].y - next.contour[index].y) * retained,
          -maximum_local_correction, maximum_local_correction);
      next.contour[index].x += correction_x;
      next.contour[index].y += correction_y;
    }
  }

  // Raw OSF 58 and 62 can appear in either screen-space order. Preserve each
  // exact current raw corner instead of assuming either index means "left".
  next.contour[10U] = current_local[10U];
  next.contour[14U] = current_local[14U];

  TrackingEvidence result = current;
  double minimum_x = std::numeric_limits<double>::max();
  double minimum_y = std::numeric_limits<double>::max();
  double maximum_x = std::numeric_limits<double>::lowest();
  double maximum_y = std::numeric_limits<double>::lowest();
  for (std::size_t index = 0U; index < mouth_contour_point_count; ++index) {
    const auto local = next.contour[index];
    const Point filtered{center.x + mouth_width * (local.x * horizontal.x +
                                                   local.y * vertical.x),
                         center.y + mouth_width * (local.x * horizontal.y +
                                                   local.y * vertical.y)};
    result.mouth_landmarks.contour[index] = normalized_point(
        filtered, current.mouth_landmarks.contour[index].confidence,
        frame_width, frame_height);
    minimum_x = std::min(minimum_x, filtered.x);
    minimum_y = std::min(minimum_y, filtered.y);
    maximum_x = std::max(maximum_x, filtered.x);
    maximum_y = std::max(maximum_y, filtered.y);
  }
  // The adapter owns semantic ordering and its closed-mouth inversion repair.
  // Those exact current anchors must not be overwritten with raw OSF indices.
  result.mouth_landmarks.left_corner = current.mouth_landmarks.left_corner;
  result.mouth_landmarks.right_corner = current.mouth_landmarks.right_corner;
  result.mouth_landmarks.upper_lip_center =
      current.mouth_landmarks.upper_lip_center;
  result.mouth_landmarks.lower_lip_center =
      current.mouth_landmarks.lower_lip_center;

  const double expanded_left = std::max(0.0, minimum_x - 0.5);
  const double expanded_top = std::max(0.0, minimum_y - 0.5);
  const double expanded_right =
      std::min(static_cast<double>(frame_width), maximum_x + 0.5);
  const double expanded_bottom =
      std::min(static_cast<double>(frame_height), maximum_y + 0.5);
  if (valid_bounds(current.mouth_bounds)) {
    const double current_left = current.mouth_bounds.x * frame_width;
    const double current_top = current.mouth_bounds.y * frame_height;
    const double current_right = current.mouth_bounds.right() * frame_width;
    const double current_bottom = current.mouth_bounds.bottom() * frame_height;
    minimum_x = std::min(current_left, expanded_left);
    minimum_y = std::min(current_top, expanded_top);
    maximum_x = std::max(current_right, expanded_right);
    maximum_y = std::max(current_bottom, expanded_bottom);
  } else {
    minimum_x = expanded_left;
    minimum_y = expanded_top;
    maximum_x = expanded_right;
    maximum_y = expanded_bottom;
  }
  result.mouth_bounds = {minimum_x / frame_width, minimum_y / frame_height,
                         (maximum_x - minimum_x) / frame_width,
                         (maximum_y - minimum_y) / frame_height};

  previous_ = next;
  return result;
}

void CurrentMouthShapeFilter::reset() noexcept { previous_.reset(); }

} // namespace npc::mouth
