#include "npc/mouth_worker/source_frame_guard.hpp"

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <string_view>

namespace {

using npc::mouth::CpuFrame;
using npc::mouth::FrameIdentity;
using npc::mouth::LeaseTransport;
using npc::mouth::NormalizedLandmark;
using npc::mouth::PixelFormat;
using npc::mouth::SourceFrameAppearanceGuard;
using npc::mouth::SourceFrameGuardDisposition;
using npc::mouth::TrackingEvidence;

void expect(const bool condition, const std::string_view message) {
  if (!condition) {
    std::cerr << "FAIL: " << message << '\n';
    std::exit(1);
  }
}

[[nodiscard]] FrameIdentity identity(const std::uint64_t sequence) {
  return {sequence, 1U, 1U,
          static_cast<npc::mouth::Nanoseconds>(sequence * 33'333'333U)};
}

enum class FrameTreatment {
  stable,
  central_articulation,
  perioral_veil,
  scene_cut,
};

[[nodiscard]] CpuFrame
make_frame(const std::uint64_t sequence,
           const FrameTreatment treatment = FrameTreatment::stable) {
  CpuFrame frame{};
  frame.lease.schema_version = 1U;
  frame.lease.transport = LeaseTransport::cpu_reference;
  frame.lease.width = 160U;
  frame.lease.height = 120U;
  frame.lease.stride_bytes = frame.lease.width * 4U;
  frame.lease.format = PixelFormat::bgra8_unorm_premultiplied;
  frame.identity = identity(sequence);
  frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) *
                    frame.lease.height);
  for (std::uint32_t y = 0U; y < frame.lease.height; ++y) {
    for (std::uint32_t x = 0U; x < frame.lease.width; ++x) {
      const auto offset =
          static_cast<std::size_t>(y) * frame.lease.stride_bytes +
          static_cast<std::size_t>(x) * 4U;
      std::uint8_t blue =
          static_cast<std::uint8_t>(28U + (x * 3U + y * 2U) % 72U);
      std::uint8_t green =
          static_cast<std::uint8_t>(38U + (x * 2U + y * 5U) % 80U);
      std::uint8_t red =
          static_cast<std::uint8_t>(48U + (x * 5U + y * 3U) % 92U);
      if (treatment == FrameTreatment::scene_cut) {
        blue = static_cast<std::uint8_t>(255U - blue);
        green = static_cast<std::uint8_t>(255U - green);
        red = static_cast<std::uint8_t>(255U - red);
      }
      if (treatment == FrameTreatment::perioral_veil && x >= 48U && x <= 112U &&
          y >= 62U && y <= 98U) {
        blue = static_cast<std::uint8_t>((static_cast<unsigned>(blue) + 360U) /
                                         3U);
        green = static_cast<std::uint8_t>(
            (static_cast<unsigned>(green) + 390U) / 3U);
        red =
            static_cast<std::uint8_t>((static_cast<unsigned>(red) + 405U) / 3U);
      }
      if (treatment == FrameTreatment::central_articulation && x >= 69U &&
          x <= 91U && y >= 73U && y <= 81U) {
        blue = 8U;
        green = 12U;
        red = 215U;
      }
      frame.bgra[offset] = blue;
      frame.bgra[offset + 1U] = green;
      frame.bgra[offset + 2U] = red;
      frame.bgra[offset + 3U] = 255U;
    }
  }
  return frame;
}

[[nodiscard]] TrackingEvidence
make_tracking(const std::uint64_t sequence, const std::uint64_t generation = 1U,
              const std::uint64_t actor = 7U, const std::uint64_t track = 11U,
              const std::uint64_t epoch = 1U) {
  TrackingEvidence result{};
  result.track = {generation, actor, track, epoch};
  result.frame = identity(sequence);
  result.face_bounds = {0.20, 0.08, 0.60, 0.84};
  result.mouth_bounds = {0.38, 0.58, 0.24, 0.16};
  result.mouth_landmarks.schema_version = 2U;
  result.mouth_landmarks.provider_instance_id = 9U;
  result.mouth_landmarks.left_corner = {0.42, 0.65, 1.0};
  result.mouth_landmarks.right_corner = {0.58, 0.65, 1.0};
  result.mouth_landmarks.upper_lip_center = {0.50, 0.63, 1.0};
  result.mouth_landmarks.lower_lip_center = {0.50, 0.67, 1.0};
  result.mouth_landmarks.contour_points =
      static_cast<std::uint32_t>(result.mouth_landmarks.contour.size());
  for (std::size_t index = 0U; index < result.mouth_landmarks.contour.size();
       ++index) {
    const double angle =
        2.0 * 3.14159265358979323846 * static_cast<double>(index) /
        static_cast<double>(result.mouth_landmarks.contour.size());
    result.mouth_landmarks.contour[index] = {
        0.50 + std::cos(angle) * 0.08,
        0.65 + std::sin(angle) * 0.035,
        1.0,
    };
  }
  result.face_confidence = 0.99;
  result.landmark_confidence = 0.99;
  result.visibility_ratio = 1.0;
  result.measured_at_ns = result.frame.captured_at_ns;
  return result;
}

