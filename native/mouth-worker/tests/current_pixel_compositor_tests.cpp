#include "npc/mouth_worker/current_pixel_compositor.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <iostream>
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

[[nodiscard]] CpuFrame make_source() {
  constexpr std::uint32_t width = 240U;
  constexpr std::uint32_t height = 180U;
  CpuFrame frame{};
  frame.lease.schema_version = 1U;
  frame.lease.transport = LeaseTransport::cpu_reference;
  frame.lease.lease_nonce_high = 0x1020304050607080ULL;
  frame.lease.lease_nonce_low = 0x8877665544332211ULL;
  frame.lease.owner_process_id = 41U;
  frame.lease.intended_consumer_process_id = 73U;
  frame.lease.adapter_luid_low = 17U;
  frame.lease.adapter_luid_high = -3;
  frame.lease.width = width;
  frame.lease.height = height;
  frame.lease.stride_bytes = width * 4U;
  frame.lease.expires_at_ns = 4'000'000'000;
  frame.identity = {11U, 12U, 13U, 2'000'000'000};
  frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) *
                    height);

  for (std::uint32_t y = 0U; y < height; ++y) {
    for (std::uint32_t x = 0U; x < width; ++x) {
      const std::size_t offset =
          static_cast<std::size_t>(y) * frame.lease.stride_bytes +
          static_cast<std::size_t>(x) * 4U;
      std::array<std::uint8_t, 3U> colour{
          static_cast<std::uint8_t>(68U + x % 31U),
          static_cast<std::uint8_t>(104U + y % 29U),
          static_cast<std::uint8_t>(156U + (x + y) % 37U)};
      const double lip =
          std::pow((static_cast<double>(x) - 120.0) / 42.0, 2.0) +
          std::pow((static_cast<double>(y) - 90.0) / 13.0, 2.0);
      if (lip <= 1.0)
        colour = {45U, 52U, 142U};
      const double cavity =
          std::pow((static_cast<double>(x) - 120.0) / 30.0, 2.0) +
          std::pow((static_cast<double>(y) - 90.0) / 4.0, 2.0);
      if (cavity <= 1.0)
        colour = {17U, 13U, 22U};
      frame.bgra[offset + 0U] = colour[0U];
      frame.bgra[offset + 1U] = colour[1U];
      frame.bgra[offset + 2U] = colour[2U];
      frame.bgra[offset + 3U] = 255U;
    }
  }
  return frame;
}

void set_point(TrackingEvidence &tracking, const CpuFrame &frame,
               const std::size_t index, const double x, const double y) {
  tracking.mouth_landmarks.contour[index] = {
      x / static_cast<double>(frame.lease.width),
      y / static_cast<double>(frame.lease.height), 1.0};
}

