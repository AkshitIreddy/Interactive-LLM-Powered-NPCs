#pragma once

#include "npc/mouth_worker/compositor.hpp"

#include <cstdint>
#include <vector>

namespace npc::mouth {

// One identity-observed or enrollment-teacher state. The coefficient vector is
// the state-space address; provider visemes and the causal PCM fallback both
// resolve through the same bounded native selector.
struct MouthAtlasState {
    MouthCoefficients coefficients;
    CanonicalMouthPatch appearance;
};

// Atlas ownership is deliberately exact. A worker may render it only for this
// actor and cancellation generation; scene/track/frame authority remains in
// the ordinary WorkItem and presentation gates.
struct CharacterMouthAtlas {
    // Schema 1: complete lip observation. Schema 2: normalized oral interior
    // with source-derived exterior lips. Schema 3: photometrically calibrated
    // full-lip references in one fixed canonical coordinate frame; state zero
    // is the mandatory closed neutral reference. Each state must match the
    // schema. The wire layout stays fixed; older workers reject newer schemas
    // at admission.
    std::uint32_t schema_version{1};
    std::uint64_t cancellation_generation{};
    std::uint64_t actor_id{};
    std::uint64_t identity_revision{};
    std::vector<MouthAtlasState> states;
};

} // namespace npc::mouth
