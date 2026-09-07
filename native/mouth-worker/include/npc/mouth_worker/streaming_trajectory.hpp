#pragma once

#include "npc/mouth_worker/types.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>

namespace npc::mouth {

// A trajectory may inspect only this much already-delivered audio metadata.
// This is a hard implementation bound, not a promise that a provider supplies
// that much timing. Missing future targets simply produce a causal release.
inline constexpr std::uint32_t maximum_streaming_audio_lead_ms = 120U;

struct TimedMouthTarget {
    std::uint64_t first_sample_index{};
    std::uint64_t sample_count{};
    MouthCoefficients coefficients;

    // Contact targets retain a finite, exact seal while their interval is
    // active. Anticipation may prepare that seal but cannot assert it early.
    bool exact_contact{};
};

struct StreamingTrajectoryPolicy {
    std::uint32_t maximum_audio_lead_ms{maximum_streaming_audio_lead_ms};
    std::uint32_t anticipation_ms{50U};
    std::uint32_t ordinary_response_ms{42U};
    std::uint32_t release_response_ms{62U};
    std::size_t maximum_targets{128U};
};

enum class TrajectoryDisposition : std::uint8_t {
    sampled,
    reset_sampled,
    invalid_clock,
    invalid_targets,
    stale_generation,
    regressed_clock,
};

struct StreamingTrajectorySample {
    TrajectoryDisposition disposition{TrajectoryDisposition::invalid_clock};
    MouthCoefficients coefficients;
    std::uint64_t playback_sample_index{};
    bool exact_contact{};
    bool anticipated{};

    [[nodiscard]] bool usable() const noexcept {
        return disposition == TrajectoryDisposition::sampled ||
               disposition == TrajectoryDisposition::reset_sampled;
    }
};

// Stateful sample-clock trajectory for incrementally available speech cues.
//
// The caller supplies only targets that have actually arrived from playback;
// the class never requires or infers the rest of the utterance. Targets follow
// the playback transport contract: ordered, non-overlapping absolute sample
// intervals for one stream generation and segment. A future target cannot
// influence output before the bounded anticipation interval. The spring is
// critically damped and clamped per coefficient so target changes remain
// monotone and cannot overshoot even with irregular render cadence.
class StreamingMouthTrajectory final {
public:
    explicit StreamingMouthTrajectory(
        std::uint64_t initial_generation = 1U,
        StreamingTrajectoryPolicy policy = {}) noexcept;

    [[nodiscard]] StreamingTrajectorySample sample(
        const AudioClockBinding& playback,
        std::span<const TimedMouthTarget> currently_available_targets) noexcept;

    // Cancellation is an anatomical reset boundary: coefficients become exact
    // neutral and all velocity, cursor, and segment history is discarded.
    [[nodiscard]] bool cancel_to(std::uint64_t new_generation) noexcept;
    void reset(std::uint64_t generation = 1U) noexcept;

    [[nodiscard]] std::uint64_t active_generation() const noexcept;
    [[nodiscard]] const MouthCoefficients& current() const noexcept;
    [[nodiscard]] const StreamingTrajectoryPolicy& policy() const noexcept;

private:
    static constexpr std::size_t coefficient_count = 8U;

    void reset_motion() noexcept;

    StreamingTrajectoryPolicy policy_;
    std::uint64_t active_generation_{};
    std::uint64_t active_segment_id_{};
    std::uint64_t previous_playback_sample_{};
    std::uint32_t previous_sample_rate_{};
    bool have_cursor_{};
    MouthCoefficients current_;
    std::array<double, coefficient_count> velocity_{};
};

[[nodiscard]] constexpr const char* to_string(
    const TrajectoryDisposition value) noexcept {
    switch (value) {
    case TrajectoryDisposition::sampled: return "sampled";
    case TrajectoryDisposition::reset_sampled: return "reset_sampled";
    case TrajectoryDisposition::invalid_clock: return "invalid_clock";
    case TrajectoryDisposition::invalid_targets: return "invalid_targets";
    case TrajectoryDisposition::stale_generation: return "stale_generation";
    case TrajectoryDisposition::regressed_clock: return "regressed_clock";
    }
    return "unknown";
}

} // namespace npc::mouth
