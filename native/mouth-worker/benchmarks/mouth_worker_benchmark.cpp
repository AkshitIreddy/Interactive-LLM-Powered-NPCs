#include "npc/mouth_worker/compositor.hpp"
#include "npc/mouth_worker/worker.hpp"

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <numeric>
#include <utility>
#include <vector>

namespace {

using namespace npc::mouth;

[[nodiscard]] WorkItem make_benchmark_item(const std::uint64_t sequence,
                                           const Nanoseconds captured_at_ns) {
    WorkItem item{};
    item.track = {9U, 4U, 12U, 3U};
    item.source.lease.schema_version = 1U;
    item.source.lease.transport = LeaseTransport::cpu_reference;
    item.source.lease.lease_nonce_high = 0x42454e43484d4152ULL;
    item.source.lease.lease_nonce_low = sequence;
    item.source.lease.owner_process_id = 100U;
    item.source.lease.intended_consumer_process_id = 200U;
    item.source.lease.width = 1920U;
    item.source.lease.height = 1080U;
    item.source.lease.stride_bytes = item.source.lease.width * 4U;
    item.source.lease.expires_at_ns = captured_at_ns + 1'000'000'000;
    item.source.identity = {sequence, 2U, 3U, captured_at_ns};
    item.source.bgra.resize(static_cast<std::size_t>(item.source.lease.stride_bytes) *
                            item.source.lease.height);
    for (std::uint32_t y = 0; y < item.source.lease.height; ++y) {
        for (std::uint32_t x = 0; x < item.source.lease.width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * item.source.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            item.source.bgra[offset + 0U] = static_cast<std::uint8_t>((x + y) & 0xffU);
            item.source.bgra[offset + 1U] = static_cast<std::uint8_t>((x * 3U) & 0xffU);
            item.source.bgra[offset + 2U] = static_cast<std::uint8_t>((y * 5U) & 0xffU);
            item.source.bgra[offset + 3U] = 255U;
        }
    }
    item.tracking.track = item.track;
    item.tracking.frame = item.source.identity;
    item.tracking.face_bounds = {0.38, 0.2, 0.24, 0.6};
    item.tracking.mouth_bounds = {0.455, 0.58, 0.09, 0.065};
    item.tracking.mouth_landmarks.schema_version = 1U;
    item.tracking.mouth_landmarks.provider_instance_id = 55U;
    item.tracking.mouth_landmarks.left_corner = {0.462, 0.614, 0.98};
    item.tracking.mouth_landmarks.right_corner = {0.538, 0.618, 0.98};
    item.tracking.mouth_landmarks.upper_lip_center = {0.5, 0.596, 0.98};
    item.tracking.mouth_landmarks.lower_lip_center = {0.5, 0.632, 0.98};
    item.tracking.pose = {5.0, -3.0, 4.0};
    item.tracking.face_confidence = 0.98;
    item.tracking.landmark_confidence = 0.97;
    item.tracking.visibility_ratio = 0.96;
    item.tracking.measured_at_ns = captured_at_ns;
    item.drive.kind = DriveKind::timed_viseme;
    item.drive.clock.stream_generation = item.track.cancellation_generation;
    item.drive.clock.segment_id = 1U;
    item.drive.clock.sample_count = 1'600U;
    item.drive.clock.sample_rate = 48'000U;
    item.drive.clock.channels = 1U;
    item.drive.clock.playback_at_ns = captured_at_ns;
    item.drive.viseme = Viseme::open_vowel;
    item.drive.viseme_strength = 0.8;
    item.deadline_ns = captured_at_ns + 500'000'000;
    return item;
}

[[nodiscard]] double percentile(std::vector<double> values, const double quantile) {
    std::sort(values.begin(), values.end());
    const auto index = static_cast<std::size_t>(std::floor(
        quantile * static_cast<double>(values.size() - 1U)));
    return values[index];
}

[[nodiscard]] CanonicalMouthPatch make_atlas_patch(const std::uint8_t red_bias) {
    CanonicalMouthPatch patch{};
    patch.width = 206U;
    patch.height = 143U;
    patch.stride_bytes = patch.width * 4U;
    patch.premultiplied_bgra.resize(
        static_cast<std::size_t>(patch.stride_bytes) * patch.height, 0U);
    for (std::uint32_t y = 0; y < patch.height; ++y) {
        for (std::uint32_t x = 0; x < patch.width; ++x) {
            const double nx = (static_cast<double>(x) + 0.5) /
                                  static_cast<double>(patch.width) * 2.0 - 1.0;
            const double ny = (static_cast<double>(y) + 0.5) /
                                  static_cast<double>(patch.height) * 2.0 - 1.0;
            const auto alpha = static_cast<std::uint8_t>(std::lround(
                std::clamp((1.0 - std::sqrt(nx * nx + ny * ny)) / 0.22, 0.0, 1.0) * 255.0));
            const auto offset = static_cast<std::size_t>(y) * patch.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            patch.premultiplied_bgra[offset + 0U] = static_cast<std::uint8_t>(
                (60U * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 1U] = static_cast<std::uint8_t>(
                (90U * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 2U] = static_cast<std::uint8_t>(
                (static_cast<std::uint32_t>(red_bias) * alpha + 127U) / 255U);
            patch.premultiplied_bgra[offset + 3U] = alpha;
        }
    }
    return patch;
}

} // namespace

int main() {
    constexpr std::size_t warmup_iterations = 20U;
    constexpr std::size_t measured_iterations = 250U;
    constexpr Nanoseconds captured_at_ns = 1'000'000'000;
    const auto prototype = make_benchmark_item(1U, captured_at_ns);
    std::vector<double> milliseconds;
    milliseconds.reserve(measured_iterations);
    std::uint64_t output_checksum = 0;

    for (std::size_t iteration = 0; iteration < warmup_iterations + measured_iterations; ++iteration) {
        auto item = prototype;
        item.source.identity.sequence = static_cast<std::uint64_t>(iteration + 1U);
        item.source.lease.lease_nonce_low = item.source.identity.sequence;
        item.tracking.frame = item.source.identity;
        const auto identity = item.source.identity;
        ReferenceMouthWorker worker(item.track.cancellation_generation);
        const auto start = std::chrono::steady_clock::now();
        (void)worker.submit(std::move(item));
        const auto result = worker.process_latest(identity, captured_at_ns + 5'000'000);
        const auto finish = std::chrono::steady_clock::now();
        if (!result.has_residual()) {
            std::cerr << "benchmark failed open unexpectedly: " << to_string(result.disposition) << '\n';
            return 1;
        }
        for (const auto byte : result.residual.premultiplied_bgra) {
            output_checksum = output_checksum * 131U + byte;
        }
        if (iteration >= warmup_iterations) {
            milliseconds.push_back(
                std::chrono::duration<double, std::milli>(finish - start).count());
        }
    }

    const double mean = std::accumulate(milliseconds.begin(), milliseconds.end(), 0.0) /
                        static_cast<double>(milliseconds.size());
    const auto closed_atlas = make_atlas_patch(120U);
    const auto open_atlas = make_atlas_patch(210U);
    std::vector<double> atlas_milliseconds;
    atlas_milliseconds.reserve(measured_iterations);
    for (std::size_t iteration = 0; iteration < warmup_iterations + measured_iterations;
         ++iteration) {
        const auto start = std::chrono::steady_clock::now();
        const auto residual = compose_atlas_residual(
            prototype.source, prototype.track, prototype.tracking,
            iteration % 2U == 0U ? closed_atlas : open_atlas,
            coefficients_for_viseme(Viseme::open_vowel, 0.85),
            captured_at_ns + 5'000'000);
        const auto finish = std::chrono::steady_clock::now();
        if (residual.premultiplied_bgra.empty()) {
            std::cerr << "atlas benchmark failed to produce a residual\n";
            return 1;
        }
        for (const auto byte : residual.premultiplied_bgra) {
            output_checksum = output_checksum * 131U + byte;
        }
        if (iteration >= warmup_iterations) {
            atlas_milliseconds.push_back(
                std::chrono::duration<double, std::milli>(finish - start).count());
        }
    }
    const double atlas_mean = std::accumulate(
        atlas_milliseconds.begin(), atlas_milliseconds.end(), 0.0) /
        static_cast<double>(atlas_milliseconds.size());
    CharacterMouthAtlas atlas{};
    atlas.cancellation_generation = prototype.track.cancellation_generation;
    atlas.actor_id = prototype.track.actor_id;
    atlas.identity_revision = 1U;
    atlas.states = {
        {coefficients_for_viseme(Viseme::silence), make_atlas_patch(112U)},
        {coefficients_for_viseme(Viseme::rounded), make_atlas_patch(146U)},
        {coefficients_for_viseme(Viseme::open_vowel), make_atlas_patch(210U)},
        {coefficients_for_viseme(Viseme::spread_vowel), make_atlas_patch(184U)},
    };
    ReferenceMouthWorker atlas_worker(prototype.track.cancellation_generation);
    if (!atlas_worker.install_atlas(std::move(atlas))) {
        std::cerr << "atlas benchmark failed to install the identity atlas\n";
        return 1;
    }
    const std::array visemes{
        Viseme::silence, Viseme::rounded, Viseme::open_vowel, Viseme::spread_vowel,
    };
    std::vector<double> atlas_worker_milliseconds;
    atlas_worker_milliseconds.reserve(measured_iterations);
    for (std::size_t iteration = 0U;
         iteration < warmup_iterations + measured_iterations; ++iteration) {
        auto item = prototype;
        item.source.identity.sequence = static_cast<std::uint64_t>(iteration + 10'000U);
        item.source.lease.lease_nonce_low = item.source.identity.sequence;
        item.tracking.frame = item.source.identity;
        item.drive.viseme = visemes[iteration % visemes.size()];
        item.drive.viseme_strength = 1.0;
        const auto identity = item.source.identity;
        const auto start = std::chrono::steady_clock::now();
        (void)atlas_worker.submit(std::move(item));
        const auto result = atlas_worker.process_latest(identity, captured_at_ns + 5'000'000);
        const auto finish = std::chrono::steady_clock::now();
        if (!result.has_residual()) {
            std::cerr << "atlas worker benchmark failed to produce a residual\n";
            return 1;
        }
        for (const auto byte : result.residual.premultiplied_bgra) {
            output_checksum = output_checksum * 131U + byte;
        }
        if (iteration >= warmup_iterations) {
            atlas_worker_milliseconds.push_back(
                std::chrono::duration<double, std::milli>(finish - start).count());
        }
    }
    const double atlas_worker_mean = std::accumulate(
        atlas_worker_milliseconds.begin(), atlas_worker_milliseconds.end(), 0.0) /
        static_cast<double>(atlas_worker_milliseconds.size());
    std::cout << std::fixed << std::setprecision(3)
              << "1920x1080 leased source, 173x71 queue+mouth residual, "
              << measured_iterations << " iterations\n"
              << "geometric_mean_ms=" << mean
              << " p50_ms=" << percentile(milliseconds, 0.50)
              << " p95_ms=" << percentile(milliseconds, 0.95)
              << " p99_ms=" << percentile(milliseconds, 0.99) << '\n'
              << "atlas_206x143_to_173x71_direct_mean_ms=" << atlas_mean
              << " p50_ms=" << percentile(atlas_milliseconds, 0.50)
              << " p95_ms=" << percentile(atlas_milliseconds, 0.95)
              << " p99_ms=" << percentile(atlas_milliseconds, 0.99)
              << '\n'
              << "atlas_worker_select_and_compose_mean_ms=" << atlas_worker_mean
              << " p50_ms=" << percentile(atlas_worker_milliseconds, 0.50)
              << " p95_ms=" << percentile(atlas_worker_milliseconds, 0.95)
              << " p99_ms=" << percentile(atlas_worker_milliseconds, 0.99)
              << " checksum=" << output_checksum << '\n';
    return 0;
}