void warm_up(SourceFrameAppearanceGuard &guard) {
  for (std::uint64_t sequence = 1U; sequence <= 6U; ++sequence) {
    const auto decision =
        guard.evaluate(make_frame(sequence), make_tracking(sequence));
    expect(decision.disposition == SourceFrameGuardDisposition::warming_up,
           "six stable observations build the baseline without claiming "
           "visibility");
  }
}

void test_stable_and_articulating_source() {
  SourceFrameAppearanceGuard guard;
  warm_up(guard);
  const auto stable = guard.evaluate(make_frame(7U), make_tracking(7U));
  expect(stable.accepted(), "stable source is accepted after warmup");
  const auto articulated = guard.evaluate(
      make_frame(8U, FrameTreatment::central_articulation), make_tracking(8U));
  expect(articulated.accepted(), "central source-mouth articulation is outside "
                                 "the perioral appearance descriptor");
}

void test_local_appearance_latch_and_recovery() {
  SourceFrameAppearanceGuard guard;
  warm_up(guard);
  expect(guard.evaluate(make_frame(7U), make_tracking(7U)).accepted(),
         "local appearance test starts accepted");
  const auto onset = guard.evaluate(
      make_frame(8U, FrameTreatment::perioral_veil), make_tracking(8U));
  expect(onset.disposition ==
             SourceFrameGuardDisposition::bypass_mouth_appearance_change,
         "abrupt perioral veil opens the local appearance latch");
  const auto held = guard.evaluate(
      make_frame(9U, FrameTreatment::perioral_veil), make_tracking(9U));
  expect(!held.accepted(),
         "persistent altered pixels cannot train the frozen baseline");
  const auto recovery_one = guard.evaluate(make_frame(10U), make_tracking(10U));
  const auto recovery_two = guard.evaluate(make_frame(11U), make_tracking(11U));
  const auto recovery_three =
      guard.evaluate(make_frame(12U), make_tracking(12U));
  expect(recovery_one.disposition ==
                 SourceFrameGuardDisposition::bypass_recovering &&
             recovery_two.disposition ==
                 SourceFrameGuardDisposition::bypass_recovering,
         "recovery requires consecutive baseline-consistent observations");
  expect(recovery_three.accepted(),
         "third clean recovery observation closes the latch");
}

void test_scene_cut_is_distinct() {
  SourceFrameAppearanceGuard guard;
  warm_up(guard);
  expect(guard.evaluate(make_frame(7U), make_tracking(7U)).accepted(),
         "scene-cut test starts accepted");
  const auto cut = guard.evaluate(make_frame(8U, FrameTreatment::scene_cut),
                                  make_tracking(8U));
  expect(cut.disposition == SourceFrameGuardDisposition::bypass_scene_change,
         "abrupt whole-face appearance change is reported as a scene change");
}

void test_tracking_and_cancellation_reset() {
  SourceFrameAppearanceGuard guard;
  warm_up(guard);
  expect(guard.appearance_latched(),
         "warm guard reports a stable appearance latch");
  const auto generation = guard.latch_generation();
  guard.notify_tracking_loss();
  expect(!guard.appearance_latched() && guard.latch_generation() > generation,
         "tracking loss clears the learned appearance state");
  expect(guard.evaluate(make_frame(7U), make_tracking(7U)).disposition ==
             SourceFrameGuardDisposition::warming_up,
         "tracking recovery cannot reuse the old baseline");

  expect(guard.cancel_to(2U), "new cancellation generation is admitted");
  expect(!guard.cancel_to(2U), "cancellation generation cannot replay");
  const auto stale = guard.evaluate(make_frame(8U), make_tracking(8U, 1U));
  expect(stale.disposition == SourceFrameGuardDisposition::bypass_cancelled,
         "old-generation tracking is rejected");
  const auto fresh = guard.evaluate(make_frame(9U), make_tracking(9U, 2U));
  expect(fresh.disposition == SourceFrameGuardDisposition::warming_up,
         "fresh generation starts a new baseline");
}

void test_track_change_and_invalid_inputs() {
  SourceFrameAppearanceGuard guard;
  warm_up(guard);
  const auto changed =
      guard.evaluate(make_frame(7U), make_tracking(7U, 1U, 7U, 12U, 2U));
  expect(changed.disposition == SourceFrameGuardDisposition::warming_up,
         "track or epoch change creates a fresh baseline");

  auto invalid = make_frame(8U);
  invalid.lease.stride_bytes = 4U;
  const auto invalid_result =
      guard.evaluate(invalid, make_tracking(8U, 1U, 7U, 12U, 2U));
  expect(invalid_result.disposition ==
             SourceFrameGuardDisposition::bypass_invalid_source,
         "undersized source stride is rejected");

  SourceFrameAppearanceGuard wrong_frame_guard;
  const auto wrong =
      wrong_frame_guard.evaluate(make_frame(1U), make_tracking(2U));
  expect(wrong.disposition == SourceFrameGuardDisposition::bypass_wrong_frame,
         "source and tracking frame identities must match");
}

} // namespace

int main() {
  test_stable_and_articulating_source();
  test_local_appearance_latch_and_recovery();
  test_scene_cut_is_distinct();
  test_tracking_and_cancellation_reset();
  test_track_change_and_invalid_inputs();
  std::cout << "source frame guard tests passed\n";
  return 0;
}
