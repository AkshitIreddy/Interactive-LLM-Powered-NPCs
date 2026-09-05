#include "npc/mouth_worker/compositor.hpp"
#include "npc/mouth_worker/landmark_provider.hpp"
#include "npc/mouth_worker/signal_adapter.hpp"
#include "npc/mouth_worker/worker.hpp"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <psapi.h>

#include <algorithm>
#include <array>
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

[[nodiscard]] CharacterMouthAtlas read_review_atlas(
    const std::filesystem::path& root,
    const std::uint64_t generation,
    const TrackBinding& track) {
    constexpr std::uint32_t width = 206U;
    constexpr std::uint32_t height = 143U;
    constexpr std::uint32_t stride = width * 4U;
    constexpr std::size_t state_bytes = static_cast<std::size_t>(stride) * height;
    constexpr std::array visemes{
        Viseme::silence,
        Viseme::labiodental,
        Viseme::rounded,
        Viseme::dental,
        Viseme::open_vowel,
        Viseme::alveolar,
        Viseme::spread_vowel,
        Viseme::postalveolar,
    };
    const auto texture_path = root / "atlas-bgra8-premultiplied.bin";
    const auto bytes = read_binary(texture_path);
    if (bytes.size() != state_bytes * visemes.size()) {
        throw std::runtime_error("review atlas has an unexpected state layout");
    }
    CharacterMouthAtlas atlas{};
    atlas.cancellation_generation = generation;
    atlas.actor_id = track.actor_id;
    atlas.identity_revision = 1U;
    atlas.states.reserve(visemes.size());
    for (std::size_t index = 0U; index < visemes.size(); ++index) {
        MouthAtlasState state{};
        state.coefficients = coefficients_for_viseme(visemes[index]);
        state.appearance.width = width;
        state.appearance.height = height;
        state.appearance.stride_bytes = stride;
        const auto first = bytes.begin() + static_cast<std::ptrdiff_t>(index * state_bytes);
        const auto last = first + static_cast<std::ptrdiff_t>(state_bytes);
        state.appearance.premultiplied_bgra.reserve(state_bytes);
        std::transform(first, last,
                       std::back_inserter(state.appearance.premultiplied_bgra),
                       [](const std::byte value) {
                           return std::to_integer<std::uint8_t>(value);
                       });
        atlas.states.push_back(std::move(state));
    }
    return atlas;
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

[[nodiscard]] std::vector<CpuFrame> read_source_frames(
    const std::filesystem::path& source_path) {
    if (!std::filesystem::is_directory(source_path)) {
        return {read_ppm(source_path)};
    }
    std::vector<std::filesystem::path> paths;
    for (const auto& entry : std::filesystem::directory_iterator(source_path)) {
        if (entry.is_regular_file() && entry.path().extension() == ".ppm") {
            paths.push_back(entry.path());
        }
    }
    std::sort(paths.begin(), paths.end());
    if (paths.size() < 2U) {
        throw std::runtime_error("moving source directory requires at least two PPM frames");
    }
    std::vector<CpuFrame> frames;
    frames.reserve(paths.size());
    for (const auto& path : paths) {
        auto frame = read_ppm(path);
        if (!frames.empty() &&
            (frame.lease.width != frames.front().lease.width ||
             frame.lease.height != frames.front().lease.height ||
             frame.lease.stride_bytes != frames.front().lease.stride_bytes)) {
            throw std::runtime_error("moving source frames must have identical geometry");
        }
        frames.push_back(std::move(frame));
    }
    return frames;
}

[[nodiscard]] NormalizedRect expanded_tracking_seed(
    const NormalizedRect& detected_face) noexcept {
    // The product path receives an identity-tracked face ROI rather than
    // searching most of the source frame after every acquisition. Keep enough
    // context for ordinary idle/head motion while preventing the offline proof
    // from benchmarking a deliberately broad cold-search rectangle forever.
    // MNV3 loses confidence when the detected face is enlarged to nearly fill
    // its 224 px detector input. Retain roughly one face-width of scene
    // context while still cutting the acquired search area substantially.
    constexpr double horizontal_margin_fraction = 1.05;
    constexpr double top_margin_fraction = 0.90;
    constexpr double bottom_margin_fraction = 1.10;
    const double left = std::clamp(
        detected_face.x - detected_face.width * horizontal_margin_fraction,
        0.0, 1.0);
    const double top = std::clamp(
        detected_face.y - detected_face.height * top_margin_fraction,
        0.0, 1.0);
    const double right = std::clamp(
        detected_face.x + detected_face.width * (1.0 + horizontal_margin_fraction),
        0.0, 1.0);
    const double bottom = std::clamp(
        detected_face.y + detected_face.height * (1.0 + bottom_margin_fraction),
        0.0, 1.0);
    return {left, top, right - left, bottom - top};
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
        if (argc < 5 || argc > 7) {
            throw std::runtime_error(
                "usage: npc_mouth_worker_headless_realistic_proof <pack-root> <portrait.ppm|source-frames-dir> <audio.wav> <output-dir> [tracking-hz:10|15] [review-mouth-atlas-root]");
        }
        const auto pack_root = std::filesystem::path(argv[1]);
        const auto source_path = std::filesystem::path(argv[2]);
        const auto audio_path = std::filesystem::path(argv[3]);
        const auto output = std::filesystem::path(argv[4]);
        std::uint32_t tracking_rate_hz = 15U;
        if (argc >= 6) {
            const auto text = std::string_view(argv[5]);
            const auto parsed = std::from_chars(
                text.data(), text.data() + text.size(), tracking_rate_hz);
            if (parsed.ec != std::errc{} || parsed.ptr != text.data() + text.size() ||
                (tracking_rate_hz != 10U && tracking_rate_hz != 15U)) {
                throw std::runtime_error("tracking-hz must be 10 or 15");
            }
        }
        const std::optional<std::filesystem::path> atlas_root = argc == 7
            ? std::optional<std::filesystem::path>(std::filesystem::path(argv[6]))
            : std::nullopt;
        const auto frames_dir = output / "frames";
        std::filesystem::create_directories(frames_dir);

        auto source_frames = read_source_frames(source_path);
        const bool moving_source = source_frames.size() > 1U;
        const auto& source_template = source_frames.front();
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
            work.seed_face_bounds = moving_source
                ? NormalizedRect{0.12, 0.08, 0.76, 0.88}
                : NormalizedRect{0.20, 0.06, 0.62, 0.68};
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
        resources.admitted_signal_rate_hz = tracking_rate_hz;
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
        const std::size_t tracking_interval_frames = video_fps / tracking_rate_hz;
        const auto audio_frames = audio.samples.size() / audio.channels;
        const auto output_frames = std::max<std::size_t>(1U,
            (audio_frames * video_fps + audio.sample_rate - 1U) / audio.sample_rate);
        const Nanoseconds frame_ns = 1'000'000'000LL / video_fps;
        const auto timeline = monotonic_ns();
        ReferenceMouthWorker worker(generation);
        if (atlas_root.has_value() &&
            !worker.install_atlas(read_review_atlas(*atlas_root, generation, track))) {
            throw std::runtime_error("review mouth atlas failed native admission");
        }
        std::vector<double> compositor_ms;
        std::vector<double> moving_inference_ms;
        std::vector<double> mouth_mean_absolute_delta;
        std::vector<double> upper_lip_darkened_fraction;
        std::vector<double> articulated_rows_over_mouth_width;
        std::vector<std::uint64_t> frame_digests;
        std::size_t residual_frames{};
        std::size_t changed_frames{};
        std::size_t changed_source_frames{};
        std::uint64_t previous_digest{};
        std::uint64_t previous_source_digest{};
        std::optional<OpenSeeFaceLandmarkPacketV1> final_packet = packet;
        std::optional<TrackingEvidence> carried_moving_tracking;
        NormalizedRect moving_seed_face = expanded_tracking_seed(packet->face_bounds);
        Nanoseconds last_moving_inference_at_ns = moving_source ? packet->measured_at_ns : 0;
        for (std::size_t frame_index = 0; frame_index < output_frames; ++frame_index) {
            const auto frame_at = timeline + static_cast<Nanoseconds>(frame_index) * frame_ns;
            auto source = source_frames[frame_index % source_frames.size()];
            source.identity = {1'000U + frame_index, 1U, 1U, frame_at};
            source.lease.lease_nonce_low = frame_index + 1U;
            source.lease.expires_at_ns = frame_at + 500'000'000LL;
            auto tracking = tracking_template;
            if (moving_source && frame_index % tracking_interval_frames == 0U) {
                const Nanoseconds admitted_tracking_period_ns =
                    (1'000'000'000LL + tracking_rate_hz - 1U) / tracking_rate_hz;
                if (last_moving_inference_at_ns > 0) {
                    const auto wait_ns = last_moving_inference_at_ns +
                        admitted_tracking_period_ns - monotonic_ns();
                    if (wait_ns > 0) {
                        std::this_thread::sleep_for(std::chrono::nanoseconds(wait_ns));
                    }
                }
                const auto inference_now = monotonic_ns();
                auto inference_source = source;
                inference_source.identity.captured_at_ns = inference_now;
                inference_source.lease.expires_at_ns = inference_now + 2'000'000'000LL;
                LandmarkInferenceWorkV1 work{};
                work.track = track;
                work.frame = inference_source.identity;
                work.source_frame_qpc = static_cast<std::uint64_t>(inference_now);
                work.qpc_frequency = 1'000'000'000U;
                work.seed_face_bounds = moving_seed_face;
                work.source = std::move(inference_source);
                work.deadline_ns = inference_now + 2'000'000'000LL;
                const auto inference_started = std::chrono::steady_clock::now();
                auto moving_packet = provider->infer(work, inference_now, failure);
                moving_inference_ms.push_back(std::chrono::duration<double, std::milli>(
                    std::chrono::steady_clock::now() - inference_started).count());
                if (!moving_packet) {
                    throw std::runtime_error("moving provider inference failed: " + failure);
                }
                const auto moving_decision = adapter.adapt(
                    *moving_packet, appearance, resources, moving_packet->frame,
                    moving_packet->measured_at_ns + 1'000'000LL);
                if (!moving_decision.accepted()) {
                    std::ostringstream detail;
                    detail << "moving landmark adapter bypassed: "
                           << to_string(moving_decision.disposition)
                           << " detector=" << moving_packet->detector_confidence
                           << " landmarks=" << moving_packet->landmark_confidence
                           << " visibility=" << moving_packet->visibility_ratio
                           << " face=[" << moving_packet->face_bounds.x << ','
                           << moving_packet->face_bounds.y << ','
                           << moving_packet->face_bounds.width << ','
                           << moving_packet->face_bounds.height << ']';
                    throw std::runtime_error(detail.str());
                }
                tracking = *moving_decision.tracking;
                // The provider is measured on the real offline execution
                // clock; bind the accepted geometry to the deterministic
                // output timeline before worker freshness validation.
                tracking.frame = source.identity;
                tracking.measured_at_ns = frame_at;
                carried_moving_tracking = tracking;
                last_moving_inference_at_ns = inference_now;
                moving_seed_face = expanded_tracking_seed(moving_packet->face_bounds);
                final_packet = std::move(moving_packet);
            } else if (moving_source) {
                tracking = carried_moving_tracking.value_or(tracking_template);
                tracking.frame = source.identity;
                tracking.measured_at_ns = frame_at;
            } else {
                tracking.frame = source.identity;
                tracking.measured_at_ns = frame_at;
            }
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

            const double source_width = static_cast<double>(source.lease.width);
            const double source_height = static_cast<double>(source.lease.height);
            const double left_corner_x =
                tracking.mouth_landmarks.left_corner.x * source_width;
            const double right_corner_x =
                tracking.mouth_landmarks.right_corner.x * source_width;
            const auto articulation_left = static_cast<std::uint32_t>(std::clamp(
                std::floor(left_corner_x), 0.0, source_width - 1.0));
            const auto articulation_right = static_cast<std::uint32_t>(std::clamp(
                std::ceil(right_corner_x), 0.0, source_width - 1.0));
            std::size_t articulated_rows{};
            if (articulation_right > articulation_left) {
                const std::size_t articulation_width =
                    static_cast<std::size_t>(articulation_right - articulation_left + 1U);
                const std::size_t minimum_changed_pixels = std::max<std::size_t>(
                    1U, static_cast<std::size_t>(std::ceil(
                        static_cast<double>(articulation_width) * 0.10)));
                for (std::uint32_t source_y = patch_top;
                     source_y < patch_top + result.residual.height; ++source_y) {
                    std::size_t changed_pixels{};
                    for (std::uint32_t source_x = articulation_left;
                         source_x <= articulation_right; ++source_x) {
                        const auto offset = static_cast<std::size_t>(source_y) *
                                                source.lease.stride_bytes +
                                            static_cast<std::size_t>(source_x) * 4U;
                        const auto channel_delta =
                            std::abs(static_cast<int>(composited[offset + 0U]) -
                                     static_cast<int>(source.bgra[offset + 0U])) +
                            std::abs(static_cast<int>(composited[offset + 1U]) -
                                     static_cast<int>(source.bgra[offset + 1U])) +
                            std::abs(static_cast<int>(composited[offset + 2U]) -
                                     static_cast<int>(source.bgra[offset + 2U]));
                        changed_pixels += channel_delta > 12 ? 1U : 0U;
                    }
                    articulated_rows += changed_pixels >= minimum_changed_pixels ? 1U : 0U;
                }
                articulated_rows_over_mouth_width.push_back(
                    static_cast<double>(articulated_rows) /
                    static_cast<double>(articulation_width));
            } else {
                articulated_rows_over_mouth_width.push_back(0.0);
            }

            // A whole-patch delta can be made larger by corrupting the upper
            // lip, so guard the failure mode independently. In schema 2 the
            // protected band is the actual upper-lip surface between outer
            // points 49..51 and inner points 59..61, not the oral aperture.
            double protected_surface_top =
                tracking.mouth_landmarks.upper_lip_center.y * source_height;
            double protected_surface_bottom = protected_surface_top;
            if (tracking.mouth_landmarks.schema_version >= 2U &&
                tracking.mouth_landmarks.contour_points ==
                    tracking.mouth_landmarks.contour.size()) {
                const auto& contour = tracking.mouth_landmarks.contour;
                const double outer_upper =
                    (contour[1U].y + contour[2U].y + contour[3U].y) / 3.0 *
                    source_height;
                const double inner_upper =
                    (contour[11U].y + contour[12U].y + contour[13U].y) / 3.0 *
                    source_height;
                const double surface_span = std::max(1.0, inner_upper - outer_upper);
                protected_surface_top = outer_upper + surface_span * 0.18;
                protected_surface_bottom = inner_upper - surface_span * 0.18;
            } else {
                const double lower_lip_y =
                    tracking.mouth_landmarks.lower_lip_center.y * source_height;
                const double lip_span = std::max(
                    2.0, lower_lip_y - protected_surface_top);
                protected_surface_bottom = protected_surface_top + lip_span * 0.36;
            }
            const double mouth_center_x = (left_corner_x + right_corner_x) * 0.5;
            const double protected_half_width =
                std::abs(right_corner_x - left_corner_x) * 0.36;
            const auto protected_left = static_cast<std::uint32_t>(std::clamp(
                std::floor(mouth_center_x - protected_half_width), 0.0,
                source_width - 1.0));
            const auto protected_right = static_cast<std::uint32_t>(std::clamp(
                std::ceil(mouth_center_x + protected_half_width), 0.0,
                source_width - 1.0));
            const auto protected_top = static_cast<std::uint32_t>(std::clamp(
                std::floor(protected_surface_top), 0.0,
                source_height - 1.0));
            const auto protected_bottom = static_cast<std::uint32_t>(std::clamp(
                std::floor(std::max(protected_surface_top, protected_surface_bottom)), 0.0,
                source_height - 1.0));
            std::size_t darkened_upper_lip_pixels{};
            std::size_t protected_upper_lip_pixels{};
            if (protected_right >= protected_left && protected_bottom >= protected_top) {
                for (std::uint32_t protected_y = protected_top;
                     protected_y <= protected_bottom; ++protected_y) {
                    for (std::uint32_t protected_x = protected_left;
                         protected_x <= protected_right; ++protected_x) {
                        const auto offset =
                            static_cast<std::size_t>(protected_y) * source.lease.stride_bytes +
                            static_cast<std::size_t>(protected_x) * 4U;
                        const double source_luma =
                            static_cast<double>(source.bgra[offset + 2U]) * 0.299 +
                            static_cast<double>(source.bgra[offset + 1U]) * 0.587 +
                            static_cast<double>(source.bgra[offset + 0U]) * 0.114;
                        const double output_luma =
                            static_cast<double>(composited[offset + 2U]) * 0.299 +
                            static_cast<double>(composited[offset + 1U]) * 0.587 +
                            static_cast<double>(composited[offset + 0U]) * 0.114;
                        darkened_upper_lip_pixels +=
                            source_luma > output_luma + 10.0 ? 1U : 0U;
                        ++protected_upper_lip_pixels;
                    }
                }
            }
            upper_lip_darkened_fraction.push_back(
                protected_upper_lip_pixels == 0U ? 1.0 :
                static_cast<double>(darkened_upper_lip_pixels) /
                    static_cast<double>(protected_upper_lip_pixels));
            const auto current_digest = digest(composited);
            const auto current_source_digest = digest(source.bgra);
            frame_digests.push_back(current_digest);
            if (frame_index > 0U && current_digest != previous_digest) ++changed_frames;
            if (frame_index > 0U && current_source_digest != previous_source_digest) {
                ++changed_source_frames;
            }
            previous_digest = current_digest;
            previous_source_digest = current_source_digest;
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
        const auto moving_inference_p50 = moving_inference_ms.empty()
            ? 0.0 : percentile(moving_inference_ms, 0.50);
        const auto moving_inference_p95 = moving_inference_ms.empty()
            ? 0.0 : percentile(moving_inference_ms, 0.95);
        std::vector<std::uint64_t> distinct = frame_digests;
        std::sort(distinct.begin(), distinct.end());
        distinct.erase(std::unique(distinct.begin(), distinct.end()), distinct.end());
        const auto [minimum_mouth_delta, maximum_mouth_delta] =
            std::minmax_element(mouth_mean_absolute_delta.begin(),
                                mouth_mean_absolute_delta.end());
        const auto visibly_changed_frames = static_cast<std::size_t>(std::count_if(
            mouth_mean_absolute_delta.begin(), mouth_mean_absolute_delta.end(),
            [](const double value) { return value >= 1.0; }));
        const auto maximum_upper_lip_darkened_fraction = *std::max_element(
            upper_lip_darkened_fraction.begin(), upper_lip_darkened_fraction.end());
        const auto maximum_articulated_rows_over_mouth_width = *std::max_element(
            articulated_rows_over_mouth_width.begin(),
            articulated_rows_over_mouth_width.end());
        const bool source_motion_qualifies =
            !moving_source || changed_source_frames > output_frames / 4U;
        const bool moving_tracking_qualifies =
            !moving_source ||
            (moving_inference_ms.size() ==
                 (output_frames + tracking_interval_frames - 1U) /
                     tracking_interval_frames &&
             moving_inference_p95 <= 600.0 / static_cast<double>(tracking_rate_hz));
        const bool qualifies = p95_inference <= 220.0 && p95_compositor <= 8.0 &&
                               residual_frames == output_frames && distinct.size() >= 3U &&
                               changed_frames > output_frames / 4U &&
                               *maximum_mouth_delta >= 1.25 && *minimum_mouth_delta <= 0.25 &&
                               visibly_changed_frames >= output_frames / 6U &&
                               maximum_upper_lip_darkened_fraction <= 0.03 &&
                               maximum_articulated_rows_over_mouth_width >= 0.10 &&
                               source_motion_qualifies && moving_tracking_qualifies;

        std::ofstream report(output / "headless-proof.json");
        if (!report) throw std::runtime_error("could not create proof report");
        report << std::fixed << std::setprecision(3)
               << "{\n  \"schema\": \"interactive-npcs-headless-realistic-lipsync/v6\",\n"
               << "  \"status\": \"" << (qualifies ? "passed" : "failed") << "\",\n"
               << "  \"source\": \"" << json_escape(source_path.string()) << "\",\n"
               << "  \"movingSource\": " << (moving_source ? "true" : "false") << ",\n"
               << "  \"sourceFrameCount\": " << source_frames.size() << ",\n"
               << "  \"audio\": \"" << json_escape(audio_path.string()) << "\",\n"
               << "  \"reviewMouthAtlas\": "
               << (atlas_root.has_value()
                       ? "\"" + json_escape(atlas_root->string()) + "\""
                       : "null")
               << ",\n"
               << "  \"model\": \"OpenSeeFace MNV3 + LM1 / ONNX Runtime 1.22.1 CPU\",\n"
               << "  \"width\": " << source_template.lease.width << ",\n"
               << "  \"height\": " << source_template.lease.height << ",\n"
               << "  \"audioSampleRate\": " << audio.sample_rate << ",\n"
               << "  \"audioChannels\": " << audio.channels << ",\n"
               << "  \"outputFps\": " << video_fps << ",\n"
               << "  \"outputFrames\": " << output_frames << ",\n"
               << "  \"residualFrames\": " << residual_frames << ",\n"
               << "  \"changedAdjacentFrames\": " << changed_frames << ",\n"
               << "  \"changedAdjacentSourceFrames\": " << changed_source_frames << ",\n"
               << "  \"distinctFrameDigests\": " << distinct.size() << ",\n"
               << "  \"visiblyChangedFrames\": " << visibly_changed_frames << ",\n"
               << "  \"minimumMouthMeanAbsoluteDelta\": " << *minimum_mouth_delta << ",\n"
               << "  \"maximumMouthMeanAbsoluteDelta\": " << *maximum_mouth_delta << ",\n"
               << "  \"maximumUpperLipDarkenedFraction\": "
               << maximum_upper_lip_darkened_fraction << ",\n"
               << "  \"maximumArticulatedRowsOverMouthWidth\": "
               << maximum_articulated_rows_over_mouth_width << ",\n"
               << "  \"providerLoadMs\": " << load_ms << ",\n"
               << "  \"inferenceSamples\": " << inference_ms.size() << ",\n"
               << "  \"inferenceMeanMs\": " << mean_inference << ",\n"
               << "  \"inferenceP50Ms\": " << p50_inference << ",\n"
               << "  \"inferenceP95Ms\": " << p95_inference << ",\n"
               << "  \"movingInferenceSamples\": " << moving_inference_ms.size() << ",\n"
               << "  \"movingTrackingRateHz\": "
               << (moving_source ? tracking_rate_hz : 0U) << ",\n"
               << "  \"movingInferenceBudgetMs\": "
               << (moving_source ? 600.0 / static_cast<double>(tracking_rate_hz) : 0.0)
               << ",\n"
               << "  \"movingInferenceP50Ms\": " << moving_inference_p50 << ",\n"
               << "  \"movingInferenceP95Ms\": " << moving_inference_p95 << ",\n"
               << "  \"compositorP95Ms\": " << p95_compositor << ",\n"
               << "  \"privateBytes\": " << private_bytes() << ",\n"
               << "  \"gpuVramBytes\": 0,\n"
               << "  \"landmarkConfidence\": " << final_packet->landmark_confidence << ",\n"
               << "  \"detectorConfidence\": " << final_packet->detector_confidence << ",\n"
               << "  \"visibilityRatio\": " << final_packet->visibility_ratio << "\n}\n";
        report.close();
        std::cout << (qualifies ? "PASS" : "FAIL")
                  << ": real OpenSeeFace landmarks + API WAV rendered headlessly\n"
                  << "inference_p50_ms=" << p50_inference
                  << " inference_p95_ms=" << p95_inference
                  << " compositor_p95_ms=" << p95_compositor << '\n'
                  << "frames=" << output_frames << " residuals=" << residual_frames
                  << " changed=" << changed_frames << " distinct=" << distinct.size()
                  << " source_changed=" << changed_source_frames
                  << " visibly_changed=" << visibly_changed_frames
                  << " mouth_delta_min=" << *minimum_mouth_delta
                  << " mouth_delta_max=" << *maximum_mouth_delta
                  << " upper_lip_darkened_max="
                  << maximum_upper_lip_darkened_fraction
                  << " opening_rows_over_width_max="
                  << maximum_articulated_rows_over_mouth_width
                  << " moving_inference_p95_ms=" << moving_inference_p95 << '\n'
                  << "output=" << output.string() << '\n';
        return qualifies ? 0 : 2;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
