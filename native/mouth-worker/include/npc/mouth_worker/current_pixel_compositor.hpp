#pragma once

#include "npc/mouth_worker/compositor.hpp"

#include <optional>

namespace npc::mouth {

struct CurrentPixelMouthShape {
  double aperture{};
  double width_scale{1.0};
  double articulation_strength{1.0};
  bool contact{};
};

// The source-edge path is opt-in because its signed-contrast evidence is useful
// for qualified dark-vermilion profiles but is not a generic lip segmenter.
struct CurrentPixelCompositorPolicy {
  bool refine_source_edges{};
  double minimum_mouth_width_pixels{24.0};
  double minimum_edge_contrast{2.0};
  // Keep 1.0 for reviewed oral references. A source-only caller can soften
  // contact so an unobserved exact seal does not flatten distinctive lipstick
  // or pull the current corners into a synthetic line.
  double contact_articulation_strength{1.0};
  // Tests and a playback-clocked coordinator can supply the continuous v10
  // aperture trajectory directly.  Ordinary callers derive a conservative
  // shape from MouthCoefficients.
  std::optional<CurrentPixelMouthShape> shape_override;
};

struct CurrentPixelCompositorEvidence {
  bool source_edge_refined{};
  bool source_edge_contact{};
  bool contact_occludes_cavity{};
  bool oral_reference_used{};
  double mouth_width_pixels{};
  double source_gap_pixels{};
  double target_gap_pixels{};
  double source_edge_upper_contrast{};
  double source_edge_lower_contrast{};
  double source_edge_inner_margin_pixels{};
  double minimum_inverse_jacobian{};
  double minimum_lip_surface_inverse_jacobian{};
};

// Schema-four coefficient calibration.  It leaves the legacy compositor's
// coefficient table untouched: jaw_open carries aperture / 0.20, funnel
// carries bounded contraction, and symmetric smile carries bounded widening.
[[nodiscard]] MouthCoefficients
current_pixel_coefficients_for_viseme(Viseme viseme,
                                      double strength = 1.0) noexcept;

// Deform the newest source pixels through ordered upper/lower lip strips.  A
// schema-four normalized oral strip may contribute only inside the eroded
// newly exposed aperture.  A valid all-transparent strip is source-only.
// Returned pixels are an exact-frame premultiplied residual; failure is an
// empty ResidualPatch and never mutates source.
[[nodiscard]] ResidualPatch compose_current_pixel_residual(
    const CpuFrame &source, const TrackBinding &track,
    const TrackingEvidence &tracking,
    const CanonicalMouthPatch *normalized_oral_strip,
    const MouthCoefficients &coefficients, Nanoseconds produced_at_ns,
    CurrentPixelCompositorPolicy policy = {},
    CurrentPixelCompositorEvidence *evidence = nullptr);

} // namespace npc::mouth
