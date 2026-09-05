#pragma once

#include "npc/mouth_worker/types.hpp"

#include <algorithm>
#include <cstdint>
#include <istream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace npc::mouth {

// Offline comparison input only. This is not a streaming speech recognizer.
// Integer sample intervals and an exact WAV hash prevent unrelated/stale
// recognition output from being used to score a rendered utterance.
struct ReviewMouthCue {
    std::uint64_t first_sample{};
    std::uint64_t end_sample{};
    Viseme viseme{Viseme::silence};
};

inline std::vector<ReviewMouthCue> read_review_cues(
    std::istream& input, const std::uint32_t sample_rate,
    const std::uint64_t sample_count, const std::string_view audio_sha256) {
    std::string magic, digest;
    std::uint64_t declared_rate{}, declared_count{};
    if (!(input >> magic >> declared_rate >> declared_count >> digest) ||
        magic != "npc-mouth-cues-v1" || declared_rate != sample_rate ||
        declared_count != sample_count || digest != audio_sha256 ||
        digest.size() != 64U || sample_count == 0U) {
        throw std::runtime_error("review cue audio binding mismatch");
    }
    std::vector<ReviewMouthCue> result;
    for (;;) {
        input >> std::ws;
        if (input.eof()) break;
        std::uint64_t first{}, end{};
        unsigned int viseme{};
        if (!(input >> first >> end >> viseme) ||
            viseme > static_cast<unsigned int>(Viseme::spread_vowel) ||
            first != (result.empty() ? 0U : result.back().end_sample) ||
            first >= end || end > sample_count || result.size() >= 10'000U) {
            throw std::runtime_error("invalid or discontinuous review mouth cue");
        }
        result.push_back({first, end, static_cast<Viseme>(viseme)});
    }
    if (result.empty() || result.back().end_sample != sample_count) {
        throw std::runtime_error("review cues must cover the complete audio");
    }
    return result;
}

inline Viseme review_viseme_at(const std::vector<ReviewMouthCue>& cues,
                              const std::uint64_t sample) noexcept {
    const auto found = std::lower_bound(cues.begin(), cues.end(), sample,
        [](const ReviewMouthCue& cue, const std::uint64_t position) {
            return cue.end_sample <= position;
        });
    return found != cues.end() && sample >= found->first_sample
        ? found->viseme : Viseme::silence;
}

} // namespace npc::mouth
