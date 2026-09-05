#pragma once

#include "npc/mouth_worker/compositor.hpp"
#include "npc/mouth_worker/landmark_provider.hpp"
#include "npc/mouth_worker/signal_adapter.hpp"
#include "npc/mouth_worker/worker.hpp"
#include "npc/mouth_worker/review_cues.hpp"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <psapi.h>
#include <bcrypt.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <charconv>
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
#include <regex>
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

[[nodiscard]] inline NormalizedRect parse_review_seed(const std::string_view text) {
    std::array<double, 4U> values{};
    std::size_t first = 0U;
    for (std::size_t index = 0U; index < values.size(); ++index) {
        const auto comma = text.find(',', first);
        const auto last = comma == std::string_view::npos ? text.size() : comma;
        const auto parsed = std::from_chars(text.data() + first, text.data() + last,
                                             values[index]);
        if (parsed.ec != std::errc{} || parsed.ptr != text.data() + last ||
            !std::isfinite(values[index]) ||
            ((index + 1U == values.size()) != (comma == std::string_view::npos))) {
            throw std::runtime_error("review seed must be x,y,width,height");
        }
        first = last + 1U;
    }
    if (values[0] < 0.0 || values[1] < 0.0 || values[2] <= 0.0 || values[3] <= 0.0 ||
        values[0] + values[2] > 1.0 || values[1] + values[3] > 1.0) {
        throw std::runtime_error("review seed must remain inside the source frame");
    }
    return {values[0], values[1], values[2], values[3]};
}

[[nodiscard]] inline Nanoseconds monotonic_ns() {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

[[nodiscard]] inline std::uint16_t little_u16(const std::byte* bytes) {
    return static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(bytes[0])) |
           static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(bytes[1])) << 8U;
}

[[nodiscard]] inline std::uint32_t little_u32(const std::byte* bytes) {
    return static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[0])) |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[1])) << 8U |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[2])) << 16U |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[3])) << 24U;
}

