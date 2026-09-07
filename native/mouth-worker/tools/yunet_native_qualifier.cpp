#include "npc/mouth_worker/landmark_provider.hpp"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <bcrypt.h>
#include <psapi.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <charconv>
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
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace {

using namespace npc::mouth;

constexpr std::string_view suite_version = "native-yunet-envelope-2026-09-05.1";
constexpr std::string_view runtime_archive_sha256 =
    "855276cd4be3cda14fe636c69eb038d75bf5bcd552bda1193a5d79c51f436dfe";
constexpr std::uint64_t runtime_archive_size_bytes = 73'731'806U;
constexpr std::string_view runtime_sha256 =
    "7788f3f38e9a339003f7d7e1bf47f928287cf409bb5273273149e1282fbf503f";
constexpr std::string_view runtime_shared_sha256 =
    "7a71dbee513692aeb0bd2346a50ae0edd28c8b34defc17254605a29e81c60569";
constexpr std::uint32_t minimum_operations = 20U;
constexpr std::uint32_t maximum_operations = 64U;

struct FileEvidence {
    std::filesystem::path path;
    std::uint64_t size_bytes{};
    std::string sha256;
};

struct ProcessMemory {
    std::uint64_t resident_bytes{};
    std::uint64_t peak_resident_bytes{};
    std::uint64_t private_bytes{};
};

struct SessionObservation {
    std::uint32_t iteration{};
    bool reload_after_unload{};
    double load_ms{};
    double inference_ms{};
    double unload_ms{};
    double detector_ms{};
    double landmark_ms{};
    ProcessMemory before_load;
    ProcessMemory after_load;
    ProcessMemory after_inference;
    ProcessMemory after_unload;
    std::uint64_t sampled_peak_resident_bytes{};
    std::uint64_t sampled_peak_private_bytes{};
    std::uint64_t absolute_process_peak_resident_bytes{};
    std::int64_t sampled_peak_resident_delta_from_baseline_bytes{};
    std::int64_t sampled_peak_private_delta_from_baseline_bytes{};
    double detector_confidence{};
    double landmark_confidence{};
    double visibility_ratio{};
    bool packet_bound{};
    bool cancellation_advanced{};
    bool stale_generation_rejected{};
    bool unloaded{};
};

class ProcessMemorySampler final {
public:
    ProcessMemorySampler() : worker_([this] { sample_loop(); }) {}
    ProcessMemorySampler(const ProcessMemorySampler&) = delete;
    ProcessMemorySampler& operator=(const ProcessMemorySampler&) = delete;
    ~ProcessMemorySampler() { (void)finish(); }

    [[nodiscard]] ProcessMemory finish() noexcept {
        if (worker_.joinable()) {
            stopping_.store(true, std::memory_order_release);
            worker_.join();
            sample_once();
        }
        return peak_;
    }

private:
    void sample_once() noexcept {
        PROCESS_MEMORY_COUNTERS_EX counters{};
        counters.cb = sizeof(counters);
        if (!GetProcessMemoryInfo(GetCurrentProcess(),
                                  reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&counters),
                                  sizeof(counters))) {
            return;
        }
        peak_.resident_bytes = std::max(
            peak_.resident_bytes, static_cast<std::uint64_t>(counters.WorkingSetSize));
        peak_.private_bytes = std::max(
            peak_.private_bytes, static_cast<std::uint64_t>(counters.PrivateUsage));
        peak_.peak_resident_bytes = std::max(
            peak_.peak_resident_bytes,
            static_cast<std::uint64_t>(counters.PeakWorkingSetSize));
    }

    void sample_loop() noexcept {
        while (!stopping_.load(std::memory_order_acquire)) {
            sample_once();
            Sleep(1U);
        }
    }

    std::atomic<bool> stopping_{};
    std::thread worker_;
    ProcessMemory peak_{};
};

[[nodiscard]] Nanoseconds monotonic_ns() noexcept {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
               std::chrono::steady_clock::now().time_since_epoch())
        .count();
}

[[nodiscard]] std::string lower_hex(const std::span<const std::byte> bytes) {
    constexpr char digits[] = "0123456789abcdef";
    std::string value(bytes.size() * 2U, '0');
    for (std::size_t index = 0U; index < bytes.size(); ++index) {
        const auto byte = std::to_integer<unsigned int>(bytes[index]);
        value[index * 2U] = digits[byte >> 4U];
        value[index * 2U + 1U] = digits[byte & 0x0fU];
    }
    return value;
}