[[nodiscard]] TrackingEvidence make_tracking(const CpuFrame &frame) {
  TrackingEvidence tracking{};
  tracking.track = {7U, 8U, 9U, 10U};
  tracking.frame = frame.identity;
  tracking.face_bounds = {0.03, 0.03, 0.94, 0.94};
  tracking.mouth_bounds = {0.27, 0.36, 0.46, 0.28};
  tracking.mouth_landmarks.schema_version = 2U;
  tracking.mouth_landmarks.provider_instance_id = 81U;
  tracking.mouth_landmarks.contour_points = mouth_contour_point_count;

  // OpenSeeFace 48..65 topology as exposed by MouthLandmarks.contour.
  set_point(tracking, frame, 0U, 88.0, 84.0);
  set_point(tracking, frame, 1U, 100.0, 80.0);
  set_point(tracking, frame, 2U, 120.0, 78.0);
  set_point(tracking, frame, 3U, 140.0, 80.0);
  set_point(tracking, frame, 4U, 152.0, 84.0);
  set_point(tracking, frame, 5U, 152.0, 96.0);
  set_point(tracking, frame, 6U, 140.0, 100.0);
  set_point(tracking, frame, 7U, 120.0, 102.0);
  set_point(tracking, frame, 8U, 100.0, 100.0);
  set_point(tracking, frame, 9U, 88.0, 96.0);
  set_point(tracking, frame, 10U, 80.0, 90.0);
  set_point(tracking, frame, 11U, 92.0, 88.0);
  set_point(tracking, frame, 12U, 120.0, 86.0);
  set_point(tracking, frame, 13U, 148.0, 88.0);
  set_point(tracking, frame, 14U, 160.0, 90.0);
  set_point(tracking, frame, 15U, 148.0, 92.0);
  set_point(tracking, frame, 16U, 120.0, 94.0);
  set_point(tracking, frame, 17U, 92.0, 92.0);
  tracking.mouth_landmarks.left_corner = tracking.mouth_landmarks.contour[10U];
  tracking.mouth_landmarks.right_corner = tracking.mouth_landmarks.contour[14U];
  tracking.mouth_landmarks.upper_lip_center =
      tracking.mouth_landmarks.contour[12U];
  tracking.mouth_landmarks.lower_lip_center =
      tracking.mouth_landmarks.contour[16U];
  tracking.face_confidence = 1.0;
  tracking.landmark_confidence = 1.0;
  tracking.visibility_ratio = 1.0;
  tracking.measured_at_ns = frame.identity.captured_at_ns + 1'000'000;
  return tracking;
}

[[nodiscard]] double arch_at(const double x) {
  const double normalized = (x - 120.0) / 40.0;
  return std::max(0.0, 1.0 - normalized * normalized);
}

[[nodiscard]] TrackingEvidence
make_false_cavity_tracking(const CpuFrame &frame) {
  auto tracking = make_tracking(frame);
  const auto upper_outer = [](const double x) {
    return 90.0 - 12.0 * arch_at(x);
  };
  const auto lower_outer = [](const double x) {
    return 90.0 + 8.0 * arch_at(x);
  };
  const auto upper_inner = [](const double x) {
    return 90.0 - 8.0 * arch_at(x);
  };
  const auto lower_inner = [](const double x) {
    return 90.0 + 2.0 * arch_at(x);
  };
  for (const auto [index, x] : std::array<std::pair<std::size_t, double>, 5U>{
           {{0U, 88.0}, {1U, 100.0}, {2U, 120.0}, {3U, 140.0}, {4U, 152.0}}}) {
    set_point(tracking, frame, index, x, upper_outer(x));
  }
  for (const auto [index, x] : std::array<std::pair<std::size_t, double>, 5U>{
           {{9U, 88.0}, {8U, 100.0}, {7U, 120.0}, {6U, 140.0}, {5U, 152.0}}}) {
    set_point(tracking, frame, index, x, lower_outer(x));
  }
  for (const auto [index, x] : std::array<std::pair<std::size_t, double>, 3U>{
           {{11U, 92.0}, {12U, 120.0}, {13U, 148.0}}}) {
    set_point(tracking, frame, index, x, upper_inner(x));
  }
  for (const auto [index, x] : std::array<std::pair<std::size_t, double>, 3U>{
           {{17U, 92.0}, {16U, 120.0}, {15U, 148.0}}}) {
    set_point(tracking, frame, index, x, lower_inner(x));
  }
  tracking.mouth_landmarks.left_corner = tracking.mouth_landmarks.contour[10U];
  tracking.mouth_landmarks.right_corner = tracking.mouth_landmarks.contour[14U];
  tracking.mouth_landmarks.upper_lip_center =
      tracking.mouth_landmarks.contour[12U];
  tracking.mouth_landmarks.lower_lip_center =
      tracking.mouth_landmarks.contour[16U];
  return tracking;
}

