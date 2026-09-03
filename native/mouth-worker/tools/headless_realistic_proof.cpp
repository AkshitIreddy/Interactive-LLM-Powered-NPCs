#include "npc/mouth_worker/compositor.hpp"
#include "npc/mouth_worker/landmark_provider.hpp"
#include "npc/mouth_worker/signal_adapter.hpp"
#include "npc/mouth_worker/worker.hpp"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <psapi.h>

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
#include <numeric>
#include <optional>
#include <span>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
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

struct WavPcm {
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::vector<float> samples;
};

[[nodiscard]] Nanoseconds monotonic_ns() {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

[[nodiscard]] std::uint16_t little_u16(const std::byte* bytes) {
    return static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(bytes[0])) |
           static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(bytes[1])) << 8U;
}

[[nodiscard]] std::uint32_t little_u32(const std::byte* bytes) {
    return static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[0])) |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[1])) << 8U |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[2])) << 16U |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[3])) << 24U;
}

[[nodiscard]] std::vector<std::byte> read_binary(const std::filesystem::path& path) {
    std::ifstream stream(path, std::ios::binary | std::ios::ate);
    if (!stream) throw std::runtime_error("could not open " + path.string());
    const auto end = stream.tellg();
    if (end <= 0) throw std::runtime_error("input is empty: " + path.string());
    std::vector<std::byte> bytes(static_cast<std::size_t>(end));
    stream.seekg(0);
    stream.read(reinterpret_cast<char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
    if (!stream) throw std::runtime_error("could not read " + path.string());
    return bytes;
}

[[nodiscard]] WavPcm read_wav_pcm16(const std::filesystem::path& path) {
    const auto bytes = read_binary(path);
    if (bytes.size() < 44U || std::string_view(reinterpret_cast<const char*>(bytes.data()), 4U) != "RIFF" ||
        std::string_view(reinterpret_cast<const char*>(bytes.data() + 8U), 4U) != "WAVE") {
        throw std::runtime_error("audio must be a RIFF/WAVE file");
    }
    std::uint16_t format{}, channels{}, bits{};
    std::uint32_t sample_rate{};
    std::span<const std::byte> pcm;
    for (std::size_t offset = 12U; offset + 8U <= bytes.size();) {
        const auto id = std::string_view(reinterpret_cast<const char*>(bytes.data() + offset), 4U);
        const auto size = static_cast<std::size_t>(little_u32(bytes.data() + offset + 4U));
        const auto payload = offset + 8U;
        if (payload + size > bytes.size()) throw std::runtime_error("WAV chunk exceeds file");
        if (id == "fmt " && size >= 16U) {
            format = little_u16(bytes.data() + payload);
            channels = little_u16(bytes.data() + payload + 2U);
            sample_rate = little_u32(bytes.data() + payload + 4U);
            bits = little_u16(bytes.data() + payload + 14U);
        } else if (id == "data") {
            pcm = std::span<const std::byte>(bytes.data() + payload, size);
        }
        offset = payload + size + (size & 1U);
    }
    if (format != 1U || channels == 0U || channels > 2U || sample_rate < 8'000U ||
        sample_rate > 192'000U || bits != 16U || pcm.empty() || pcm.size() % 2U != 0U) {
        throw std::runtime_error("headless proof requires mono/stereo PCM16 WAV");
    }
    WavPcm result{sample_rate, channels, {}};
    result.samples.reserve(pcm.size() / 2U);
    for (std::size_t index = 0; index < pcm.size(); index += 2U) {
        const auto value = static_cast<std::int16_t>(little_u16(pcm.data() + index));
        result.samples.push_back(static_cast<float>(value) / 32768.0F);
    }
    return result;
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
    if (ppm_token(stream) != "255" || width == 0U || height == 0U || width > 4096U || height > 4096U) {
        throw std::runtime_error("PPM dimensions/range are unsupported");
    }
    stream.get();
    const auto rgb_size = static_cast<std::size_t>(width) * height * 3U;
    std::vector<std::uint8_t> rgb(rgb_size);
    stream.read(reinterpret_cast<char*>(rgb.data()), static_cast<std::streamsize>(rgb.size()));
    if (!stream) throw std::runtime_error("PPM pixel payload is truncated");
    CpuFrame frame{};
    frame.lease.width = width;
    frame.lease.height = height;
    frame.lease.stride_bytes = width * 4U;
    frame.lease.lease_nonce_high = 0x484541444c455353ULL;
    frame.lease.lease_nonce_low = 0x5245414c00000001ULL;
    frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) * height);
    for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(width) * height; ++pixel) {
        frame.bgra[pixel * 4U + 0U] = rgb[pixel * 3U + 2U];
        frame.bgra[pixel * 4U + 1U] = rgb[pixel * 3U + 1U];
        frame.bgra[pixel * 4U + 2U] = rgb[pixel * 3U + 0U];
        frame.bgra[pixel * 4U + 3U] = 255U;
    }
    return frame;
}

