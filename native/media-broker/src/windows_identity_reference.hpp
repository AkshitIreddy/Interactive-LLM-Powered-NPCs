#pragma once

#ifdef _WIN32

#include "npc/media_broker/types.hpp"

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace npc::media::windows {

struct DecodedIdentityReference {
    std::vector<std::byte> bgra;
    std::uint32_t width{};
    std::uint32_t height{};
    std::uint32_t stride_bytes{};
    std::string media_type;
    std::string source_asset_sha256;
};

// The selected path and encoded file bytes remain confined to this function.
// Only bounded normalized pixels and nonsecret digests leave it.
[[nodiscard]] bool pick_and_decode_identity_reference(DecodedIdentityReference& decoded,
                                                      Failure& failure);

} // namespace npc::media::windows

#endif