[[nodiscard]] CpuFrame make_false_cavity_source() {
  auto source = make_source();
  for (std::uint32_t y = 0U; y < source.lease.height; ++y) {
    for (std::uint32_t x = 0U; x < source.lease.width; ++x) {
      const std::size_t offset =
          static_cast<std::size_t>(y) * source.lease.stride_bytes +
          static_cast<std::size_t>(x) * 4U;
      std::array<std::uint8_t, 3U> colour{150U, 150U, 150U};
      if (x >= 80U && x <= 160U) {
        const double arch = arch_at(static_cast<double>(x));
        const double actual_upper = 90.0 - 4.0 * arch;
        const double actual_lower = 90.0 + 5.0 * arch;
        if (static_cast<double>(y) >= actual_upper &&
            static_cast<double>(y) <= actual_lower) {
          colour = {25U, 15U, 40U};
        }
        if (y == 90U && x >= 88U && x <= 152U) {
          colour = {10U, 5U, 15U};
        }
      }
      source.bgra[offset + 0U] = colour[0U];
      source.bgra[offset + 1U] = colour[1U];
      source.bgra[offset + 2U] = colour[2U];
      source.bgra[offset + 3U] = 255U;
    }
  }
  return source;
}

[[nodiscard]] CanonicalMouthPatch
make_oral_patch(const bool transparent = false) {
  CanonicalMouthPatch patch{};
  patch.width = 128U;
  patch.height = 64U;
  patch.stride_bytes = patch.width * 4U;
  patch.representation = MouthPatchRepresentation::normalized_oral_strip_v1;
  patch.reference_context_mean = 115.0;
  patch.premultiplied_bgra.assign(
      static_cast<std::size_t>(patch.stride_bytes) * patch.height, 0U);
  if (!transparent) {
    for (std::size_t offset = 0U; offset < patch.premultiplied_bgra.size();
         offset += 4U) {
      // An impossible source colour makes any reference leakage obvious.
      patch.premultiplied_bgra[offset + 0U] = 3U;
      patch.premultiplied_bgra[offset + 1U] = 250U;
      patch.premultiplied_bgra[offset + 2U] = 5U;
      patch.premultiplied_bgra[offset + 3U] = 255U;
    }
  }
  return patch;
}

[[nodiscard]] std::array<std::uint8_t, 4U>
pixel(const std::vector<std::uint8_t> &bytes, const CpuFrame &frame,
      const std::uint32_t x, const std::uint32_t y) {
  const std::size_t offset =
      static_cast<std::size_t>(y) * frame.lease.stride_bytes +
      static_cast<std::size_t>(x) * 4U;
  return {bytes[offset], bytes[offset + 1U], bytes[offset + 2U],
          bytes[offset + 3U]};
}

[[nodiscard]] bool impossible_green(const std::array<std::uint8_t, 4U> value) {
  return value[1U] > 205U && value[0U] < 45U && value[2U] < 55U;
}

void test_coefficients_preserve_silence_and_bilabial_provenance() {
  const auto silence_coefficients =
      current_pixel_coefficients_for_viseme(Viseme::silence);
  const auto bilabial = current_pixel_coefficients_for_viseme(Viseme::bilabial);
  const auto rounded = current_pixel_coefficients_for_viseme(Viseme::rounded);
  expect(silence_coefficients.lip_close == 1.0 &&
             silence_coefficients.pucker == 0.0,
         "silence uses the legacy-compatible closed-neutral sentinel");
  expect(bilabial.lip_close > 0.94 && bilabial.lip_close < 1.0 &&
             bilabial.pucker > 0.2,
         "bilabial keeps explicit contact provenance distinct from silence");
  expect(std::abs(rounded.jaw_open - 0.75) < 1.0e-9 &&
             std::abs(rounded.funnel - 1.0) < 1.0e-9,
         "schema-four rounded coefficients encode v10 aperture and width");
}