[[nodiscard]] inline std::vector<std::byte> read_binary(const std::filesystem::path& path) {
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

[[nodiscard]] inline std::string read_text(const std::filesystem::path& path) {
    std::ifstream stream(path, std::ios::binary);
    if (!stream) throw std::runtime_error("could not open " + path.string());
    return {std::istreambuf_iterator<char>(stream), std::istreambuf_iterator<char>()};
}

[[nodiscard]] inline std::string sha256_bytes(const std::vector<std::byte>& bytes) {
    if (bytes.size() > std::numeric_limits<ULONG>::max()) {
        throw std::runtime_error("review input too large to hash");
    }
    std::array<unsigned char, 32> hash{};
    if (BCryptHash(BCRYPT_SHA256_ALG_HANDLE, nullptr, 0,
            reinterpret_cast<PUCHAR>(const_cast<std::byte*>(bytes.data())),
            static_cast<ULONG>(bytes.size()), hash.data(),
            static_cast<ULONG>(hash.size())) < 0) {
        throw std::runtime_error("could not hash review input");
    }
    std::ostringstream digest;
    for (const auto byte : hash) {
        digest << std::hex << std::setfill('0') << std::setw(2)
               << static_cast<unsigned int>(byte);
    }
    return digest.str();
}

[[nodiscard]] inline std::uint32_t manifest_u32(const std::string& manifest,
                                         const std::string_view name) {
    const std::regex pattern("\\\"" + std::string(name) +
                             "\\\"\\s*:\\s*([0-9]+)");
    std::smatch match;
    if (!std::regex_search(manifest, match, pattern)) {
        throw std::runtime_error("review atlas manifest is missing " + std::string(name));
    }
    const auto value = std::stoull(match[1].str());
    if (value > std::numeric_limits<std::uint32_t>::max()) {
        throw std::runtime_error("review atlas manifest integer is out of range");
    }
    return static_cast<std::uint32_t>(value);
}

[[nodiscard]] inline std::vector<MouthCoefficients> manifest_coefficients(
    const std::string& manifest) {
    const std::regex block("\\\"coefficients\\\"\\s*:\\s*\\[([^\\]]+)\\]");
    const std::regex number("[-+]?(?:[0-9]+\\.?[0-9]*|\\.[0-9]+)(?:[eE][-+]?[0-9]+)?");
    std::vector<MouthCoefficients> result;
    for (auto state = std::sregex_iterator(manifest.begin(), manifest.end(), block);
         state != std::sregex_iterator(); ++state) {
        std::array<double, 8U> values{};
        std::size_t index{};
        const auto body = (*state)[1].str();
        for (auto value = std::sregex_iterator(body.begin(), body.end(), number);
             value != std::sregex_iterator(); ++value) {
            if (index >= values.size()) {
                throw std::runtime_error("review atlas coefficient vector is oversized");
            }
            values[index++] = std::stod((*value).str());
        }
        if (index != values.size()) {
            throw std::runtime_error("review atlas coefficient vector is incomplete");
        }
        result.push_back({values[0], values[1], values[2], values[3],
                          values[4], values[5], values[6], values[7]});
    }
    return result;
}

[[nodiscard]] inline CharacterMouthAtlas read_review_atlas(
    const std::filesystem::path& root,
    const std::uint64_t generation,
    const TrackBinding& track) {
    const auto manifest = read_text(root / "atlas.json");
    const auto schema = manifest_u32(manifest, "schemaVersion");
    std::smatch representation_match;
    const bool has_representation = std::regex_search(manifest, representation_match,
        std::regex("\"representation\"\\s*:\\s*\"([^\"]+)\""));
    const bool normalized_oral = has_representation &&
        representation_match[1].str() == "normalized-oral-interior-v1";
    const bool photometric_full_lip = has_representation &&
        representation_match[1].str() == "photometric-full-lip-reference-v1";
    std::smatch neutral_index;
    const bool has_neutral_index = std::regex_search(manifest, neutral_index,
        std::regex("\"neutralStateIndex\"\\s*:\\s*([0-9]+)"));
    if ((schema != 1U && schema != 2U && schema != 3U) ||
        (schema == 2U) != normalized_oral ||
        (schema == 3U) != photometric_full_lip ||
        (schema == 3U && (!has_neutral_index || neutral_index[1].str() != "0")) ||
        (schema == 1U && has_representation &&
         representation_match[1].str() != "full-lip-observation-v1")) {
        throw std::runtime_error("review atlas representation/schema mismatch");
    }
    const auto width = manifest_u32(manifest, "width");
    const auto height = manifest_u32(manifest, "height");
    const auto stride = manifest_u32(manifest, "strideBytes");
    const auto state_count = manifest_u32(manifest, "stateCount");
    const auto coefficients = manifest_coefficients(manifest);
    const auto state_bytes = static_cast<std::size_t>(stride) * height;
    if (width < 16U || height < 16U || width > 512U || height > 512U ||
        stride != width * 4U || state_count < 4U || state_count > 16U ||
        coefficients.size() != state_count) {
        throw std::runtime_error("review atlas manifest has an unsupported layout");
    }
    const auto texture_path = root / "atlas-bgra8-premultiplied.bin";
    const auto bytes = read_binary(texture_path);
    std::smatch texture_hash;
    if (!std::regex_search(manifest, texture_hash,
            std::regex("\"sha256\"\\s*:\\s*\"([0-9a-f]{64})\"")) ||
        texture_hash[1].str() != sha256_bytes(bytes)) {
        throw std::runtime_error("review atlas texture hash mismatch");
    }
    if (bytes.size() != state_bytes * state_count) {
        throw std::runtime_error("review atlas has an unexpected state layout");
    }
    CharacterMouthAtlas atlas{};
    atlas.schema_version = schema;
    atlas.cancellation_generation = generation;
    atlas.actor_id = track.actor_id;
    std::smatch identity;
    if (!std::regex_search(manifest, identity,
            std::regex("\"identityRevision\"\\s*:\\s*([0-9]+)"))) {
        throw std::runtime_error("review atlas has no identity revision");
    }
    atlas.identity_revision = std::stoull(identity[1].str());
    atlas.states.reserve(state_count);
    for (std::size_t index = 0U; index < state_count; ++index) {
        MouthAtlasState state{};
        state.appearance.representation = normalized_oral
            ? MouthPatchRepresentation::normalized_oral_interior_v1
            : photometric_full_lip
                ? MouthPatchRepresentation::photometric_full_lip_reference_v1
                : MouthPatchRepresentation::full_lip_observation_v1;
        state.coefficients = coefficients[index];
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

[[nodiscard]] inline WavPcm read_wav_pcm16(const std::filesystem::path& path) {
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

[[nodiscard]] inline std::string ppm_token(std::istream& stream) {
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

[[nodiscard]] inline CpuFrame read_ppm(const std::filesystem::path& path) {
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

[[nodiscard]] inline std::vector<CpuFrame> read_source_frames(
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

[[nodiscard]] inline NormalizedRect expanded_tracking_seed(
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

inline void write_ppm(const std::filesystem::path& path, const std::vector<std::uint8_t>& bgra,
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

[[nodiscard]] inline std::uint64_t digest(const std::vector<std::uint8_t>& bytes) {
    std::uint64_t value = 1469598103934665603ULL;
    for (const auto byte : bytes) value = (value ^ byte) * 1099511628211ULL;
    return value;
}

[[nodiscard]] inline std::uint64_t private_bytes() {
    PROCESS_MEMORY_COUNTERS_EX counters{};
    counters.cb = sizeof(counters);
    return GetProcessMemoryInfo(GetCurrentProcess(),
                                reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&counters),
                                sizeof(counters))
        ? static_cast<std::uint64_t>(counters.PrivateUsage)
        : 0U;
}

[[nodiscard]] inline double percentile(std::vector<double> values, const double quantile) {
    if (values.empty()) return 0.0;
    std::sort(values.begin(), values.end());
    const auto position = quantile * static_cast<double>(values.size() - 1U);
    const auto lower = static_cast<std::size_t>(std::floor(position));
    const auto upper = static_cast<std::size_t>(std::ceil(position));
    const auto weight = position - static_cast<double>(lower);
    return values[lower] * (1.0 - weight) + values[upper] * weight;
}

[[nodiscard]] inline std::string json_escape(const std::string_view value) {
    std::string output;
    for (const char character : value) {
        if (character == '\\' || character == '"') output.push_back('\\');
        if (character == '\n') output += "\\n";
        else if (character != '\r') output.push_back(character);
    }
    return output;
}

[[nodiscard]] inline AdmittedLandmarkProviderLaunchV1 launch_for(const std::filesystem::path& root) {
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