[[nodiscard]] FileEvidence evidence_for(const std::filesystem::path& path) {
    std::ifstream stream(path, std::ios::binary);
    if (!stream) throw std::runtime_error("could not read evidence file: " + path.string());

    BCRYPT_ALG_HANDLE algorithm{};
    BCRYPT_HASH_HANDLE hash{};
    DWORD object_size{};
    DWORD transferred{};
    std::vector<std::byte> object;
    std::array<std::byte, 32U> digest{};
    bool accepted = BCryptOpenAlgorithmProvider(
        &algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0U) == 0 &&
        BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                          reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size),
                          &transferred, 0U) == 0 && object_size != 0U;
    if (accepted) {
        object.resize(object_size);
        accepted = BCryptCreateHash(algorithm, &hash,
                                    reinterpret_cast<PUCHAR>(object.data()), object_size,
                                    nullptr, 0U, 0U) == 0;
    }
    std::array<char, 64U * 1024U> block{};
    while (accepted && stream) {
        stream.read(block.data(), static_cast<std::streamsize>(block.size()));
        const auto count = stream.gcount();
        if (count > 0) {
            accepted = BCryptHashData(hash, reinterpret_cast<PUCHAR>(block.data()),
                                      static_cast<ULONG>(count), 0U) == 0;
        }
    }
    if (accepted && !stream.eof()) accepted = false;
    if (accepted) {
        accepted = BCryptFinishHash(hash, reinterpret_cast<PUCHAR>(digest.data()),
                                    static_cast<ULONG>(digest.size()), 0U) == 0;
    }
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0U);
    if (!accepted) throw std::runtime_error("SHA-256 failed: " + path.string());
    return {std::filesystem::canonical(path), std::filesystem::file_size(path),
            lower_hex(digest)};
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
    if (!stream || ppm_token(stream) != "P6") {
        throw std::runtime_error("source must be a P6 PPM file");
    }
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
    frame.lease.lease_nonce_high = 0x5155414c49465931ULL;
    frame.bgra.resize(static_cast<std::size_t>(frame.lease.stride_bytes) * height);
    for (std::size_t pixel = 0U; pixel < static_cast<std::size_t>(width) * height; ++pixel) {
        frame.bgra[pixel * 4U + 0U] = rgb[pixel * 3U + 2U];
        frame.bgra[pixel * 4U + 1U] = rgb[pixel * 3U + 1U];
        frame.bgra[pixel * 4U + 2U] = rgb[pixel * 3U + 0U];
        frame.bgra[pixel * 4U + 3U] = 255U;
    }
    return frame;
}

[[nodiscard]] ProcessMemory process_memory() {
    PROCESS_MEMORY_COUNTERS_EX counters{};
    counters.cb = sizeof(counters);
    if (!GetProcessMemoryInfo(GetCurrentProcess(),
                              reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&counters),
                              sizeof(counters))) {
        throw std::runtime_error("GetProcessMemoryInfo failed");
    }
    return {static_cast<std::uint64_t>(counters.WorkingSetSize),
            static_cast<std::uint64_t>(counters.PeakWorkingSetSize),
            static_cast<std::uint64_t>(counters.PrivateUsage)};
}

[[nodiscard]] std::int64_t signed_delta(const std::uint64_t after,
                                        const std::uint64_t before) noexcept {
    if (after >= before) {
        return static_cast<std::int64_t>(std::min(
            after - before,
            static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max())));
    }
    return -static_cast<std::int64_t>(std::min(
        before - after,
        static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max())));
}

[[nodiscard]] double percentile(std::vector<double> values, const double quantile) {
    std::sort(values.begin(), values.end());
    const double position = static_cast<double>(values.size() - 1U) * quantile;
    const auto lower = static_cast<std::size_t>(std::floor(position));
    const auto upper = static_cast<std::size_t>(std::ceil(position));
    const double fraction = position - static_cast<double>(lower);
    return values[lower] + (values[upper] - values[lower]) * fraction;
}

