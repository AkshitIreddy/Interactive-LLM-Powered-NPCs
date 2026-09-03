#include "npc/mouth_worker/compositor.hpp"
#include "npc/mouth_worker/worker.hpp"

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

using namespace npc::mouth;

[[nodiscard]] CpuFrame make_portrait() {
    CpuFrame frame{};
    frame.lease.schema_version = 1U;
    frame.lease.transport = LeaseTransport::cpu_reference;
    frame.lease.lease_nonce_high = 0x53594e5448455449ULL;
    frame.lease.lease_nonce_low = 0x4350524f4f460001ULL;
    frame.lease.width = 640U;
    frame.lease.height = 360U;
    frame.lease.stride_bytes = frame.lease.width * 4U;
    frame.lease.expires_at_ns = 1'100'000'000;
    frame.identity = {400U, 8U, 2U, 1'000'000'000};
    frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) * frame.lease.height);
    for (std::uint32_t y = 0; y < frame.lease.height; ++y) {
        for (std::uint32_t x = 0; x < frame.lease.width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * frame.lease.stride_bytes +
                                static_cast<std::size_t>(x) * 4U;
            const double dx = (static_cast<double>(x) - 320.0) / 155.0;
            const double dy = (static_cast<double>(y) - 180.0) / 145.0;
            const bool face = dx * dx + dy * dy < 1.0;
            const double mouth_dx = (static_cast<double>(x) - 320.0) / 64.0;
            const double mouth_dy = (static_cast<double>(y) - 229.0) / 13.0;
            const bool mouth = mouth_dx * mouth_dx + mouth_dy * mouth_dy < 1.0;
            if (mouth) {
                frame.bgra[offset + 0U] = 42U;
                frame.bgra[offset + 1U] = 28U;
                frame.bgra[offset + 2U] = 115U;
            } else if (face) {
                frame.bgra[offset + 0U] = 153U;
                frame.bgra[offset + 1U] = 191U;
                frame.bgra[offset + 2U] = 225U;
            } else {
                frame.bgra[offset + 0U] = static_cast<std::uint8_t>(42U + y / 8U);
                frame.bgra[offset + 1U] = static_cast<std::uint8_t>(31U + x / 18U);
                frame.bgra[offset + 2U] = 31U;
            }
            frame.bgra[offset + 3U] = 255U;
        }
    }
    return frame;
}

void write_ppm(const std::filesystem::path& path,
               const std::vector<std::uint8_t>& bgra,
               const std::uint32_t width,
               const std::uint32_t height,
               const std::uint32_t stride) {
    std::ofstream stream(path, std::ios::binary);
    if (!stream) {
        throw std::runtime_error("could not create " + path.string());
    }
    stream << "P6\n" << width << ' ' << height << "\n255\n";
    for (std::uint32_t y = 0; y < height; ++y) {
        for (std::uint32_t x = 0; x < width; ++x) {
            const auto offset = static_cast<std::size_t>(y) * stride +
                                static_cast<std::size_t>(x) * 4U;
            const char rgb[]{static_cast<char>(bgra[offset + 2U]),
                             static_cast<char>(bgra[offset + 1U]),
                             static_cast<char>(bgra[offset + 0U])};
            stream.write(rgb, 3);
        }
    }
}

[[nodiscard]] std::uint64_t digest(const std::vector<std::uint8_t>& bytes) {
    std::uint64_t value = 1469598103934665603ULL;
    for (const auto byte : bytes) {
        value = (value ^ byte) * 1099511628211ULL;
    }
    return value;
}

} // namespace

int main(int argc, char** argv) {
    try {
        const std::filesystem::path output_directory = argc > 1
            ? std::filesystem::path(argv[1])
            : std::filesystem::current_path() / "mouth-worker-proof";
        std::filesystem::create_directories(output_directory);

        WorkItem item{};
        item.track = {5U, 77U, 91U, 6U};
        item.source = make_portrait();
        item.tracking.track = item.track;
        item.tracking.frame = item.source.identity;
        item.tracking.face_bounds = {0.25, 0.09, 0.50, 0.82};
        item.tracking.mouth_bounds = {0.385, 0.56, 0.23, 0.16};
        item.tracking.mouth_landmarks.schema_version = 1U;
        item.tracking.mouth_landmarks.provider_instance_id = 0x53594e5448455449ULL;
        item.tracking.mouth_landmarks.left_corner = {0.40, 0.64, 0.99};
        item.tracking.mouth_landmarks.right_corner = {0.60, 0.64, 0.99};
        item.tracking.mouth_landmarks.upper_lip_center = {0.50, 0.59, 0.99};
        item.tracking.mouth_landmarks.lower_lip_center = {0.50, 0.69, 0.99};
        item.tracking.pose = {0.0, 0.0, 0.0};
        item.tracking.face_confidence = 0.99;
        item.tracking.landmark_confidence = 0.99;
        item.tracking.visibility_ratio = 1.0;
        item.tracking.measured_at_ns = 1'002'000'000;
        item.drive.kind = DriveKind::timed_viseme;
        item.drive.clock.stream_generation = item.track.cancellation_generation;
        item.drive.clock.segment_id = 44U;
        item.drive.clock.sample_rate = 48'000U;
        item.drive.clock.channels = 1U;
        item.drive.clock.playback_at_ns = item.source.identity.captured_at_ns;
        item.drive.viseme = Viseme::open_vowel;
        item.drive.viseme_strength = 1.0;
        item.deadline_ns = 1'050'000'000;

        const auto source_copy = item.source;
        ReferenceMouthWorker worker(item.track.cancellation_generation);
        if (!worker.submit(item)) {
            throw std::runtime_error("synthetic work was not accepted");
        }
        const auto result = worker.process_latest(item.source.identity, 1'010'000'000);
        if (!result.has_residual()) {
            throw std::runtime_error("synthetic work bypassed: " +
                                     std::string(to_string(result.disposition)));
        }
        const auto composited = composite_over_source(source_copy, result.residual);
        if (source_copy.bgra != item.source.bgra || composited == source_copy.bgra) {
            throw std::runtime_error("immutability or visible-change proof failed");
        }

        write_ppm(output_directory / "current-frame.ppm", source_copy.bgra,
                  source_copy.lease.width, source_copy.lease.height, source_copy.lease.stride_bytes);
        write_ppm(output_directory / "current-frame-with-residual.ppm", composited,
                  source_copy.lease.width, source_copy.lease.height, source_copy.lease.stride_bytes);
        std::cout << "PASS: exact current frame remained immutable; mouth-only residual composited\n"
                  << "source_digest=" << digest(source_copy.bgra)
                  << " composited_digest=" << digest(composited) << '\n'
                  << "actor=" << result.residual.track.actor_id
                  << " track=" << result.residual.track.track_id
                  << " track_epoch=" << result.residual.track.track_epoch
                  << " frame=" << result.residual.source_frame.sequence << '\n'
                  << "output=" << output_directory.string() << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
