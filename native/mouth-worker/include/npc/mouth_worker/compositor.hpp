#pragma once

#include "npc/mouth_worker/types.hpp"

#include <span>

namespace npc::mouth {

// One enrollment-generated mouth appearance in canonical mouth coordinates.
// Pixels are BGRA8 premultiplied alpha so interpolation and presentation remain
// deterministic. The active character/identity revision owns the collection;
// this type intentionally carries no actor-selection authority.
struct CanonicalMouthPatch {
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    std::vector<std::uint8_t> premultiplied_bgra;
};

[[nodiscard]] MouthCoefficients coefficients_for_viseme(Viseme viseme,
                                                        double strength = 1.0) noexcept;

// Deterministic, causal fallback. This is an energy/zero-crossing mouth driver,
// not a phoneme recognizer. Prefer provider/TTS visemes when available.
[[nodiscard]] MouthCoefficients coefficients_from_pcm(std::span<const float> interleaved_pcm,
                                                      std::uint32_t sample_rate,
                                                      std::uint16_t channels) noexcept;

[[nodiscard]] bool valid_cpu_frame(const CpuFrame& frame) noexcept;

[[nodiscard]] ResidualPatch compose_current_frame_residual(const CpuFrame& source,
                                                           const TrackBinding& track,
                                                           const TrackingEvidence& tracking,
                                                           const MouthCoefficients& coefficients,
                                                           Nanoseconds produced_at_ns);

// Warp and interpolate two enrollment-generated atlas states into the current
// tracked mouth rectangle. The caller must still pass the result through the
// same worker validation and presentation-time frame/track/cancellation gates
// as every other residual. This primitive does not perform color adaptation;
// callers must reject atlas use outside its enrolled illumination envelope.
[[nodiscard]] ResidualPatch compose_atlas_residual(const CpuFrame& source,
                                                   const TrackBinding& track,
                                                   const TrackingEvidence& tracking,
                                                   const CanonicalMouthPatch& primary,
                                                   const CanonicalMouthPatch& secondary,
                                                   double secondary_weight,
                                                   Nanoseconds produced_at_ns);

// Test/demo helper which composites a validated residual over a copy of its
// exact source. It never modifies source.bgra.
[[nodiscard]] std::vector<std::uint8_t> composite_over_source(const CpuFrame& source,
                                                             const ResidualPatch& residual);

} // namespace npc::mouth
