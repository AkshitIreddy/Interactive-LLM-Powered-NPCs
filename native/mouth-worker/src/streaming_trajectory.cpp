#include "npc/mouth_worker/streaming_trajectory.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

namespace npc::mouth {
namespace {

constexpr std::uint32_t minimum_sample_rate = 8'000U;
constexpr std::uint32_t maximum_sample_rate = 192'000U;
constexpr std::uint32_t maximum_response_ms = 250U;

[[nodiscard]] MouthCoefficients neutral_coefficients() noexcept {
    MouthCoefficients value{};
    value.lip_close = 1.0;
    return value;
}

[[nodiscard]] std::array<double, 8U> as_array(
    const MouthCoefficients& value) noexcept {
    return {
        value.jaw_open,
        value.lip_close,
        value.funnel,
        value.pucker,
        value.smile_left,
        value.smile_right,
        value.upper_lip_raise,
        value.lower_lip_depress,
    };
}

[[nodiscard]] MouthCoefficients as_coefficients(
    const std::array<double, 8U>& value) noexcept {
    return {
        value[0U], value[1U], value[2U], value[3U],
        value[4U], value[5U], value[6U], value[7U],
    };
}

[[nodiscard]] bool valid_coefficients(
    const MouthCoefficients& value) noexcept {
    const auto channels = as_array(value);
    return std::all_of(channels.begin(), channels.end(), [](const double channel) {
        return std::isfinite(channel) && channel >= 0.0 && channel <= 1.0;
    });
}

[[nodiscard]] bool valid_clock(const AudioClockBinding& clock) noexcept {
    if (clock.stream_generation == 0U || clock.segment_id == 0U ||
        clock.sample_rate < minimum_sample_rate ||
        clock.sample_rate > maximum_sample_rate || clock.channels == 0U ||
        clock.channels > 8U || clock.sample_count == 0U ||
        clock.playback_at_ns <= 0 ||
        clock.first_sample_index >
            std::numeric_limits<std::uint64_t>::max() - clock.sample_count) {
        return false;
    }
    const auto end = clock.first_sample_index + clock.sample_count;
    return clock.playback_sample_index >= clock.first_sample_index &&
           clock.playback_sample_index < end;
}

[[nodiscard]] bool valid_targets(
    const std::span<const TimedMouthTarget> targets,
    const std::size_t maximum_targets) noexcept {
    if (targets.size() > maximum_targets) {
        return false;
    }
    std::uint64_t previous_end{};
    bool have_previous{};
    for (const auto& target : targets) {
        if (target.sample_count == 0U || !valid_coefficients(target.coefficients) ||
            target.first_sample_index >
                std::numeric_limits<std::uint64_t>::max() - target.sample_count) {
            return false;
        }
        const auto end = target.first_sample_index + target.sample_count;
        // The native playback contract has already rejected overlapping or
        // out-of-order provider cues. Recheck that invariant at this public
        // boundary so a malformed direct caller cannot create ambiguity.
        if (have_previous && target.first_sample_index < previous_end) {
            return false;
        }
        previous_end = end;
        have_previous = true;
    }
    return true;
}

[[nodiscard]] std::uint64_t milliseconds_to_samples(
    const std::uint32_t milliseconds,
    const std::uint32_t sample_rate) noexcept {
    return static_cast<std::uint64_t>(sample_rate) * milliseconds / 1'000U;
}

[[nodiscard]] double smoothstep(const double value) noexcept {
    const double bounded = std::clamp(value, 0.0, 1.0);
    return bounded * bounded * (3.0 - 2.0 * bounded);
}

[[nodiscard]] MouthCoefficients blend(const MouthCoefficients& first,
                                      const MouthCoefficients& second,
                                      const double amount) noexcept {
    const auto left = as_array(first);
    const auto right = as_array(second);
    std::array<double, 8U> result{};
    const double bounded = std::clamp(amount, 0.0, 1.0);
    for (std::size_t index = 0U; index < result.size(); ++index) {
        result[index] = left[index] + (right[index] - left[index]) * bounded;
    }
    return as_coefficients(result);
}

[[nodiscard]] MouthCoefficients contact_coefficients(
    MouthCoefficients value) noexcept {
    value.jaw_open = 0.0;
    value.lip_close = 1.0;
    return value;
}

[[nodiscard]] bool is_neutral(const MouthCoefficients& value) noexcept {
    const auto neutral = neutral_coefficients();
    const auto channels = as_array(value);
    const auto reference = as_array(neutral);
    for (std::size_t index = 0U; index < channels.size(); ++index) {
        if (std::abs(channels[index] - reference[index]) > 1.0e-9) {
            return false;
        }
    }
    return true;
}

struct SelectedTarget {
    MouthCoefficients coefficients{neutral_coefficients()};
    bool exact_contact{};
    bool anticipated{};
};

[[nodiscard]] SelectedTarget select_target(
    const std::span<const TimedMouthTarget> targets,
    const std::uint64_t playback_sample,
    const std::uint64_t maximum_lead_samples,
    const std::uint64_t anticipation_samples) noexcept {
    const TimedMouthTarget* active{};
    const TimedMouthTarget* next{};
    for (const auto& target : targets) {
        const auto end = target.first_sample_index + target.sample_count;
        if (playback_sample >= target.first_sample_index && playback_sample < end) {
            active = &target;
            continue;
        }
        if (target.first_sample_index > playback_sample) {
            next = &target;
            break;
        }
    }

    if (active != nullptr && active->exact_contact) {
        return {contact_coefficients(active->coefficients), true, false};
    }

    SelectedTarget selected{};
    if (active != nullptr) {
        selected.coefficients = active->coefficients;
    }
    if (next == nullptr || anticipation_samples == 0U) {
        return selected;
    }
    const auto distance = next->first_sample_index - playback_sample;
    if (distance > maximum_lead_samples || distance > anticipation_samples) {
        return selected;
    }

    const double proximity = 1.0 - static_cast<double>(distance) /
                                       static_cast<double>(anticipation_samples);
    selected.coefficients = blend(
        selected.coefficients,
        next->exact_contact ? contact_coefficients(next->coefficients)
                            : next->coefficients,
        smoothstep(proximity));
    selected.anticipated = proximity > 0.0;
    return selected;
}

void advance_spring_channel(double& current, double& velocity,
                            const double target, const double elapsed_seconds,
                            const double omega) noexcept {
    if (elapsed_seconds <= 0.0 || !std::isfinite(elapsed_seconds) ||
        !std::isfinite(omega) || omega <= 0.0) {
        return;
    }
    const double delta = target - current;
    if (std::abs(delta) <= 1.0e-12) {
        current = target;
        velocity = 0.0;
        return;
    }

    // A critically damped response is monotone when the incoming velocity is
    // directed at the target and no larger than omega * displacement. Rebase
    // on target reversals, then retain safe momentum across ordinary cadence.
    const double maximum_toward_velocity = omega * delta;
    if (delta > 0.0) {
        velocity = std::clamp(velocity, 0.0, maximum_toward_velocity);
    } else {
        velocity = std::clamp(velocity, maximum_toward_velocity, 0.0);
    }

    const double error = current - target;
    const double term = velocity + omega * error;
    const double decay = std::exp(-omega * elapsed_seconds);
    double next = target + (error + term * elapsed_seconds) * decay;
    double next_velocity =
        (velocity - omega * term * elapsed_seconds) * decay;

    const double lower = std::min(current, target);
    const double upper = std::max(current, target);
    if (!std::isfinite(next) || !std::isfinite(next_velocity)) {
        next = target;
        next_velocity = 0.0;
    }
    next = std::clamp(next, lower, upper);
    if (next == lower || next == upper) {
        if (std::abs(next - target) <= 1.0e-12) {
            next_velocity = 0.0;
        }
    }
    current = std::clamp(next, 0.0, 1.0);
    velocity = next_velocity;
}

} // namespace

StreamingMouthTrajectory::StreamingMouthTrajectory(
    const std::uint64_t initial_generation,
    StreamingTrajectoryPolicy policy) noexcept
    : policy_(policy),
      active_generation_(std::max<std::uint64_t>(1U, initial_generation)) {
    policy_.maximum_audio_lead_ms = std::min(
        policy_.maximum_audio_lead_ms, maximum_streaming_audio_lead_ms);
    policy_.anticipation_ms = std::min(
        policy_.anticipation_ms, policy_.maximum_audio_lead_ms);
    policy_.ordinary_response_ms = std::clamp(
        policy_.ordinary_response_ms, 1U, maximum_response_ms);
    policy_.release_response_ms = std::clamp(
        policy_.release_response_ms, 1U, maximum_response_ms);
    policy_.maximum_targets = std::clamp<std::size_t>(policy_.maximum_targets, 1U, 128U);
    reset_motion();
}

StreamingTrajectorySample StreamingMouthTrajectory::sample(
    const AudioClockBinding& playback,
    const std::span<const TimedMouthTarget> currently_available_targets) noexcept {
    const auto rejected = [&](const TrajectoryDisposition disposition) {
        StreamingTrajectorySample result{};
        result.disposition = disposition;
        result.coefficients = neutral_coefficients();
        result.playback_sample_index = playback.playback_sample_index;
        return result;
    };

    if (!valid_clock(playback)) {
        reset_motion();
        return rejected(TrajectoryDisposition::invalid_clock);
    }
    if (playback.stream_generation < active_generation_) {
        return rejected(TrajectoryDisposition::stale_generation);
    }
    if (!valid_targets(currently_available_targets, policy_.maximum_targets)) {
        reset_motion();
        return rejected(TrajectoryDisposition::invalid_targets);
    }

    bool reset_applied = false;
    if (playback.stream_generation > active_generation_) {
        active_generation_ = playback.stream_generation;
        active_segment_id_ = 0U;
        reset_motion();
        reset_applied = true;
    }
    if (active_segment_id_ != playback.segment_id ||
        (have_cursor_ && previous_sample_rate_ != playback.sample_rate)) {
        active_segment_id_ = playback.segment_id;
        reset_motion();
        reset_applied = true;
    }
    if (have_cursor_ && playback.playback_sample_index < previous_playback_sample_) {
        reset_motion();
        return rejected(TrajectoryDisposition::regressed_clock);
    }

    const auto maximum_lead_samples = milliseconds_to_samples(
        policy_.maximum_audio_lead_ms, playback.sample_rate);
    const auto anticipation_samples = milliseconds_to_samples(
        policy_.anticipation_ms, playback.sample_rate);
    const auto selected = select_target(
        currently_available_targets, playback.playback_sample_index,
        maximum_lead_samples, anticipation_samples);

    const std::uint64_t elapsed_samples = have_cursor_
        ? playback.playback_sample_index - previous_playback_sample_
        : 0U;
    const double elapsed_seconds = static_cast<double>(elapsed_samples) /
                                   static_cast<double>(playback.sample_rate);

    if (selected.exact_contact) {
        current_ = selected.coefficients;
        velocity_.fill(0.0);
    } else {
        auto current_channels = as_array(current_);
        const auto target_channels = as_array(selected.coefficients);
        const std::uint32_t response_ms = is_neutral(selected.coefficients)
            ? policy_.release_response_ms
            : policy_.ordinary_response_ms;
        const double omega = 4'000.0 / static_cast<double>(response_ms);
        for (std::size_t index = 0U; index < current_channels.size(); ++index) {
            advance_spring_channel(current_channels[index], velocity_[index],
                                   target_channels[index], elapsed_seconds, omega);
        }
        current_ = as_coefficients(current_channels);
    }

    previous_playback_sample_ = playback.playback_sample_index;
    previous_sample_rate_ = playback.sample_rate;
    have_cursor_ = true;

    return {
        reset_applied ? TrajectoryDisposition::reset_sampled
                      : TrajectoryDisposition::sampled,
        current_,
        playback.playback_sample_index,
        selected.exact_contact,
        selected.anticipated,
    };
}

bool StreamingMouthTrajectory::cancel_to(
    const std::uint64_t new_generation) noexcept {
    if (new_generation <= active_generation_) {
        return false;
    }
    active_generation_ = new_generation;
    active_segment_id_ = 0U;
    reset_motion();
    return true;
}

void StreamingMouthTrajectory::reset(const std::uint64_t generation) noexcept {
    active_generation_ = std::max<std::uint64_t>(1U, generation);
    active_segment_id_ = 0U;
    reset_motion();
}

std::uint64_t StreamingMouthTrajectory::active_generation() const noexcept {
    return active_generation_;
}

const MouthCoefficients& StreamingMouthTrajectory::current() const noexcept {
    return current_;
}

const StreamingTrajectoryPolicy& StreamingMouthTrajectory::policy() const noexcept {
    return policy_;
}

void StreamingMouthTrajectory::reset_motion() noexcept {
    previous_playback_sample_ = 0U;
    previous_sample_rate_ = 0U;
    have_cursor_ = false;
    current_ = neutral_coefficients();
    velocity_.fill(0.0);
}

} // namespace npc::mouth
