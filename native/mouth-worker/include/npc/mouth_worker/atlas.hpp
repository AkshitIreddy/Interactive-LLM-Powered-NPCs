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
    std::uint32_t schema_version{1};
    std::uint64_t cancellation_generation{};
    std::uint64_t actor_id{};
    std::uint64_t identity_revision{};
    std::vector<MouthAtlasState> states;
};

} // namespace npc::mouth