void test_silence_is_exact_source_and_metadata_is_bound() {
  const auto source = make_source();
  const auto source_before = source.bgra;
  const auto tracking = make_tracking(source);
  const auto residual = compose_current_pixel_residual(
      source, tracking.track, tracking, nullptr,
      current_pixel_coefficients_for_viseme(Viseme::silence),
      source.identity.captured_at_ns + 4'000'000);
  expect(!residual.premultiplied_bgra.empty(),
         "silence returns a valid transparent residual");
  expect(std::all_of(residual.premultiplied_bgra.begin(),
                     residual.premultiplied_bgra.end(),
                     [](const std::uint8_t value) { return value == 0U; }),
         "silence does not close or repaint current source pixels");
  expect(composite_over_source(source, residual) == source.bgra,
         "silence composite is byte-exact source");
  expect(source.bgra == source_before, "renderer never mutates source storage");
  expect(residual.track == tracking.track &&
             residual.source_frame == source.identity,
         "residual retains exact frame and track authority");
  expect(residual.source_lease.native_handle_value == 0U &&
             residual.residual_lease.owner_process_id ==
                 source.lease.intended_consumer_process_id &&
             residual.residual_lease.intended_consumer_process_id ==
                 source.lease.owner_process_id,
         "CPU residual preserves lease ownership semantics");
}

void test_oral_reference_stays_inside_eroded_aperture() {
  const auto source = make_source();
  const auto tracking = make_tracking(source);
  const auto oral = make_oral_patch();
  CurrentPixelCompositorEvidence evidence{};
  const auto residual = compose_current_pixel_residual(
      source, tracking.track, tracking, &oral,
      current_pixel_coefficients_for_viseme(Viseme::open_vowel),
      source.identity.captured_at_ns + 4'000'000, {}, &evidence);
  const auto output = composite_over_source(source, residual);
  expect(!residual.premultiplied_bgra.empty(),
         "open vowel with oral reference produces a residual");
  expect(evidence.oral_reference_used,
         "oral reference use is reported even where source pixels also move");

  std::size_t oral_pixels = 0U;
  for (std::uint32_t y = 0U; y < source.lease.height; ++y) {
    for (std::uint32_t x = 0U; x < source.lease.width; ++x) {
      if (!impossible_green(pixel(output, source, x, y)))
        continue;
      ++oral_pixels;
      expect(x > 80U && x < 160U && y > 83U && y < 101U,
             "oral pixels remain within the mouth's inner-column aperture");
    }
  }
  expect(oral_pixels > 120U, "open vowel exposes a meaningful oral interior");
  expect(!impossible_green(pixel(output, source, 120U, 81U)) &&
             !impossible_green(pixel(output, source, 120U, 102U)) &&
             !impossible_green(pixel(output, source, 79U, 90U)),
         "reference does not repaint upper lip, lower lip, or mouth corner");
}

void test_contact_hides_cavity_at_partial_strength() {
  const auto source = make_source();
  const auto tracking = make_tracking(source);
  const auto oral = make_oral_patch();
  CurrentPixelCompositorPolicy policy{};
  policy.shape_override = CurrentPixelMouthShape{0.0, 1.0, 0.35, true};
  CurrentPixelCompositorEvidence evidence{};
  const auto residual = compose_current_pixel_residual(
      source, tracking.track, tracking, &oral,
      current_pixel_coefficients_for_viseme(Viseme::bilabial, 0.35),
      source.identity.captured_at_ns + 4'000'000, policy, &evidence);
  const auto output = composite_over_source(source, residual);
  expect(!residual.premultiplied_bgra.empty(),
         "partial-strength bilabial produces a valid contact residual");
  expect(evidence.contact_occludes_cavity && evidence.target_gap_pixels == 0.0,
         "bilabial topology forces full contact independent of cue strength");
  expect(!evidence.oral_reference_used,
         "contact never squeezes oral-reference colour into the seam");
  expect(!impossible_green(pixel(output, source, 120U, 90U)),
         "contact centre contains only current-frame lip pixels");
  const auto centre = pixel(output, source, 120U, 90U);
  expect(centre[0U] > 25U || centre[1U] > 25U || centre[2U] > 35U,
         "contact replaces the source cavity rather than retaining its dark "
         "centre");
}

