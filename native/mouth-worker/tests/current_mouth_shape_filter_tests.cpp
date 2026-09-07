#include "npc/mouth_worker/current_mouth_shape_filter.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <iostream>
#include <string_view>

namespace {

using namespace npc::mouth;

int failures = 0;

void expect(const bool condition, const std::string_view message) {
  if (!condition) {
    std::cerr << "FAIL: " << message << '\n';
    ++failures;
  }
}

constexpr std::uint32_t frame_width = 240U;
constexpr std::uint32_t frame_height = 180U;

struct Point {
  double x{};
  double y{};
};

[[nodiscard]] Point point(const NormalizedLandmark &landmark) {
  return {landmark.x * frame_width, landmark.y * frame_height};
}

void set_point(TrackingEvidence &tracking, const std::size_t index,
               const double x, const double y) {
  tracking.mouth_landmarks.contour[index] = {x / frame_width, y / frame_height,
                                             1.0};
}

[[nodiscard]] TrackingEvidence make_tracking(const Nanoseconds measured_at_ns) {
  TrackingEvidence tracking{};
  tracking.track = {4U, 3U, 2U, 1U};
  tracking.frame = {9U, 8U, 7U, measured_at_ns - 1'000'000};
  tracking.face_bounds = {0.05, 0.05, 0.90, 0.90};
  tracking.mouth_bounds = {0.30, 0.38, 0.40, 0.24};
  tracking.mouth_landmarks.schema_version = 2U;
  tracking.mouth_landmarks.provider_instance_id = 6U;
  tracking.mouth_landmarks.contour_points = mouth_contour_point_count;
  const std::array<Point, mouth_contour_point_count> points{{
      {88.0, 84.0},
      {100.0, 80.0},
      {120.0, 78.0},
      {140.0, 80.0},
      {152.0, 84.0},
      {152.0, 96.0},
      {140.0, 100.0},
      {120.0, 102.0},
      {100.0, 100.0},
      {88.0, 96.0},
      {80.0, 90.0},
      {92.0, 88.0},
      {120.0, 86.0},
      {148.0, 88.0},
      {160.0, 90.0},
      {148.0, 92.0},
      {120.0, 94.0},
      {92.0, 92.0},
  }};
  for (std::size_t index = 0U; index < points.size(); ++index) {
    set_point(tracking, index, points[index].x, points[index].y);
  }
  tracking.mouth_landmarks.left_corner = tracking.mouth_landmarks.contour[10U];
  tracking.mouth_landmarks.right_corner = tracking.mouth_landmarks.contour[14U];
  tracking.mouth_landmarks.upper_lip_center =
      tracking.mouth_landmarks.contour[12U];
  tracking.mouth_landmarks.lower_lip_center =
      tracking.mouth_landmarks.contour[16U];
  tracking.face_confidence = 1.0;
  tracking.landmark_confidence = 1.0;
  tracking.visibility_ratio = 1.0;
  tracking.measured_at_ns = measured_at_ns;
  return tracking;
}

[[nodiscard]] TrackingEvidence rigid_transform(TrackingEvidence tracking,
                                               const Point new_center,
                                               const double scale,
                                               const double radians) {
  constexpr Point old_center{120.0, 90.0};
  const double cosine = std::cos(radians);
  const double sine = std::sin(radians);
  for (std::size_t index = 0U; index < mouth_contour_point_count; ++index) {
    const Point original = point(tracking.mouth_landmarks.contour[index]);
    const double x = (original.x - old_center.x) * scale;
    const double y = (original.y - old_center.y) * scale;
    set_point(tracking, index, new_center.x + x * cosine - y * sine,
              new_center.y + x * sine + y * cosine);
  }
  tracking.mouth_landmarks.left_corner = tracking.mouth_landmarks.contour[10U];
  tracking.mouth_landmarks.right_corner = tracking.mouth_landmarks.contour[14U];
  tracking.mouth_landmarks.upper_lip_center =
      tracking.mouth_landmarks.contour[12U];
  tracking.mouth_landmarks.lower_lip_center =
      tracking.mouth_landmarks.contour[16U];
  return tracking;
}

[[nodiscard]] double distance(const NormalizedLandmark &first,
                              const NormalizedLandmark &second) {
  const Point a = point(first);
  const Point b = point(second);
  return std::hypot(a.x - b.x, a.y - b.y);
}

void test_rigid_motion_roll_and_scale_never_lag() {
  CurrentMouthShapeFilter filter{};
  const auto first = make_tracking(1'000'000'000);
  const auto first_output = filter.filter(first, frame_width, frame_height);
  auto moved =
      rigid_transform(make_tracking(1'016'666'667), {151.0, 111.0}, 1.28, 0.31);
  const auto output = filter.filter(moved, frame_width, frame_height);

  for (std::size_t index = 0U; index < mouth_contour_point_count; ++index) {
    expect(distance(output.mouth_landmarks.contour[index],
                    moved.mouth_landmarks.contour[index]) < 1.0e-8,
           "unchanged local shape follows current translation, roll and scale "
           "exactly");
  }
  expect(distance(first_output.mouth_landmarks.left_corner,
                  first.mouth_landmarks.left_corner) < 1.0e-8,
         "first sample is exact current geometry");
  expect(distance(output.mouth_landmarks.left_corner,
                  moved.mouth_landmarks.left_corner) < 1.0e-8 &&
             distance(output.mouth_landmarks.right_corner,
                      moved.mouth_landmarks.right_corner) < 1.0e-8,
         "current corners are never shape-filtered away from the pose basis");
}

void test_local_noise_is_bounded_without_freezing_shape() {
  CurrentMouthShapeFilter filter{};
  const auto first = make_tracking(2'000'000'000);
  static_cast<void>(filter.filter(first, frame_width, frame_height));
  auto noisy = make_tracking(2'016'666'667);
  const Point noisy_inner = point(noisy.mouth_landmarks.contour[12U]);
  set_point(noisy, 12U, noisy_inner.x, noisy_inner.y + 8.0);
  noisy.mouth_landmarks.upper_lip_center = noisy.mouth_landmarks.contour[12U];
  // Deliberately undersized: output must union its filtered contour support.
  noisy.mouth_bounds = {0.49, 0.49, 0.02, 0.02};

  const auto output = filter.filter(noisy, frame_width, frame_height);
  const Point filtered_inner = point(output.mouth_landmarks.contour[12U]);
  const Point current_inner = point(noisy.mouth_landmarks.contour[12U]);
  const double correction = current_inner.y - filtered_inner.y;
  expect(correction > 0.5 && correction <= 1.2000001,
         "25 ms local EMA reacts while capping one-frame correction at .015 "
         "mouth widths");

  for (const auto &landmark : output.mouth_landmarks.contour) {
    const Point filtered = point(landmark);
    expect(output.mouth_bounds.x * frame_width <= filtered.x - 0.499999 &&
               output.mouth_bounds.y * frame_height <= filtered.y - 0.499999 &&
               output.mouth_bounds.right() * frame_width >=
                   filtered.x + 0.499999 &&
               output.mouth_bounds.bottom() * frame_height >=
                   filtered.y + 0.499999,
           "mouth bounds contain every filtered contour point with half-pixel "
           "support");
  }

  filter.reset();
  const auto reset_output = filter.filter(noisy, frame_width, frame_height);
  expect(distance(reset_output.mouth_landmarks.contour[12U],
                  noisy.mouth_landmarks.contour[12U]) < 1.0e-8,
         "reset removes stale local shape before a new segment or track");
}

void test_track_change_starts_from_current_shape() {
  CurrentMouthShapeFilter filter{};
  static_cast<void>(
      filter.filter(make_tracking(3'000'000'000), frame_width, frame_height));
  auto next_track = make_tracking(3'016'666'667);
  next_track.track.track_id = 99U;
  const Point original = point(next_track.mouth_landmarks.contour[16U]);
  set_point(next_track, 16U, original.x, original.y + 7.0);
  next_track.mouth_landmarks.lower_lip_center =
      next_track.mouth_landmarks.contour[16U];
  const auto output = filter.filter(next_track, frame_width, frame_height);
  expect(distance(output.mouth_landmarks.contour[16U],
                  next_track.mouth_landmarks.contour[16U]) < 1.0e-8,
         "a different track cannot inherit local mouth shape history");
}

void test_reversed_raw_corners_preserve_semantic_adapter_authority() {
  CurrentMouthShapeFilter filter{};
  auto reversed = make_tracking(4'000'000'000);
  for (std::size_t index = 0U; index < mouth_contour_point_count; ++index) {
    const Point original = point(reversed.mouth_landmarks.contour[index]);
    set_point(reversed, index, 240.0 - original.x, original.y);
  }
  // Raw OSF 58 (contour 10) is now screen-right. The adapter's semantic
  // anchors remain sorted and can contain its own closed-mouth repair.
  reversed.mouth_landmarks.left_corner = reversed.mouth_landmarks.contour[14U];
  reversed.mouth_landmarks.right_corner = reversed.mouth_landmarks.contour[10U];
  reversed.mouth_landmarks.upper_lip_center = {120.0 / frame_width,
                                               89.75 / frame_height, 0.93};
  reversed.mouth_landmarks.lower_lip_center = {120.0 / frame_width,
                                               90.25 / frame_height, 0.91};

  const auto output = filter.filter(reversed, frame_width, frame_height);
  expect(distance(output.mouth_landmarks.contour[10U],
                  reversed.mouth_landmarks.contour[10U]) < 1.0e-8 &&
             distance(output.mouth_landmarks.contour[14U],
                      reversed.mouth_landmarks.contour[14U]) < 1.0e-8,
         "reversed raw OSF corners remain exact current contour samples");
  expect(distance(output.mouth_landmarks.left_corner,
                  reversed.mouth_landmarks.left_corner) < 1.0e-8 &&
             distance(output.mouth_landmarks.right_corner,
                      reversed.mouth_landmarks.right_corner) < 1.0e-8,
         "semantic left and right ordering remains adapter-owned");
  expect(distance(output.mouth_landmarks.upper_lip_center,
                  reversed.mouth_landmarks.upper_lip_center) < 1.0e-8 &&
             distance(output.mouth_landmarks.lower_lip_center,
                      reversed.mouth_landmarks.lower_lip_center) < 1.0e-8,
         "semantic upper/lower canonicalization remains adapter-owned");
}

} // namespace

int main() {
  test_rigid_motion_roll_and_scale_never_lag();
  test_local_noise_is_bounded_without_freezing_shape();
  test_track_change_starts_from_current_shape();
  test_reversed_raw_corners_preserve_semantic_adapter_authority();
  if (failures != 0) {
    std::cerr << failures
              << " current-mouth shape filter assertion(s) failed\n";
    return 1;
  }
  std::cout << "current-mouth shape filter tests passed\n";
  return 0;
}
