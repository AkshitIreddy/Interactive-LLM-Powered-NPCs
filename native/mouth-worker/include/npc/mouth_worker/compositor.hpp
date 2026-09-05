#pragma once

#include "npc/mouth_worker/types.hpp"

#include <span>

namespace npc::mouth {

enum class MouthPatchRepresentation : std::uint8_t {
    full_lip_observation_v1 = 0,
    normalized_oral_interior_v1 = 1,
    // A full lip reference sharing one fixed canonical coordinate frame with
    // state zero. Before it is composited, state zero is fitted to the newest
    // source mouth with a bounded per-channel affine colour transform. This is
    // deliberately opt-in: legacy full-lip and oral-interior pixels retain
    // their existing rendering semantics.
    photometric_full_lip_reference_v1 = 2,
};

// One enrollment-generated mouth appearance in canonical mouth coordinates.
// Pixels are BGRA8 premultiplied alpha so interpolation and presentation remain
// deterministic. The active character/identity revision owns the collection;
// this type intentionally carries no actor-selection authority.
struct CanonicalMouthPatch {
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    HeadPoseDegrees enrolled_pose;
    std::vector<std::uint8_t> premultiplied_bgra;
    MouthPatchRepresentation representation{MouthPatchRepresentation::full_lip_observation_v1};
};

[[nodiscard]] MouthCoefficients coefficients_for_viseme(Viseme viseme,
                                                        double strength = 1.0) noexcept;

// Deterministic, causal fallback. This is an energy/zero-crossing mouth driver,
// not a phoneme recognizer. Prefer provider/TTS visemes when available.
[[nodiscard]] MouthCoefficients coefficients_from_pcm(std::span<const float> interleaved_pcm,
                                                      std::uint32_t sample_rate,
                                                      std::uint16_t channels) noexcept;

[[nodiscard]] bool valid_cpu_frame(const CpuFrame& frame) noexcept;

// Normalize an identity-authorized reference frame into mouth-corner space.
// The resulting patch contains real observed lip/oral pixels and a curved,
// feathered alpha support; it never synthesizes teeth, tongue, or a cavity.
[[nodiscard]] CanonicalMouthPatch extract_canonical_mouth_patch(
    const CpuFrame& source,
    const TrackingEvidence& tracking,
    std::uint32_t canonical_width = 192U,
    std::uint32_t canonical_height = 120U);

[[nodiscard]] ResidualPatch compose_current_frame_residual(const CpuFrame& source,
                                                           const TrackBinding& track,
                                                           const TrackingEvidence& tracking,
                                                           const MouthCoefficients& coefficients,
                                                           Nanoseconds produced_at_ns);

// Render source-preserving lip motion and admit pixels from one nearest
// identity-observed atlas state only inside the tracked oral aperture. The
// outer lips, corners, moustache/facial hair, and surrounding skin always come
// from the current source frame. Photographed states are never cross-faded at
// pixel level: doing so creates ghosted teeth and duplicate lip edges. The
// caller must still pass the result through the ordinary frame/track/
// cancellation presentation gates.
[[nodiscard]] ResidualPatch compose_atlas_residual(const CpuFrame& source,
                                                   const TrackBinding& track,
                                                   const TrackingEvidence& tracking,
                                                   const CanonicalMouthPatch& observed_state,
                                                   const MouthCoefficients& coefficients,
                                                   Nanoseconds produced_at_ns);

// Render one schema-three full-lip reference after calibrating it against the
// atlas' closed neutral reference and the exact current source frame. Both
// references must use the same dimensions, stride, pose coordinate frame, and
// photometric representation. Pixels remain confined to the tracked mouth ROI
// and are bound to the caller's exact frame/track/generation authority.
[[nodiscard]] ResidualPatch compose_photometric_atlas_residual(
    const CpuFrame& source,
    const TrackBinding& track,
    const TrackingEvidence& tracking,
    const CanonicalMouthPatch& neutral_reference,
    const CanonicalMouthPatch& observed_state,
    const MouthCoefficients& coefficients,
    Nanoseconds produced_at_ns);

// Test/demo helper which composites a validated residual over a copy of its
// exact source. It never modifies source.bgra.
[[nodiscard]] std::vector<std::uint8_t> composite_over_source(const CpuFrame& source,
                                                             const ResidualPatch& residual);

} // namespace npc::mouth