void write_statistics(std::ostream& stream, const std::vector<double>& values) {
    if (values.empty()) throw std::runtime_error("statistics require observations");
    stream << "{\"count\":" << values.size()
           << ",\"mean\":" << std::accumulate(values.begin(), values.end(), 0.0) /
                                  static_cast<double>(values.size())
           << ",\"p50\":" << percentile(values, 0.50)
           << ",\"p95\":" << percentile(values, 0.95)
           << ",\"p99\":" << percentile(values, 0.99)
           << ",\"maximum\":" << *std::max_element(values.begin(), values.end()) << '}';
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

[[nodiscard]] std::string utc_now() {
    SYSTEMTIME value{};
    GetSystemTime(&value);
    std::ostringstream stream;
    stream << std::setfill('0') << value.wYear << '-' << std::setw(2) << value.wMonth << '-'
           << std::setw(2) << value.wDay << 'T' << std::setw(2) << value.wHour << ':'
           << std::setw(2) << value.wMinute << ':' << std::setw(2) << value.wSecond << '.'
           << std::setw(3) << value.wMilliseconds << 'Z';
    return stream.str();
}

[[nodiscard]] std::filesystem::path executable_path() {
    std::vector<wchar_t> buffer(1024U);
    for (;;) {
        const DWORD count = GetModuleFileNameW(nullptr, buffer.data(),
                                               static_cast<DWORD>(buffer.size()));
        if (count == 0U) throw std::runtime_error("GetModuleFileNameW failed");
        if (count < buffer.size() - 1U) {
            return std::filesystem::canonical(std::filesystem::path(
                std::wstring_view(buffer.data(), count)));
        }
        buffer.resize(buffer.size() * 2U);
    }
}

[[nodiscard]] std::string cpu_name() {
    HKEY key{};
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE,
                      L"HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0", 0U,
                      KEY_QUERY_VALUE, &key) != ERROR_SUCCESS) {
        return "unavailable";
    }
    std::array<wchar_t, 256U> value{};
    DWORD type{};
    DWORD bytes = static_cast<DWORD>(value.size() * sizeof(wchar_t));
    const LSTATUS status = RegQueryValueExW(key, L"ProcessorNameString", nullptr, &type,
                                           reinterpret_cast<LPBYTE>(value.data()), &bytes);
    RegCloseKey(key);
    if (status != ERROR_SUCCESS || (type != REG_SZ && type != REG_EXPAND_SZ)) {
        return "unavailable";
    }
    const std::wstring wide(value.data());
    if (wide.empty()) return "unavailable";
    const int count = WideCharToMultiByte(CP_UTF8, 0, wide.c_str(),
                                          static_cast<int>(wide.size()), nullptr, 0,
                                          nullptr, nullptr);
    if (count <= 0) return "unavailable";
    std::string result(static_cast<std::size_t>(count), '\0');
    WideCharToMultiByte(CP_UTF8, 0, wide.c_str(), static_cast<int>(wide.size()),
                        result.data(), count, nullptr, nullptr);
    return result;
}

[[nodiscard]] AdmittedLandmarkProviderLaunchV1 launch_for(
    const std::filesystem::path& root) {
    AdmittedLandmarkProviderLaunchV1 launch{};
    launch.pack_id = std::string(admitted_yunet_openseeface_pack_id_v1);
    launch.pack_revision = std::string(admitted_openseeface_revision_v1);
    launch.artifact_root = std::filesystem::canonical(root);
    launch.detector_model = launch.artifact_root / "models/face_detection_yunet_2023mar.onnx";
    launch.landmark_model = launch.artifact_root / "models/lm_model1_opt.onnx";
    launch.runtime_library =
        launch.artifact_root / "runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime.dll";
    launch.runtime_shared_library = launch.artifact_root /
        "runtime/onnxruntime-1.22.1-cpu/lib/onnxruntime_providers_shared.dll";
    launch.detector_size_bytes = std::filesystem::file_size(launch.detector_model);
    launch.landmark_size_bytes = std::filesystem::file_size(launch.landmark_model);
    launch.runtime_size_bytes = std::filesystem::file_size(launch.runtime_library);
    launch.runtime_shared_size_bytes = std::filesystem::file_size(launch.runtime_shared_library);
    launch.detector_sha256 = std::string(admitted_yunet_detector_sha256_v1);
    launch.landmark_sha256 = std::string(admitted_openseeface_lm1_sha256_v1);
    launch.runtime_sha256 = std::string(runtime_sha256);
    launch.runtime_shared_sha256 = std::string(runtime_shared_sha256);
    launch.measured_envelope_sha256 = std::string(runtime_sha256);
    launch.runtime_revision = std::string(admitted_openseeface_runtime_revision_v1);
    launch.backend = std::string(admitted_openseeface_backend_v1);
    launch.maximum_signal_rate_hz = 15U;
    launch.inference_threads = 1U;
    launch.exact_target_process_id = GetCurrentProcessId();
    return launch;
}