void test_transparent_oral_patch_is_source_only() {
  const auto source = make_source();
  const auto tracking = make_tracking(source);
  auto oral = make_oral_patch(true);
  oral.reference_context_mean =
      0.0; // legal for all-transparent source-only states
  CurrentPixelCompositorEvidence evidence{};
  const auto residual = compose_current_pixel_residual(
      source, tracking.track, tracking, &oral,
      current_pixel_coefficients_for_viseme(Viseme::open_vowel),
      source.identity.captured_at_ns + 4'000'000, {}, &evidence);
  const auto output = composite_over_source(source, residual);
  expect(!residual.premultiplied_bgra.empty(),
         "all-transparent schema-four patch selects the source-only route");
  expect(!evidence.oral_reference_used,
         "transparent oral state never claims a reference contribution");
  expect(evidence.target_gap_pixels <=
             evidence.source_gap_pixels * 2.25 + 1.0e-9,
         "source-only opening is capped by available current-frame coverage");
  expect(
      std::none_of(output.begin(), output.end(),
                   [](const std::uint8_t value) { return value == 250U; }),
      "transparent oral state cannot invent its impossible reference colour");
}

void test_geometry_rejections_are_fail_closed() {
  const auto source = make_source();
  auto inverted = make_tracking(source);
  std::swap(inverted.mouth_landmarks.contour[12U],
            inverted.mouth_landmarks.contour[16U]);
  const auto inverted_result = compose_current_pixel_residual(
      source, inverted.track, inverted, nullptr,
      current_pixel_coefficients_for_viseme(Viseme::open_vowel),
      source.identity.captured_at_ns + 4'000'000);
  expect(inverted_result.premultiplied_bgra.empty(),
         "crossed OpenSeeFace inner geometry is rejected");

  auto small = make_tracking(source);
  for (auto &point : small.mouth_landmarks.contour) {
    const double x = point.x * source.lease.width;
    point.x = (120.0 + (x - 120.0) * 0.20) / source.lease.width;
  }
  const auto small_result = compose_current_pixel_residual(
      source, small.track, small, nullptr,
      current_pixel_coefficients_for_viseme(Viseme::open_vowel),
      source.identity.captured_at_ns + 4'000'000);
  expect(small_result.premultiplied_bgra.empty(),
         "mouths below the 24-pixel deformation floor bypass");
}

void test_reversed_raw_osf_corner_order_uses_semantic_orientation() {
  const auto source = make_source();
  auto tracking = make_tracking(source);
  for (auto &landmark : tracking.mouth_landmarks.contour) {
    landmark.x = 1.0 - landmark.x;
  }
  tracking.mouth_landmarks.left_corner = tracking.mouth_landmarks.contour[14U];
  tracking.mouth_landmarks.right_corner = tracking.mouth_landmarks.contour[10U];
  tracking.mouth_landmarks.upper_lip_center =
      tracking.mouth_landmarks.contour[12U];
  tracking.mouth_landmarks.lower_lip_center =
      tracking.mouth_landmarks.contour[16U];
  const auto residual = compose_current_pixel_residual(
      source, tracking.track, tracking, nullptr,
      current_pixel_coefficients_for_viseme(Viseme::open_vowel),
      source.identity.captured_at_ns + 4'000'000);
  expect(
      !residual.premultiplied_bgra.empty(),
      "raw OSF 58 screen-right / 62 screen-left topology remains renderable");
}

void test_source_edge_repair_closes_false_mesh_cavity() {
  const auto source = make_false_cavity_source();
  const auto tracking = make_false_cavity_tracking(source);
  CurrentPixelCompositorPolicy policy{};
  policy.refine_source_edges = true;
  policy.shape_override = CurrentPixelMouthShape{0.0, 1.0, 0.35, true};
  CurrentPixelCompositorEvidence evidence{};
  const auto residual = compose_current_pixel_residual(
      source, tracking.track, tracking, nullptr,
      current_pixel_coefficients_for_viseme(Viseme::bilabial, 0.35),
      source.identity.captured_at_ns + 4'000'000, policy, &evidence);
  expect(!residual.premultiplied_bgra.empty(),
         "contrast-qualified false-cavity geometry remains renderable");
  expect(evidence.source_edge_refined && evidence.source_edge_contact,
         "source edges classify the mesh cavity through closed vermilion as "
         "contact");
  expect(evidence.source_edge_upper_contrast >= policy.minimum_edge_contrast &&
             evidence.source_edge_lower_contrast >=
                 policy.minimum_edge_contrast,
         "edge repair is supported by both signed vermilion boundaries");
  expect(evidence.source_edge_inner_margin_pixels < 0.75,
         "corrected outer edges expose the false inner-boundary margin");
  expect(evidence.contact_occludes_cavity && evidence.target_gap_pixels == 0.0,
         "repaired source geometry keeps physical contact at reduced strength");
}

