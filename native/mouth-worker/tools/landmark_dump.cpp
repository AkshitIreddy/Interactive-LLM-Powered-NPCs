#include "npc/mouth_worker/landmark_provider.hpp"
#include "npc/mouth_worker/signal_adapter.hpp"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace {

using namespace npc::mouth;

constexpr std::string_view detector_sha256 =
    "0e8e4806766d85ab067a52c7af0dcb59eb7f9dfe580b44f20a8e6ab712d89809";
constexpr std::string_view landmark_sha256 =
    "5bec42b298a24142cdb249a7256d65bc3fc0fbc673fa1752a64f4d7164719c9f";
constexpr std::string_view runtime_sha256 =
    "ea37f63d94a0f37405bf47eaf9c2287cd84b8084bfc37f682d07c4bc305105ed";
constexpr std::string_view runtime_shared_sha256 =
    "6da7afec6c88cf51572c1d0ce60cd97c865b5f348c2431b0733ce48cd03c201c";

[[nodiscard]] Nanoseconds monotonic_ns() {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

[[nodiscard]] std::string ppm_token(std::istream& stream) {
    std::string token;
    for (;;) {
        stream >> std::ws;
        if (stream.peek() != '#') break;
        stream.ignore(std::numeric_limits<std::streamsize>::max(), '\n');
    }
    stream >> token;
    if (!stream) throw std::runtime_error("malformed PPM header");
    return token;
}

[[nodiscard]] CpuFrame read_ppm(const std::filesystem::path& path) {
    std::ifstream stream(path, std::ios::binary);
    if (!stream || ppm_token(stream) != "P6") throw std::runtime_error("input must be P6 PPM");
    const auto width = static_cast<std::uint32_t>(std::stoul(ppm_token(stream)));
    const auto height = static_cast<std::uint32_t>(std::stoul(ppm_token(stream)));
    if (ppm_token(stream) != "255" || width == 0U || height == 0U ||
        width > 4096U || height > 4096U) {
        throw std::runtime_error("PPM dimensions/range are unsupported");
    }
    stream.get();
    std::vector<std::uint8_t> rgb(static_cast<std::size_t>(width) * height * 3U);
    stream.read(reinterpret_cast<char*>(rgb.data()), static_cast<std::streamsize>(rgb.size()));
    if (!stream) throw std::runtime_error("PPM pixel payload is truncated");
    CpuFrame frame{};
    frame.lease.width = width;
    frame.lease.height = height;
    frame.lease.stride_bytes = width * 4U;
    frame.lease.lease_nonce_high = 0x4c414e444d41524bULL;
    frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) * height);
    for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(width) * height; ++pixel) {
        frame.bgra[pixel * 4U + 0U] = rgb[pixel * 3U + 2U];
        frame.bgra[pixel * 4U + 1U] = rgb[pixel * 3U + 1U];
        frame.bgra[pixel * 4U + 2U] = rgb[pixel * 3U + 0U];
        frame.bgra[pixel * 4U + 3U] = 255U;
    }
    return frame;
}

[[nodiscard]] std::vector<std::filesystem::path> ppm_paths(
    const std::filesystem::path& input) {
    if (!std::filesystem::is_directory(input)) return {input};
    std::vector<std::filesystem::path> paths;
    for (const auto& entry : std::filesystem::directory_iterator(input)) {
        if (entry.is_regular_file() && entry.path().extension() == ".ppm") {
            paths.push_back(entry.path());
        }
    }
    std::sort(paths.begin(), paths.end());
    if (paths.empty()) throw std::runtime_error("input directory contains no PPM files");
    return paths;
}

[[nodiscard]] std::string json_escape(const std::string_view value) {
    std::string output;
    for (const char character : value) {
        if (character == '\\' || character == '"') output.push_back('\\');
        if (character == '\n') output += "\\n";
        else if (character != '\r') output.push_back(character);
    }
    return output;
}

[[nodiscard]] NormalizedRect expanded_tracking_seed(
    const NormalizedRect& detected_face) noexcept {
    constexpr double horizontal_margin_fraction = 0.40;
    constexpr double top_margin_fraction = 0.35;
    constexpr double bottom_margin_fraction = 0.45;
    const double left = std::clamp(
        detected_face.x - detected_face.width * horizontal_margin_fraction, 0.0, 1.0);
    const double top = std::clamp(
        detected_face.y - detected_face.height * top_margin_fraction, 0.0, 1.0);
    const double right = std::clamp(
        detected_face.right() + detected_face.width * horizontal_margin_fraction, 0.0, 1.0);
    const double bottom = std::clamp(
        detected_face.bottom() + detected_face.height * bottom_margin_fraction, 0.0, 1.0);
    return {left, top, right - left, bottom - top};
}