void write_artifact(std::ostream& stream,
                    const std::string_view role,
                    const FileEvidence& value) {
    stream << "{\"role\":\"" << role << "\",\"path\":\""
           << json_escape(value.path.string()) << "\",\"sizeBytes\":" << value.size_bytes
           << ",\"sha256\":\"" << value.sha256 << "\"}";
}

} // namespace

int main(int argc, char** argv) {
    try {
        if (argc < 4 || argc > 5) {
            throw std::runtime_error(
                "usage: npc_mouth_worker_yunet_qualifier <pack-root> <source.ppm> "
                "<output.json> [operations:20..64]");
        }
        std::uint32_t operation_count = 24U;
        if (argc == 5) {
            const std::string_view text(argv[4]);
            const auto parsed = std::from_chars(
                text.data(), text.data() + text.size(), operation_count);
            if (parsed.ec != std::errc{} || parsed.ptr != text.data() + text.size() ||
                operation_count < minimum_operations || operation_count > maximum_operations) {
                throw std::runtime_error("operations must be an integer from 20 through 64");
            }
        }

        const auto started_utc = utc_now();
        const auto pack_root = std::filesystem::canonical(argv[1]);
        const auto source_path = std::filesystem::canonical(argv[2]);
        const auto output_path = std::filesystem::absolute(argv[3]);
        if (!output_path.parent_path().empty()) {
            std::filesystem::create_directories(output_path.parent_path());
        }
        const auto source_evidence = evidence_for(source_path);
        const auto executable_evidence = evidence_for(executable_path());
        const auto launch = launch_for(pack_root);
        const auto archive_evidence = evidence_for(
            pack_root / "onnxruntime-win-x64-1.22.1.zip");
        const auto detector_evidence = evidence_for(launch.detector_model);
        const auto landmark_evidence = evidence_for(launch.landmark_model);
        const auto runtime_evidence = evidence_for(launch.runtime_library);
        const auto shared_evidence = evidence_for(launch.runtime_shared_library);
        if (archive_evidence.size_bytes != runtime_archive_size_bytes ||
            archive_evidence.sha256 != runtime_archive_sha256 ||
            detector_evidence.sha256 != launch.detector_sha256 ||
            landmark_evidence.sha256 != launch.landmark_sha256 ||
            runtime_evidence.sha256 != launch.runtime_sha256 ||
            shared_evidence.sha256 != launch.runtime_shared_sha256) {
            throw std::runtime_error("artifact evidence does not match admitted exact hashes");
        }

        // Decode once. Operation clocks and frame capture timestamps begin only
        // after this fixture is an in-memory BGRA frame, matching the native
        // capture-worker/provider boundary.
        const auto source_template = read_ppm(source_path);
        std::vector<SessionObservation> sessions;
        sessions.reserve(static_cast<std::size_t>(operation_count) * 2U);
        const TrackBinding base_track{1U, 0x5155414c49465931ULL,
                                      0x59554e4554363430ULL, 1U};
        std::uint64_t frame_sequence = 1U;
        const auto measure_session = [&](NativeLandmarkProviderV1& provider,
                                         const std::uint32_t iteration,
                                         const bool reload_after_unload,
                                         const std::uint64_t generation) {
            SessionObservation observation{};
            observation.iteration = iteration;
            observation.reload_after_unload = reload_after_unload;
            auto source = source_template;
            ProcessMemorySampler memory_sampler;
            // Establish the model-memory baseline after the sampler thread is
            // resident, so its stack is not misattributed to the ORT sessions.
            observation.before_load = process_memory();
            std::string failure;
            const auto load_started = std::chrono::steady_clock::now();
            if (!provider.load(launch, generation, failure)) {
                throw std::runtime_error("provider load failed: " + failure);
            }
            observation.load_ms = std::chrono::duration<double, std::milli>(
                std::chrono::steady_clock::now() - load_started).count();
            observation.after_load = process_memory();

            const Nanoseconds frame_ready = monotonic_ns();
            source.identity = {frame_sequence++, 1U, 1U, frame_ready};
            source.lease.lease_nonce_low = source.identity.sequence;
            source.lease.expires_at_ns = frame_ready + 5'000'000'000LL;
            LandmarkInferenceWorkV1 work{};
            work.track = base_track;
            work.track.cancellation_generation = generation;
            work.frame = source.identity;
            work.source_frame_qpc = static_cast<std::uint64_t>(frame_ready);
            work.qpc_frequency = 1'000'000'000U;
            work.seed_face_bounds = {0.0, 0.0, 1.0, 0.88};
            work.source = std::move(source);
            work.deadline_ns = frame_ready + 5'000'000'000LL;
            const auto infer_started = std::chrono::steady_clock::now();
            const auto packet = provider.infer(work, frame_ready, failure);
            observation.inference_ms = std::chrono::duration<double, std::milli>(
                std::chrono::steady_clock::now() - infer_started).count();
            const auto diagnostics = provider.last_inference_diagnostics();
            observation.detector_ms = diagnostics.detector_ms;
            observation.landmark_ms = diagnostics.landmark_ms;
            observation.after_inference = process_memory();
            if (!packet) throw std::runtime_error("provider inference failed: " + failure);
            observation.packet_bound = packet->track == work.track &&
                packet->frame == work.frame &&
                packet->source_frame_qpc == work.source_frame_qpc &&
                packet->qpc_frequency == work.qpc_frequency;
            observation.detector_confidence = packet->detector_confidence;
            observation.landmark_confidence = packet->landmark_confidence;
            observation.visibility_ratio = packet->visibility_ratio;
            if (!observation.packet_bound) {
                throw std::runtime_error("provider packet escaped exact work binding");
            }

            observation.cancellation_advanced = provider.cancel_to(generation + 1U);
            failure.clear();
            observation.stale_generation_rejected =
                !provider.infer(work, monotonic_ns(), failure).has_value();
            if (!observation.cancellation_advanced ||
                !observation.stale_generation_rejected) {
                throw std::runtime_error("provider cancellation boundary failed");
            }
            const auto unload_started = std::chrono::steady_clock::now();
            provider.unload();
            observation.unload_ms = std::chrono::duration<double, std::milli>(
                std::chrono::steady_clock::now() - unload_started).count();
            observation.unloaded = !provider.loaded();
            observation.after_unload = process_memory();
            if (!observation.unloaded) throw std::runtime_error("provider remained loaded");
            const auto sampled_peak = memory_sampler.finish();
            if (sampled_peak.resident_bytes == 0U || sampled_peak.private_bytes == 0U ||
                sampled_peak.peak_resident_bytes == 0U) {
                throw std::runtime_error("process memory sampling failed");
            }
            observation.sampled_peak_resident_bytes = std::max({
                sampled_peak.resident_bytes, observation.before_load.resident_bytes,
                observation.after_load.resident_bytes,
                observation.after_inference.resident_bytes,
                observation.after_unload.resident_bytes});
            observation.sampled_peak_private_bytes = std::max({
                sampled_peak.private_bytes, observation.before_load.private_bytes,
                observation.after_load.private_bytes,
                observation.after_inference.private_bytes,
                observation.after_unload.private_bytes});
            observation.absolute_process_peak_resident_bytes =
                sampled_peak.peak_resident_bytes;
            observation.sampled_peak_resident_delta_from_baseline_bytes = signed_delta(
                sampled_peak.resident_bytes, observation.before_load.resident_bytes);
            observation.sampled_peak_private_delta_from_baseline_bytes = signed_delta(
                sampled_peak.private_bytes, observation.before_load.private_bytes);
            sessions.push_back(observation);
        };

        for (std::uint32_t index = 0U; index < operation_count; ++index) {
            auto provider = make_windows_ort_landmark_provider_v1();
            if (!provider) throw std::runtime_error("Windows ORT provider is unavailable");
            const std::uint64_t generation = static_cast<std::uint64_t>(index) * 4U + 1U;
            measure_session(*provider, index, false, generation);
            measure_session(*provider, index, true, generation + 2U);
        }

        std::vector<double> fresh_loads;
        std::vector<double> reload_loads;
        std::vector<double> fresh_inferences;
        std::vector<double> reload_inferences;
        std::vector<double> sampled_peak_resident;
        std::vector<double> sampled_peak_private;
        std::vector<double> absolute_process_peak_resident;
        std::vector<double> sampled_peak_resident_delta;
        std::vector<double> sampled_peak_private_delta;
        for (const auto& session : sessions) {
            auto& loads = session.reload_after_unload ? reload_loads : fresh_loads;
            auto& inferences = session.reload_after_unload
                ? reload_inferences
                : fresh_inferences;
            loads.push_back(session.load_ms);
            inferences.push_back(session.inference_ms);
            sampled_peak_resident.push_back(
                static_cast<double>(session.sampled_peak_resident_bytes));
            sampled_peak_private.push_back(
                static_cast<double>(session.sampled_peak_private_bytes));
            absolute_process_peak_resident.push_back(
                static_cast<double>(session.absolute_process_peak_resident_bytes));
            sampled_peak_resident_delta.push_back(static_cast<double>(
                session.sampled_peak_resident_delta_from_baseline_bytes));
            sampled_peak_private_delta.push_back(static_cast<double>(
                session.sampled_peak_private_delta_from_baseline_bytes));
        }

        SYSTEM_INFO system{};
        GetNativeSystemInfo(&system);
        MEMORYSTATUSEX memory{};
        memory.dwLength = sizeof(memory);
        if (!GlobalMemoryStatusEx(&memory)) {
            throw std::runtime_error("GlobalMemoryStatusEx failed");
        }
        std::ofstream stream(output_path, std::ios::binary);
        if (!stream) throw std::runtime_error("could not create qualifier report");
        stream << std::fixed << std::setprecision(6);
        stream << "{\n  \"schema\":\"interactive-npcs-yunet-native-envelope/v1\","
               << "\n  \"suiteVersion\":\"" << suite_version << "\","
               << "\n  \"startedAtUtc\":\"" << started_utc << "\","
               << "\n  \"completedAtUtc\":\"" << utc_now() << "\","
               << "\n  \"scope\":{\"provider\":\"native-windows-ort\","
               << "\"backend\":\"cpu-execution-provider-one-thread\","
               << "\"runtimeRevision\":\"" << admitted_openseeface_runtime_revision_v1
               << "\",\"inferenceThreads\":1,\"gpuUsed\":false,"
               << "\"fixtureDecodeExcluded\":true,"
               << "\"currentDeviceOnly\":true,\"gamePressureMeasured\":false,"
               << "\"captureMeasured\":false,\"sixtyFpsMeasured\":false,"
               << "\"fixtureIdle\":true,\"osDiskCacheStateControlled\":false,"
               << "\"telemetryFingerprint\":\"issuer-collected\"},"
               << "\n  \"device\":{\"cpu\":\"" << json_escape(cpu_name())
               << "\",\"logicalProcessors\":" << system.dwNumberOfProcessors
               << ",\"pageSizeBytes\":" << system.dwPageSize
               << ",\"physicalMemoryBytes\":" << memory.ullTotalPhys << "},"
               << "\n  \"pack\":{\"id\":\"" << admitted_yunet_openseeface_pack_id_v1
               << "\",\"revision\":\"" << admitted_openseeface_revision_v1
               << "\"},\n  \"artifacts\":[";
        write_artifact(stream, "runtimeArchive", archive_evidence); stream << ',';
        write_artifact(stream, "detector", detector_evidence); stream << ',';
        write_artifact(stream, "landmark", landmark_evidence); stream << ',';
        write_artifact(stream, "runtime", runtime_evidence); stream << ',';
        write_artifact(stream, "runtimeShared", shared_evidence); stream << ',';
        write_artifact(stream, "qualifierExecutable", executable_evidence); stream << ',';
        write_artifact(stream, "sourceFrame", source_evidence);
        stream << "],\n  \"iterationCount\":" << operation_count
               << ",\n  \"sessionCount\":" << sessions.size()
               << ",\n  \"sessions\":[\n";
        for (std::size_t index = 0U; index < sessions.size(); ++index) {
            const auto& session = sessions[index];
            stream << "    {\"iteration\":" << session.iteration
                   << ",\"kind\":\""
                   << (session.reload_after_unload
                           ? "reloadAfterUnload"
                           : "freshProviderLoad")
                   << "\",\"loadMs\":" << session.load_ms
                   << ",\"inferenceMs\":" << session.inference_ms
                   << ",\"unloadMs\":" << session.unload_ms
                   << ",\"detectorMs\":" << session.detector_ms
                   << ",\"landmarkMs\":" << session.landmark_ms
                   << ",\"memory\":{\"beforeLoadResidentBytes\":"
                   << session.before_load.resident_bytes
                   << ",\"afterLoadResidentBytes\":" << session.after_load.resident_bytes
                   << ",\"afterInferenceResidentBytes\":"
                   << session.after_inference.resident_bytes
                   << ",\"afterUnloadResidentBytes\":"
                   << session.after_unload.resident_bytes
                   << ",\"sampledPeakResidentBytes\":"
                   << session.sampled_peak_resident_bytes
                   << ",\"sampledPeakPrivateBytes\":"
                   << session.sampled_peak_private_bytes
                   << ",\"absoluteProcessPeakResidentBytes\":"
                   << session.absolute_process_peak_resident_bytes
                   << ",\"sampledPeakResidentDeltaFromBaselineBytes\":"
                   << session.sampled_peak_resident_delta_from_baseline_bytes
                   << ",\"sampledPeakPrivateDeltaFromBaselineBytes\":"
                   << session.sampled_peak_private_delta_from_baseline_bytes << '}'
                   << ",\"detectorConfidence\":" << session.detector_confidence
                   << ",\"landmarkConfidence\":" << session.landmark_confidence
                   << ",\"visibilityRatio\":" << session.visibility_ratio
                   << ",\"packetBound\":" << (session.packet_bound ? "true" : "false")
                   << ",\"cancellationAdvanced\":"
                   << (session.cancellation_advanced ? "true" : "false")
                   << ",\"staleGenerationRejected\":"
                   << (session.stale_generation_rejected ? "true" : "false")
                   << ",\"unloaded\":" << (session.unloaded ? "true" : "false") << '}'
                   << (index + 1U == sessions.size() ? "\n" : ",\n");
        }
        stream << "  ],\n  \"aggregates\":{\"freshProviderLoadMs\":";
        write_statistics(stream, fresh_loads);
        stream << ",\"reloadAfterUnloadMs\":"; write_statistics(stream, reload_loads);
        stream << ",\"freshInferenceMs\":"; write_statistics(stream, fresh_inferences);
        stream << ",\"reloadInferenceMs\":"; write_statistics(stream, reload_inferences);
        stream << ",\"sampledPeakResidentBytes\":";
        write_statistics(stream, sampled_peak_resident);
        stream << ",\"sampledPeakPrivateBytes\":";
        write_statistics(stream, sampled_peak_private);
        stream << ",\"absoluteProcessPeakResidentBytes\":";
        write_statistics(stream, absolute_process_peak_resident);
        stream << ",\"sampledPeakResidentDeltaFromBaselineBytes\":";
        write_statistics(stream, sampled_peak_resident_delta);
        stream << ",\"sampledPeakPrivateDeltaFromBaselineBytes\":";
        write_statistics(stream, sampled_peak_private_delta);
        stream << "},\n  \"counters\":{\"freshProviderLoads\":" << operation_count
               << ",\"reloadsAfterUnload\":" << operation_count
               << ",\"packetBound\":" << sessions.size()
               << ",\"cancellationPassed\":" << sessions.size()
               << ",\"staleGenerationRejected\":" << sessions.size()
               << ",\"unloadPassed\":" << sessions.size() << "}\n}\n";
        if (!stream) throw std::runtime_error("could not write qualifier report");
        std::cout << output_path.string() << " iterations=" << operation_count
                  << " sessions=" << sessions.size() << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "YuNet native qualifier error: " << error.what() << '\n';
        return 2;
    }
}
