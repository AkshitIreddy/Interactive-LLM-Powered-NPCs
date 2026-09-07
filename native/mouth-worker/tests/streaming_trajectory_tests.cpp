#include "npc/mouth_worker/streaming_trajectory.hpp"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <iostream>
#include <limits>
#include <span>
#include <string_view>
#include <vector>

namespace {

using namespace npc::mouth;

void expect(const bool condition, const std::string_view message) {
    if (!condition) {
        std::cerr << "FAIL: " << message << '\n';
        std::exit(1);
    }
}

[[nodiscard]] bool near(const double first, const double second,
                        const double tolerance = 1.0e-9) {
    return std::abs(first - second) <= tolerance;
}

[[nodiscard]] MouthCoefficients neutral() {
    MouthCoefficients value{};
    value.lip_close = 1.0;
    return value;
}

[[nodiscard]] MouthCoefficients open() {
    MouthCoefficients value{};
    value.jaw_open = 0.88;
    value.lip_close = 0.04;
    value.upper_lip_raise = 0.44;
    value.lower_lip_depress = 0.72;
    return value;
}

[[nodiscard]] MouthCoefficients rounded() {
    MouthCoefficients value{};
    value.jaw_open = 0.34;
    value.lip_close = 0.12;
    value.funnel = 0.86;
    value.pucker = 0.78;
    return value;
}

[[nodiscard]] MouthCoefficients contact() {
    MouthCoefficients value{};
    value.lip_close = 0.95;
    value.pucker = 0.22;
    return value;
}

[[nodiscard]] AudioClockBinding clock(
    const std::uint64_t sample, const std::uint64_t generation = 1U,
    const std::uint64_t segment = 7U, const std::uint32_t rate = 48'000U) {
    AudioClockBinding value{};
    value.stream_generation = generation;
    value.segment_id = segment;
    value.first_sample_index = sample;
    value.sample_count = 480U;
    value.playback_sample_index = sample;
    value.sample_rate = rate;
    value.channels = 1U;
    value.playback_at_ns = 1'000'000'000LL +
        static_cast<Nanoseconds>(sample) * 1'000'000'000LL /
            static_cast<Nanoseconds>(rate);
    return value;
}

void future_targets_are_inert_until_bounded_anticipation() {
    StreamingTrajectoryPolicy policy{};
    policy.maximum_audio_lead_ms = 500U;
    policy.anticipation_ms = 500U;
    StreamingMouthTrajectory trajectory(1U, policy);
    expect(trajectory.policy().maximum_audio_lead_ms == 120U &&
               trajectory.policy().anticipation_ms == 120U,
           "the public policy is hard-capped at 120 ms");

    const std::vector targets{TimedMouthTarget{6'000U, 2'400U, open(), false}};
    const auto origin = trajectory.sample(clock(0U), targets);
    expect(origin.usable() && near(origin.coefficients.jaw_open, 0.0) &&
               !origin.anticipated,
           "a target more than 120 ms ahead is completely inert");

    const auto boundary = trajectory.sample(clock(240U), targets);
    expect(boundary.usable() && near(boundary.coefficients.jaw_open, 0.0) &&
               !boundary.anticipated,
           "the outer lead boundary has zero influence");

    const auto inside = trajectory.sample(clock(480U), targets);
    expect(inside.usable() && inside.anticipated &&
               inside.coefficients.jaw_open > 0.0 &&
               inside.coefficients.jaw_open < open().jaw_open,
           "an already-delivered target moves only inside bounded anticipation");
}

void spring_is_monotone_without_overshoot_at_irregular_cadence() {
    StreamingMouthTrajectory trajectory;
    const std::vector targets{
        TimedMouthTarget{0U, 6'000U, open(), false},
        TimedMouthTarget{6'000U, 6'000U, rounded(), false},
    };
    auto previous = trajectory.sample(clock(0U), targets).coefficients;
    for (const std::uint64_t sample : {120U, 721U, 1'900U, 3'333U}) {
        const auto current = trajectory.sample(clock(sample), targets).coefficients;
        expect(current.jaw_open >= previous.jaw_open &&
                   current.jaw_open <= open().jaw_open &&
                   current.lip_close <= previous.lip_close &&
                   current.lip_close >= open().lip_close,
               "irregular spring steps approach the active cue monotonically");
        previous = current;
    }

    const auto before_change = trajectory.sample(clock(5'999U), targets).coefficients;
    const auto after_change = trajectory.sample(clock(6'000U), targets).coefficients;
    const auto after_irregular = trajectory.sample(clock(7'337U), targets).coefficients;
    expect(after_change.jaw_open <= before_change.jaw_open &&
               after_change.jaw_open >= rounded().jaw_open &&
               after_irregular.jaw_open <= after_change.jaw_open &&
               after_irregular.jaw_open >= rounded().jaw_open,
           "a target reversal rebases momentum and cannot overshoot");
}

void exact_contact_holds_at_sample_boundaries() {
    StreamingMouthTrajectory trajectory;
    const std::vector targets{
        TimedMouthTarget{0U, 4'800U, open(), false},
        TimedMouthTarget{4'800U, 1'200U, contact(), true},
        TimedMouthTarget{6'000U, 3'000U, rounded(), false},
    };
    (void)trajectory.sample(clock(0U), targets);
    const auto preparation = trajectory.sample(clock(4'000U), targets);
    expect(preparation.anticipated && preparation.coefficients.jaw_open > 0.0,
           "contact anticipation prepares closure without asserting it early");

    const auto first_contact = trajectory.sample(clock(4'800U), targets);
    const auto held_contact = trajectory.sample(clock(5'999U), targets);
    expect(first_contact.exact_contact && held_contact.exact_contact &&
               near(first_contact.coefficients.jaw_open, 0.0) &&
               near(first_contact.coefficients.lip_close, 1.0) &&
               near(held_contact.coefficients.jaw_open, 0.0) &&
               near(held_contact.coefficients.lip_close, 1.0),
           "the complete active contact interval is an exact anatomical seal");

    const auto released = trajectory.sample(clock(6'000U), targets);
    expect(!released.exact_contact && released.coefficients.jaw_open >= 0.0 &&
               released.coefficients.jaw_open <= rounded().jaw_open,
           "contact releases toward the next cue without an overshoot");
}

void incremental_delivery_matches_an_available_full_prefix() {
    const std::vector full{
        TimedMouthTarget{0U, 4'800U, open(), false},
        TimedMouthTarget{4'800U, 4'800U, rounded(), false},
    };
    StreamingMouthTrajectory full_schedule;
    StreamingMouthTrajectory incremental;
    for (const std::uint64_t sample : {0U, 1'200U, 2'399U, 2'400U,
                                       2'880U, 3'600U, 4'799U, 4'800U,
                                       5'600U}) {
        const std::span<const TimedMouthTarget> available = sample < 2'400U
            ? std::span<const TimedMouthTarget>(full.data(), 1U)
            : std::span<const TimedMouthTarget>(full);
        const auto expected = full_schedule.sample(clock(sample), full);
        const auto actual = incremental.sample(clock(sample), available);
        expect(near(actual.coefficients.jaw_open, expected.coefficients.jaw_open) &&
                   near(actual.coefficients.lip_close, expected.coefficients.lip_close) &&
                   near(actual.coefficients.funnel, expected.coefficients.funnel) &&
                   near(actual.coefficients.pucker, expected.coefficients.pucker),
               "a cue delivered by its anticipation boundary matches full-prefix motion");
    }
}

void single_sample_snapshots_keep_coarticulation_history() {
    StreamingMouthTrajectory trajectory;
    const auto sample_snapshot = [&](const std::uint64_t sample,
                                     const MouthCoefficients& coefficients,
                                     const bool exact_contact = false) {
        const TimedMouthTarget target{sample, 1U, coefficients, exact_contact};
        return trajectory.sample(clock(sample), std::span(&target, 1U));
    };

    const auto origin = sample_snapshot(0U, open());
    const auto opening = sample_snapshot(480U, open());
    const auto changed_shape = sample_snapshot(960U, rounded());
    expect(origin.disposition == TrajectoryDisposition::reset_sampled &&
               near(origin.coefficients.jaw_open, 0.0) &&
               opening.coefficients.jaw_open > 0.0 &&
               changed_shape.disposition == TrajectoryDisposition::sampled &&
               changed_shape.coefficients.jaw_open > 0.0,
           "count-one snapshots retain spring history instead of resetting per cue");

    const auto sealed = sample_snapshot(1'440U, contact(), true);
    expect(sealed.exact_contact && near(sealed.coefficients.jaw_open, 0.0) &&
               near(sealed.coefficients.lip_close, 1.0) &&
               near(sealed.coefficients.pucker, contact().pucker),
           "an exact count-one contact seals anatomy while preserving its shape channels");

    const auto release = sample_snapshot(1'920U, open());
    expect(!release.exact_contact && release.coefficients.jaw_open > 0.0 &&
               release.coefficients.jaw_open < open().jaw_open &&
               release.coefficients.lip_close < 1.0 &&
               release.coefficients.lip_close > open().lip_close,
           "a following snapshot releases continuously from exact contact");
}

void cancellation_and_segment_changes_reset_all_motion() {
    const std::vector targets{TimedMouthTarget{0U, 12'000U, open(), false}};
    StreamingMouthTrajectory trajectory;
    (void)trajectory.sample(clock(0U), targets);
    const auto moving = trajectory.sample(clock(2'400U), targets);
    expect(moving.coefficients.jaw_open > 0.0,
           "fixture establishes non-neutral spring state");

    expect(trajectory.cancel_to(2U), "a newer cancellation generation is accepted");
    expect(!trajectory.cancel_to(2U), "cancellation generation cannot repeat or regress");
    expect(near(trajectory.current().jaw_open, 0.0) &&
               near(trajectory.current().lip_close, 1.0),
           "cancellation returns exact neutral immediately");
    const auto stale = trajectory.sample(clock(2'880U, 1U), targets);
    expect(stale.disposition == TrajectoryDisposition::stale_generation &&
               near(stale.coefficients.jaw_open, 0.0),
           "old-generation audio cannot revive cancelled motion");

    const auto new_generation = trajectory.sample(clock(0U, 2U, 8U), targets);
    expect(new_generation.disposition == TrajectoryDisposition::reset_sampled &&
               near(new_generation.coefficients.jaw_open, 0.0),
           "the first sample of a replacement segment starts from exact neutral");
    const auto resumed = trajectory.sample(clock(960U, 2U, 8U), targets);
    expect(resumed.coefficients.jaw_open > 0.0,
           "the replacement segment advances from a cleared spring");

    const auto another_segment = trajectory.sample(clock(0U, 2U, 9U), targets);
    expect(another_segment.disposition == TrajectoryDisposition::reset_sampled &&
               near(another_segment.coefficients.jaw_open, 0.0) &&
               near(another_segment.coefficients.lip_close, 1.0),
           "segment identity is also a hard trajectory reset boundary");
}

void malformed_targets_and_clock_regression_fail_to_neutral() {
    StreamingMouthTrajectory trajectory;
    const std::vector good{TimedMouthTarget{0U, 9'600U, open(), false}};
    (void)trajectory.sample(clock(0U), good);
    (void)trajectory.sample(clock(2'400U), good);

    const std::vector overlap{
        TimedMouthTarget{0U, 2'000U, open(), false},
        TimedMouthTarget{1'999U, 2'000U, rounded(), false},
    };
    const auto malformed = trajectory.sample(clock(2'800U), overlap);
    expect(malformed.disposition == TrajectoryDisposition::invalid_targets &&
               near(malformed.coefficients.jaw_open, 0.0) &&
               near(trajectory.current().jaw_open, 0.0),
           "overlap forbidden by the playback contract fails to neutral");

    (void)trajectory.sample(clock(3'000U), good);
    const auto regression = trajectory.sample(clock(2'999U), good);
    expect(regression.disposition == TrajectoryDisposition::regressed_clock &&
               near(regression.coefficients.jaw_open, 0.0) &&
               near(trajectory.current().lip_close, 1.0),
           "a regressed playback cursor clears velocity and stale shape");

    auto invalid = good;
    invalid[0].coefficients.jaw_open = std::numeric_limits<double>::quiet_NaN();
    expect(trajectory.sample(clock(3'100U), invalid).disposition ==
               TrajectoryDisposition::invalid_targets,
           "non-finite coefficient metadata is rejected");
}

} // namespace

int main() {
    future_targets_are_inert_until_bounded_anticipation();
    spring_is_monotone_without_overshoot_at_irregular_cadence();
    exact_contact_holds_at_sample_boundaries();
    incremental_delivery_matches_an_available_full_prefix();
    single_sample_snapshots_keep_coarticulation_history();
    cancellation_and_segment_changes_reset_all_motion();
    malformed_targets_and_clock_regression_fail_to_neutral();
    std::cout << "streaming trajectory tests passed\n";
    return 0;
}