void write_ppm(const std::filesystem::path& path, const std::vector<std::uint8_t>& bgra,
               const std::uint32_t width, const std::uint32_t height,
               const std::uint32_t stride) {
    std::ofstream stream(path, std::ios::binary);
    if (!stream) throw std::runtime_error("could not create " + path.string());
    stream << "P6\n" << width << ' ' << height << "\n255\n";
    for (std::uint32_t y = 0; y < height; ++y) {
        for (std::uint32_t x = 0; x < width; ++x) {
            const auto at = static_cast<std::size_t>(y) * stride + static_cast<std::size_t>(x) * 4U;
            const char rgb[]{static_cast<char>(bgra[at + 2U]), static_cast<char>(bgra[at + 1U]),
                             static_cast<char>(bgra[at + 0U])};
            stream.write(rgb, 3);
        }
    }
}

[[nodiscard]] std::uint64_t digest(const std::vector<std::uint8_t>& bytes) {
    std::uint64_t value = 1469598103934665603ULL;
    for (const auto byte : bytes) value = (value ^ byte) * 1099511628211ULL;
    return value;
}

[[nodiscard]] std::uint64_t private_bytes() {
    PROCESS_MEMORY_COUNTERS_EX counters{};
    counters.cb = sizeof(counters);
    return GetProcessMemoryInfo(GetCurrentProcess(),
                                reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&counters),
                                sizeof(counters))
        ? static_cast<std::uint64_t>(counters.PrivateUsage)
        : 0U;
}