void test_source_edge_repair_rejects_flat_appearance() {
  auto source = make_source();
  for (std::size_t offset = 0U; offset < source.bgra.size(); offset += 4U) {
    source.bgra[offset + 0U] = 80U;
    source.bgra[offset + 1U] = 80U;
    source.bgra[offset + 2U] = 80U;
    source.bgra[offset + 3U] = 255U;
  }
  const auto tracking = make_tracking(source);
  CurrentPixelCompositorPolicy policy{};
  policy.refine_source_edges = true;
  const auto residual = compose_current_pixel_residual(
      source, tracking.track, tracking, nullptr,
      current_pixel_coefficients_for_viseme(Viseme::open_vowel),
      source.identity.captured_at_ns + 4'000'000, policy);
  expect(residual.premultiplied_bgra.empty(),
         "opt-in source-edge repair fails closed without usable contrast");
}

void test_residual_support_has_no_rectangular_edge() {
  const auto source = make_source();
  const auto tracking = make_tracking(source);
  CurrentPixelCompositorPolicy policy{};
  policy.shape_override = CurrentPixelMouthShape{0.15, 0.90, 1.0, false};
  const auto residual = compose_current_pixel_residual(
      source, tracking.track, tracking, nullptr,
      current_pixel_coefficients_for_viseme(Viseme::rounded),
      source.identity.captured_at_ns + 4'000'000, policy);
  expect(!residual.premultiplied_bgra.empty(),
         "rolled-support seam test has a valid residual");
  std::size_t opaque_border_pixels = 0U;
  for (std::uint32_t x = 0U; x < residual.width; ++x) {
    opaque_border_pixels += residual.premultiplied_bgra[x * 4U + 3U] != 0U;
    const std::size_t bottom =
        static_cast<std::size_t>(residual.height - 1U) * residual.stride_bytes +
        x * 4U + 3U;
    opaque_border_pixels += residual.premultiplied_bgra[bottom] != 0U;
  }
  for (std::uint32_t y = 1U; y + 1U < residual.height; ++y) {
    const std::size_t row = static_cast<std::size_t>(y) * residual.stride_bytes;
    opaque_border_pixels += residual.premultiplied_bgra[row + 3U] != 0U;
    opaque_border_pixels +=
        residual.premultiplied_bgra[row + (residual.width - 1U) * 4U + 3U] !=
        0U;
  }
  expect(opaque_border_pixels == 0U,
         "full rotated support reaches an exact transparent boundary");
}

} // namespace

int main() {
  test_coefficients_preserve_silence_and_bilabial_provenance();
  test_silence_is_exact_source_and_metadata_is_bound();
  test_oral_reference_stays_inside_eroded_aperture();
  test_contact_hides_cavity_at_partial_strength();
  test_transparent_oral_patch_is_source_only();
  test_geometry_rejections_are_fail_closed();
  test_reversed_raw_osf_corner_order_uses_semantic_orientation();
  test_source_edge_repair_closes_false_mesh_cavity();
  test_source_edge_repair_rejects_flat_appearance();
  test_residual_support_has_no_rectangular_edge();
  if (failures != 0) {
    std::cerr << failures << " current-pixel compositor assertion(s) failed\n";
    return 1;
  }
  std::cout << "current-pixel compositor tests passed\n";
  return 0;
}