[[nodiscard]] AdmittedLandmarkProviderLaunchV1 launch_for(
    const std::filesystem::path& root) {
    AdmittedLandmarkProviderLaunchV1 launch{};
    launch.pack_id = std::string(admitted_openseeface_pack_id_v1);
    launch.pack_revision = std::string(admitted_openseeface_revision_v1);
    launch.artifact_root = std::filesystem::canonical(root);
    launch.detector_model = launch.artifact_root / "models/mnv3_detection_opt.onnx";
    launch.landmark_model = launch.artifact_root / "models/lm_model1_opt.onnx";
    launch.runtime_library =
        launch.artifact_root / "runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime.dll";
    launch.runtime_shared_library = launch.artifact_root /
        "runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime_providers_shared.dll";
    launch.detector_size_bytes = std::filesystem::file_size(launch.detector_model);
    launch.landmark_size_bytes = std::filesystem::file_size(launch.landmark_model);
    launch.runtime_size_bytes = std::filesystem::file_size(launch.runtime_library);
    launch.runtime_shared_size_bytes = std::filesystem::file_size(launch.runtime_shared_library);
    launch.detector_sha256 = detector_sha256;
    launch.landmark_sha256 = landmark_sha256;
    launch.runtime_sha256 = runtime_sha256;
    launch.runtime_shared_sha256 = runtime_shared_sha256;
    launch.measured_envelope_sha256 = runtime_sha256;
    launch.runtime_revision = std::string(admitted_openseeface_runtime_revision_v1);
    launch.backend = std::string(admitted_openseeface_backend_v1);
    launch.maximum_signal_rate_hz = 15U;
    launch.inference_threads = 1U;
    launch.exact_target_process_id = GetCurrentProcessId();
    return launch;
}

void write_rect(std::ostream& stream, const NormalizedRect& value) {
    stream << '[' << value.x << ',' << value.y << ',' << value.width << ',' << value.height << ']';
}

void write_point(std::ostream& stream, const NormalizedLandmark& value) {
    stream << '[' << value.x << ',' << value.y << ',' << value.confidence << ']';
}

[[nodiscard]] bool finite_landmark(const NormalizedLandmark& point) noexcept {
    return std::isfinite(point.x) && std::isfinite(point.y) &&
           std::isfinite(point.confidence) && point.x >= 0.0 && point.x <= 1.0 &&
           point.y >= 0.0 && point.y <= 1.0 && point.confidence >= 0.0 &&
           point.confidence <= 1.0;
}

[[nodiscard]] std::optional<NormalizedRect> enrollment_mouth_bounds(
    const OpenSeeFaceLandmarkPacketV1& packet) noexcept {
    double left = 1.0;
    double top = 1.0;
    double right = 0.0;
    double bottom = 0.0;
    for (std::size_t index = 48U; index < 66U; ++index) {
        if (!finite_landmark(packet.landmarks[index])) return std::nullopt;
        left = std::min(left, packet.landmarks[index].x);
        top = std::min(top, packet.landmarks[index].y);
        right = std::max(right, packet.landmarks[index].x);
        bottom = std::max(bottom, packet.landmarks[index].y);
    }
    const double width = right - left;
    const double height = bottom - top;
    if (!(width > 0.0) || !(height > 0.0)) return std::nullopt;
    const double horizontal_padding = width * 0.34;
    const double vertical_padding = std::max(height * 0.70, width * 0.22);
    const double padded_left = std::max(0.0, left - horizontal_padding);
    const double padded_top = std::max(0.0, top - vertical_padding);
    const double padded_right = std::min(1.0, right + horizontal_padding);
    const double padded_bottom = std::min(1.0, bottom + vertical_padding);
    return NormalizedRect{padded_left, padded_top, padded_right - padded_left,
                          padded_bottom - padded_top};
}

} // namespace