[[nodiscard]] double percentile(std::vector<double> values, const double quantile) {
    if (values.empty()) return 0.0;
    std::sort(values.begin(), values.end());
    const auto position = quantile * static_cast<double>(values.size() - 1U);
    const auto lower = static_cast<std::size_t>(std::floor(position));
    const auto upper = static_cast<std::size_t>(std::ceil(position));
    const auto weight = position - static_cast<double>(lower);
    return values[lower] * (1.0 - weight) + values[upper] * weight;
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

[[nodiscard]] AdmittedLandmarkProviderLaunchV1 launch_for(const std::filesystem::path& root) {
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

} // namespace

int main(int argc, char** argv) {
    try {
        if (argc != 5) {
            throw std::runtime_error(
                "usage: npc_mouth_worker_headless_realistic_proof <pack-root> <portrait.ppm> <audio.wav> <output-dir>");
        }
        const auto pack_root = std::filesystem::path(argv[1]);
        const auto portrait_path = std::filesystem::path(argv[2]);
        const auto audio_path = std::filesystem::path(argv[3]);
        const auto output = std::filesystem::path(argv[4]);
        const auto frames_dir = output / "frames";
        std::filesystem::create_directories(frames_dir);

        auto source_template = read_ppm(portrait_path);
        const auto audio = read_wav_pcm16(audio_path);
        constexpr std::uint64_t generation = 1U;
        const TrackBinding track{generation, 0x4d415241U, 0x56454e4eU, 1U};
        auto provider = make_windows_ort_landmark_provider_v1();
        if (!provider) throw std::runtime_error("Windows ORT provider is unavailable");
        const auto launch = launch_for(pack_root);
        std::string failure;
        const auto load_started = std::chrono::steady_clock::now();
        if (!provider->load(launch, generation, failure)) {
            throw std::runtime_error("provider load failed: " + failure);
        }
        const auto load_ms = std::chrono::duration<double, std::milli>(
                                 std::chrono::steady_clock::now() - load_started)
                                 .count();

        constexpr std::size_t warmups = 3U;
        constexpr std::size_t measured_samples = 20U;
        std::vector<double> inference_ms;
        std::optional<OpenSeeFaceLandmarkPacketV1> packet;
        for (std::size_t sample = 0; sample < warmups + measured_samples; ++sample) {
            const auto now = monotonic_ns();
            auto source = source_template;
            source.identity = {sample + 1U, 1U, 1U, now};
            source.lease.expires_at_ns = now + 2'000'000'000LL;
            LandmarkInferenceWorkV1 work{};
            work.track = track;
            work.frame = source.identity;
            work.source_frame_qpc = static_cast<std::uint64_t>(now);
            work.qpc_frequency = 1'000'000'000U;
            work.seed_face_bounds = {0.20, 0.06, 0.62, 0.68};
            work.source = std::move(source);
            work.deadline_ns = now + 2'000'000'000LL;
            const auto started = std::chrono::steady_clock::now();
            auto candidate = provider->infer(work, now, failure);
            const auto elapsed = std::chrono::duration<double, std::milli>(
                                     std::chrono::steady_clock::now() - started)
                                     .count();
            if (!candidate) throw std::runtime_error("provider inference failed: " + failure);
            if (sample >= warmups) inference_ms.push_back(elapsed);
            packet = std::move(candidate);
        }
        if (!packet) throw std::runtime_error("provider produced no landmark packet");

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
        OpenSeeFaceSignalAdapter adapter(generation);
        const auto adapted_at = packet->measured_at_ns + 1'000'000LL;
        const auto decision = adapter.adapt(*packet, appearance, resources, packet->frame, adapted_at);
        if (!decision.accepted()) {
            std::ostringstream detail;
            detail << "real landmark adapter bypassed: " << to_string(decision.disposition)
                   << " detector=" << packet->detector_confidence
                   << " landmark=" << packet->landmark_confidence
                   << " visibility=" << packet->visibility_ratio
                   << " face=" << packet->face_bounds.x << ',' << packet->face_bounds.y << ','
                   << packet->face_bounds.width << ',' << packet->face_bounds.height
                   << " pose=" << packet->pose.yaw << ',' << packet->pose.pitch << ','
                   << packet->pose.roll;
            throw std::runtime_error(detail.str());
        }
        const auto tracking_template = *decision.tracking;

        constexpr std::uint32_t video_fps = 30U;
        const auto audio_frames = audio.samples.size() / audio.channels;
        const auto output_frames = std::max<std::size_t>(1U,
            (audio_frames * video_fps + audio.sample_rate - 1U) / audio.sample_rate);
        const Nanoseconds frame_ns = 1'000'000'000LL / video_fps;
        const auto timeline = monotonic_ns();
        ReferenceMouthWorker worker(generation);
        std::vector<double> compositor_ms;
        std::vector<double> mouth_mean_absolute_delta;
        std::vector<std::uint64_t> frame_digests;
        std::size_t residual_frames{};
        std::size_t changed_frames{};
        std::uint64_t previous_digest{};
        for (std::size_t frame_index = 0; frame_index < output_frames; ++frame_index) {
            const auto frame_at = timeline + static_cast<Nanoseconds>(frame_index) * frame_ns;
            auto source = source_template;
            source.identity = {1'000U + frame_index, 1U, 1U, frame_at};
            source.lease.lease_nonce_low = frame_index + 1U;
            source.lease.expires_at_ns = frame_at + 500'000'000LL;
            auto tracking = tracking_template;
            tracking.frame = source.identity;
            tracking.measured_at_ns = frame_at;
            const auto first = frame_index * audio.sample_rate / video_fps;
            const auto last = std::min(audio_frames,
                (frame_index + 1U) * audio.sample_rate / video_fps);
            MouthDrive drive{};
            drive.kind = DriveKind::pcm_window;
            drive.clock = {generation, 1U, first, audio.sample_rate, audio.channels, frame_at};
            drive.interleaved_pcm.assign(
                audio.samples.begin() + static_cast<std::ptrdiff_t>(first * audio.channels),
                audio.samples.begin() + static_cast<std::ptrdiff_t>(last * audio.channels));
            WorkItem item{track, source, tracking, std::move(drive), frame_at + 100'000'000LL};
            if (!worker.submit(std::move(item))) throw std::runtime_error("worker rejected frame");
            const auto started = std::chrono::steady_clock::now();
            const auto result = worker.process_latest(source.identity, frame_at + 2'000'000LL);
            if (!result.has_residual()) {
                throw std::runtime_error("worker bypassed rendered frame: " +
                                         std::string(to_string(result.disposition)));
            }
            const auto composited = composite_over_source(source, result.residual);
            compositor_ms.push_back(std::chrono::duration<double, std::milli>(
                                        std::chrono::steady_clock::now() - started)
                                        .count());
            const auto patch_left = static_cast<std::uint32_t>(std::llround(
                result.residual.normalized_bounds.x * source.lease.width));
            const auto patch_top = static_cast<std::uint32_t>(std::llround(
                result.residual.normalized_bounds.y * source.lease.height));
            double absolute_delta{};
            std::size_t delta_samples{};
            for (std::uint32_t patch_y = 0; patch_y < result.residual.height; ++patch_y) {
                for (std::uint32_t patch_x = 0; patch_x < result.residual.width; ++patch_x) {
                    const auto source_offset =
                        static_cast<std::size_t>(patch_top + patch_y) * source.lease.stride_bytes +
                        static_cast<std::size_t>(patch_left + patch_x) * 4U;
                    for (std::size_t channel = 0; channel < 3U; ++channel) {
                        absolute_delta += std::abs(
                            static_cast<double>(composited[source_offset + channel]) -
                            static_cast<double>(source.bgra[source_offset + channel]));
                        ++delta_samples;
                    }
                }
            }
            mouth_mean_absolute_delta.push_back(
                delta_samples == 0U ? 0.0 : absolute_delta / static_cast<double>(delta_samples));
            const auto current_digest = digest(composited);
            frame_digests.push_back(current_digest);
            if (frame_index > 0U && current_digest != previous_digest) ++changed_frames;
            previous_digest = current_digest;
            ++residual_frames;
            std::ostringstream name;
            name << "frame-" << std::setfill('0') << std::setw(5) << frame_index << ".ppm";
            write_ppm(frames_dir / name.str(), composited, source.lease.width,
                      source.lease.height, source.lease.stride_bytes);
        }
        write_ppm(output / "source.ppm", source_template.bgra, source_template.lease.width,
                  source_template.lease.height, source_template.lease.stride_bytes);

        const auto mean_inference = std::accumulate(inference_ms.begin(), inference_ms.end(), 0.0) /
                                    static_cast<double>(inference_ms.size());
        const auto p50_inference = percentile(inference_ms, 0.50);
        const auto p95_inference = percentile(inference_ms, 0.95);
        const auto p95_compositor = percentile(compositor_ms, 0.95);
        std::vector<std::uint64_t> distinct = frame_digests;
        std::sort(distinct.begin(), distinct.end());
        distinct.erase(std::unique(distinct.begin(), distinct.end()), distinct.end());
        const auto [minimum_mouth_delta, maximum_mouth_delta] =
            std::minmax_element(mouth_mean_absolute_delta.begin(),
                                mouth_mean_absolute_delta.end());
        const auto visibly_changed_frames = static_cast<std::size_t>(std::count_if(
            mouth_mean_absolute_delta.begin(), mouth_mean_absolute_delta.end(),
            [](const double value) { return value >= 1.0; }));
        const bool qualifies = p95_inference <= 220.0 && p95_compositor <= 8.0 &&
                               residual_frames == output_frames && distinct.size() >= 3U &&
                               changed_frames > output_frames / 4U &&
                               *maximum_mouth_delta >= 2.0 && *minimum_mouth_delta <= 0.25 &&
                               visibly_changed_frames >= output_frames / 6U;

        std::ofstream report(output / "headless-proof.json");
        if (!report) throw std::runtime_error("could not create proof report");
        report << std::fixed << std::setprecision(3)
               << "{\n  \"schema\": \"interactive-npcs-headless-realistic-lipsync/v2\",\n"
               << "  \"status\": \"" << (qualifies ? "passed" : "failed") << "\",\n"
               << "  \"portrait\": \"" << json_escape(portrait_path.string()) << "\",\n"
               << "  \"audio\": \"" << json_escape(audio_path.string()) << "\",\n"
               << "  \"model\": \"OpenSeeFace MNV3 + LM1 / ONNX Runtime 1.22.1 CPU\",\n"
               << "  \"width\": " << source_template.lease.width << ",\n"
               << "  \"height\": " << source_template.lease.height << ",\n"
               << "  \"audioSampleRate\": " << audio.sample_rate << ",\n"
               << "  \"audioChannels\": " << audio.channels << ",\n"
               << "  \"outputFps\": " << video_fps << ",\n"
               << "  \"outputFrames\": " << output_frames << ",\n"
               << "  \"residualFrames\": " << residual_frames << ",\n"
               << "  \"changedAdjacentFrames\": " << changed_frames << ",\n"
               << "  \"distinctFrameDigests\": " << distinct.size() << ",\n"
               << "  \"visiblyChangedFrames\": " << visibly_changed_frames << ",\n"
               << "  \"minimumMouthMeanAbsoluteDelta\": " << *minimum_mouth_delta << ",\n"
               << "  \"maximumMouthMeanAbsoluteDelta\": " << *maximum_mouth_delta << ",\n"
               << "  \"providerLoadMs\": " << load_ms << ",\n"
               << "  \"inferenceSamples\": " << inference_ms.size() << ",\n"
               << "  \"inferenceMeanMs\": " << mean_inference << ",\n"
               << "  \"inferenceP50Ms\": " << p50_inference << ",\n"
               << "  \"inferenceP95Ms\": " << p95_inference << ",\n"
               << "  \"compositorP95Ms\": " << p95_compositor << ",\n"
               << "  \"privateBytes\": " << private_bytes() << ",\n"
               << "  \"gpuVramBytes\": 0,\n"
               << "  \"landmarkConfidence\": " << packet->landmark_confidence << ",\n"
               << "  \"detectorConfidence\": " << packet->detector_confidence << ",\n"
               << "  \"visibilityRatio\": " << packet->visibility_ratio << "\n}\n";
        report.close();
        std::cout << (qualifies ? "PASS" : "FAIL")
                  << ": real OpenSeeFace landmarks + API WAV rendered headlessly\n"
                  << "inference_p50_ms=" << p50_inference
                  << " inference_p95_ms=" << p95_inference
                  << " compositor_p95_ms=" << p95_compositor << '\n'
                  << "frames=" << output_frames << " residuals=" << residual_frames
                  << " changed=" << changed_frames << " distinct=" << distinct.size()
                  << " visibly_changed=" << visibly_changed_frames
                  << " mouth_delta_min=" << *minimum_mouth_delta
                  << " mouth_delta_max=" << *maximum_mouth_delta << '\n'
                  << "output=" << output.string() << '\n';
        return qualifies ? 0 : 2;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