int main(int argc, char** argv) {
    try {
        if (argc != 4) {
            throw std::runtime_error(
                "usage: npc_mouth_worker_landmark_dump <pack-root> <ppm|ppm-directory> <output.json>");
        }
        const auto pack_root = std::filesystem::path(argv[1]);
        const auto input = std::filesystem::path(argv[2]);
        const auto output = std::filesystem::path(argv[3]);
        if (!output.parent_path().empty()) std::filesystem::create_directories(output.parent_path());

        constexpr std::uint64_t generation = 1U;
        const TrackBinding track{generation, 0x41544c4153454e52ULL, 0x4d4f555448U, 1U};
        auto provider = make_windows_ort_landmark_provider_v1();
        if (!provider) throw std::runtime_error("Windows ORT provider is unavailable");
        std::string failure;
        if (!provider->load(launch_for(pack_root), generation, failure)) {
            throw std::runtime_error("provider load failed: " + failure);
        }

        AppearanceGateEvidenceV1 appearance{};
        appearance.runtime_actor_id = track.actor_id;
        appearance.descriptor_revision = 1U;
        appearance.expected_descriptor_digest_high = 11U;
        appearance.expected_descriptor_digest_low = 12U;
        appearance.observed_descriptor_digest_high = 11U;
        appearance.observed_descriptor_digest_low = 12U;
        appearance.similarity = 0.99;
        appearance.temporal_iou = 0.99;
        appearance.identity_locked = true;
        appearance.target_visible = true;
        VisualResourceStateV1 resources{};
        resources.admitted_signal_rate_hz = 15U;
        OpenSeeFaceSignalAdapter adapter(generation);

        const auto paths = ppm_paths(input);
        std::ofstream stream(output, std::ios::binary);
        if (!stream) throw std::runtime_error("could not create output JSON");
        stream << std::fixed << std::setprecision(8);
        stream << "{\n  \"schema\": \"interactive-npcs-landmark-dump/v1\",\n  \"frames\": [\n";
        std::optional<NormalizedRect> seed;
        std::size_t accepted{};
        Nanoseconds previous_inference_ns{};
        for (std::size_t index = 0; index < paths.size(); ++index) {
            const auto now = monotonic_ns();
            if (previous_inference_ns > 0 && now - previous_inference_ns < 67'000'000LL) {
                std::this_thread::sleep_for(
                    std::chrono::nanoseconds(67'000'000LL - (now - previous_inference_ns)));
            }
            const auto inference_now = monotonic_ns();
            auto frame = read_ppm(paths[index]);
            frame.identity = {index + 1U, 1U, 1U, inference_now};
            frame.lease.lease_nonce_low = index + 1U;
            frame.lease.expires_at_ns = inference_now + 2'000'000'000LL;
            LandmarkInferenceWorkV1 work{};
            work.track = track;
            work.frame = frame.identity;
            work.source_frame_qpc = static_cast<std::uint64_t>(inference_now);
            work.qpc_frequency = 1'000'000'000U;
            work.seed_face_bounds = seed.value_or(NormalizedRect{0.0, 0.0, 1.0, 0.88});
            work.source = std::move(frame);
            work.deadline_ns = inference_now + 2'000'000'000LL;
            auto packet = provider->infer(work, inference_now, failure);
            previous_inference_ns = monotonic_ns();

            stream << "    {\"file\":\"" << json_escape(paths[index].filename().string()) << "\"";
            if (!packet) {
                stream << ",\"accepted\":false,\"reason\":\"provider:" << json_escape(failure) << "\"}";
            } else {
                seed = expanded_tracking_seed(packet->face_bounds);
                const auto decision = adapter.adapt(
                    *packet, appearance, resources, packet->frame, packet->measured_at_ns + 1'000'000LL);
                const auto mouth_bounds = enrollment_mouth_bounds(*packet);
                const bool enrollment_accepted = mouth_bounds.has_value() &&
                    packet->detector_confidence >= 0.60 &&
                    packet->landmark_confidence >= 0.62 &&
                    packet->visibility_ratio >= 0.80;
                if (!enrollment_accepted) {
                    stream << ",\"accepted\":false,\"reason\":\"adapter:"
                           << to_string(decision.disposition) << "\",\"detectorConfidence\":"
                           << packet->detector_confidence << ",\"landmarkConfidence\":"
                           << packet->landmark_confidence << ",\"visibilityRatio\":"
                           << packet->visibility_ratio << ",\"rawFace\":";
                    write_rect(stream, packet->face_bounds);
                    stream << ",\"rawPose\":[" << packet->pose.yaw << ',' << packet->pose.pitch
                           << ',' << packet->pose.roll << "]}";
                } else {
                    ++accepted;
                    stream << ",\"accepted\":true,\"width\":" << work.source.lease.width
                           << ",\"height\":" << work.source.lease.height << ",\"face\":";
                    write_rect(stream, packet->face_bounds);
                    stream << ",\"mouth\":";
                    write_rect(stream, *mouth_bounds);
                    stream << ",\"runtimeDisposition\":\"" << to_string(decision.disposition)
                           << "\",\"detectorConfidence\":" << packet->detector_confidence
                           << ",\"landmarkConfidence\":" << packet->landmark_confidence
                           << ",\"visibilityRatio\":" << packet->visibility_ratio
                           << ",\"pose\":[" << packet->pose.yaw << ',' << packet->pose.pitch
                           << ',' << packet->pose.roll << "],\"contour\":[";
                    for (std::size_t point = 48U; point < 66U; ++point) {
                        if (point != 48U) stream << ',';
                        write_point(stream, packet->landmarks[point]);
                    }
                    stream << "]}";
                }
            }
            stream << (index + 1U == paths.size() ? "\n" : ",\n");
        }
        stream << "  ],\n  \"accepted\": " << accepted << ",\n  \"total\": " << paths.size() << "\n}\n";
        if (!stream) throw std::runtime_error("could not write output JSON");
        std::cout << output.string() << " accepted=" << accepted << '/' << paths.size() << '\n';
        return accepted == paths.size() ? 0 : 1;
    } catch (const std::exception& error) {
        std::cerr << "landmark dump error: " << error.what() << '\n';
        return 2;
    }
}
