#include "npc/mouth_worker/landmark_provider.hpp"

#ifdef _WIN32

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <bcrypt.h>

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <memory>
#include <numbers>
#include <numeric>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace npc::mouth {
namespace {

// The product deliberately does not bundle or link ONNX Runtime. The optional
// pack pins ORT 1.22.1 and its exact DLL hash. These opaque declarations and
// indices are the frozen C API v22 ABI from the upstream 1.22.1 header. Loading
// any other runtime version or file digest fails before a session is created.
struct OrtStatus;
struct OrtEnv;
struct OrtSession;
struct OrtSessionOptions;
struct OrtRunOptions;
struct OrtMemoryInfo;
struct OrtValue;
struct OrtTensorTypeAndShapeInfo;

struct OrtApiSlots {
    std::array<void*, 318U> slots;
};

struct OrtApiBaseV22 {
    const OrtApiSlots* (__stdcall* get_api)(std::uint32_t) noexcept;
    const char* (__stdcall* get_version_string)() noexcept;
};

using OrtGetApiBaseFn = const OrtApiBaseV22* (__stdcall*)() noexcept;
using GetErrorMessageFn = const char* (__stdcall*)(const OrtStatus*) noexcept;
using CreateEnvFn = OrtStatus* (__stdcall*)(int, const char*, OrtEnv**);
using CreateSessionFn = OrtStatus* (__stdcall*)(const OrtEnv*, const wchar_t*,
                                                const OrtSessionOptions*, OrtSession**);
using RunFn = OrtStatus* (__stdcall*)(OrtSession*, const OrtRunOptions*,
                                     const char* const*, const OrtValue* const*, std::size_t,
                                     const char* const*, std::size_t, OrtValue**);
using CreateSessionOptionsFn = OrtStatus* (__stdcall*)(OrtSessionOptions**);
using SetSessionExecutionModeFn = OrtStatus* (__stdcall*)(OrtSessionOptions*, int);
using SetSessionGraphOptimizationLevelFn = OrtStatus* (__stdcall*)(OrtSessionOptions*, int);
using SetThreadCountFn = OrtStatus* (__stdcall*)(OrtSessionOptions*, int);
using CreateTensorWithDataFn = OrtStatus* (__stdcall*)(const OrtMemoryInfo*, void*,
                                                       std::size_t, const std::int64_t*,
                                                       std::size_t, int, OrtValue**);
using IsTensorFn = OrtStatus* (__stdcall*)(const OrtValue*, int*);
using GetTensorMutableDataFn = OrtStatus* (__stdcall*)(OrtValue*, void**);
using GetTensorElementTypeFn = OrtStatus* (__stdcall*)(const OrtTensorTypeAndShapeInfo*, int*);
using GetDimensionsCountFn = OrtStatus* (__stdcall*)(const OrtTensorTypeAndShapeInfo*,
                                                     std::size_t*);
using GetDimensionsFn = OrtStatus* (__stdcall*)(const OrtTensorTypeAndShapeInfo*,
                                                std::int64_t*, std::size_t);
using GetTensorShapeElementCountFn = OrtStatus* (__stdcall*)(
    const OrtTensorTypeAndShapeInfo*, std::size_t*);
using GetTensorTypeAndShapeFn = OrtStatus* (__stdcall*)(const OrtValue*,
                                                       OrtTensorTypeAndShapeInfo**);
using CreateCpuMemoryInfoFn = OrtStatus* (__stdcall*)(int, int, OrtMemoryInfo**);
using ReleaseEnvFn = void (__stdcall*)(OrtEnv*);
using ReleaseStatusFn = void (__stdcall*)(OrtStatus*);
using ReleaseMemoryInfoFn = void (__stdcall*)(OrtMemoryInfo*);
using ReleaseSessionFn = void (__stdcall*)(OrtSession*);
using ReleaseValueFn = void (__stdcall*)(OrtValue*);
using ReleaseTensorTypeAndShapeInfoFn = void (__stdcall*)(OrtTensorTypeAndShapeInfo*);
using ReleaseSessionOptionsFn = void (__stdcall*)(OrtSessionOptions*);

constexpr std::uint32_t ort_api_version_v22 = 22U;
constexpr int ort_logging_warning = 2;
constexpr int ort_sequential = 0;
constexpr int ort_enable_all = 99;
constexpr int ort_arena_allocator = 1;
constexpr int ort_mem_type_default = 0;
constexpr int onnx_tensor_float = 1;
constexpr std::size_t detector_input_size = 224U;
constexpr std::size_t detector_grid_size = 56U;
constexpr std::size_t yunet_input_size = 640U;
constexpr std::size_t landmark_grid_size = 28U;
constexpr std::size_t landmark_channels = 198U;
constexpr float detector_threshold = 0.60F;
constexpr double landmark_visibility_threshold = 0.55;

enum class ApiSlot : std::size_t {
    get_error_message = 2U,
    create_env = 3U,
    create_session = 7U,
    run = 9U,
    create_session_options = 10U,
    set_session_execution_mode = 13U,
    set_session_graph_optimization_level = 23U,
    set_intra_op_num_threads = 24U,
    set_inter_op_num_threads = 25U,
    create_tensor_with_data = 49U,
    is_tensor = 50U,
    get_tensor_mutable_data = 51U,
    get_tensor_element_type = 60U,
    get_dimensions_count = 61U,
    get_dimensions = 62U,
    get_tensor_shape_element_count = 64U,
    get_tensor_type_and_shape = 65U,
    create_cpu_memory_info = 69U,
    release_env = 92U,
    release_status = 93U,
    release_memory_info = 94U,
    release_session = 95U,
    release_value = 96U,
    release_tensor_type_and_shape_info = 99U,
    release_session_options = 100U,
};

class UniqueHandle final {
public:
    UniqueHandle() = default;
    explicit UniqueHandle(HANDLE value) noexcept : value_(value) {}
    ~UniqueHandle() { reset(); }
    UniqueHandle(const UniqueHandle&) = delete;
    UniqueHandle& operator=(const UniqueHandle&) = delete;
    UniqueHandle(UniqueHandle&& other) noexcept : value_(other.release()) {}
    UniqueHandle& operator=(UniqueHandle&& other) noexcept {
        if (this != &other) reset(other.release());
        return *this;
    }
    [[nodiscard]] HANDLE get() const noexcept { return value_; }
    [[nodiscard]] explicit operator bool() const noexcept {
        return value_ && value_ != INVALID_HANDLE_VALUE;
    }
    [[nodiscard]] HANDLE release() noexcept {
        const HANDLE value = value_;
        value_ = nullptr;
        return value;
    }
    void reset(HANDLE value = nullptr) noexcept {
        if (value_ && value_ != INVALID_HANDLE_VALUE) CloseHandle(value_);
        value_ = value;
    }

private:
    HANDLE value_{};
};

class UniqueModule final {
public:
    ~UniqueModule() { reset(); }
    UniqueModule() = default;
    UniqueModule(const UniqueModule&) = delete;
    UniqueModule& operator=(const UniqueModule&) = delete;
    [[nodiscard]] HMODULE get() const noexcept { return value_; }
    [[nodiscard]] explicit operator bool() const noexcept { return value_ != nullptr; }
    void reset(HMODULE value = nullptr) noexcept {
        if (value_) FreeLibrary(value_);
        value_ = value;
    }

private:
    HMODULE value_{};
};

struct VerifiedFile {
    std::filesystem::path path;
    UniqueHandle handle;
};

[[nodiscard]] Nanoseconds monotonic_ns() noexcept {
    LARGE_INTEGER counter{};
    LARGE_INTEGER frequency{};
    if (!QueryPerformanceCounter(&counter) || !QueryPerformanceFrequency(&frequency) ||
        counter.QuadPart <= 0 || frequency.QuadPart <= 0) {
        return 0;
    }
    const long double value = static_cast<long double>(counter.QuadPart) * 1'000'000'000.0L /
                              static_cast<long double>(frequency.QuadPart);
    return value <= static_cast<long double>(std::numeric_limits<Nanoseconds>::max())
        ? static_cast<Nanoseconds>(value)
        : 0;
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

[[nodiscard]] bool hash_open_file(const HANDLE file, std::string& digest) {
    BCRYPT_ALG_HANDLE algorithm{};
    BCRYPT_HASH_HANDLE hash{};
    DWORD object_size{};
    DWORD transferred{};
    std::vector<std::byte> object;
    std::array<std::byte, 32U> bytes{};
    bool accepted = false;
    LARGE_INTEGER origin{};
    if (SetFilePointerEx(file, origin, nullptr, FILE_BEGIN) &&
        BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0U) == 0 &&
        BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                          reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size),
                          &transferred, 0U) == 0 && object_size != 0U) {
        object.resize(object_size);
        if (BCryptCreateHash(algorithm, &hash, reinterpret_cast<PUCHAR>(object.data()),
                             object_size, nullptr, 0U, 0U) == 0) {
            std::array<std::byte, 64U * 1024U> block{};
            accepted = true;
            for (;;) {
                DWORD count{};
                if (!ReadFile(file, block.data(), static_cast<DWORD>(block.size()), &count,
                              nullptr)) {
                    accepted = false;
                    break;
                }
                if (count == 0U) break;
                if (BCryptHashData(hash, reinterpret_cast<PUCHAR>(block.data()), count, 0U) !=
                    0) {
                    accepted = false;
                    break;
                }
            }
            if (accepted && BCryptFinishHash(hash, reinterpret_cast<PUCHAR>(bytes.data()),
                                             static_cast<ULONG>(bytes.size()), 0U) != 0) {
                accepted = false;
            }
        }
    }
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0U);
    if (accepted) digest = lower_hex(bytes);
    return accepted;
}

[[nodiscard]] std::optional<VerifiedFile> verify_and_lock_file(
    const std::filesystem::path& path,
    const std::uint64_t expected_size,
    const std::string_view expected_digest) {
    UniqueHandle file(CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr,
                                  OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN,
                                  nullptr));
    LARGE_INTEGER size{};
    std::string digest;
    if (!file || !GetFileSizeEx(file.get(), &size) || size.QuadPart < 0 ||
        static_cast<std::uint64_t>(size.QuadPart) != expected_size ||
        !hash_open_file(file.get(), digest) || digest != expected_digest) {
        return std::nullopt;
    }
    return VerifiedFile{path, std::move(file)};
}

template <typename Function>
[[nodiscard]] Function api_function(const OrtApiSlots* api, const ApiSlot slot) noexcept {
    if (!api) return nullptr;
    return reinterpret_cast<Function>(api->slots[static_cast<std::size_t>(slot)]);
}

struct PixelRect {
    double left{};
    double top{};
    double right{};
    double bottom{};

    [[nodiscard]] double width() const noexcept { return right - left; }
    [[nodiscard]] double height() const noexcept { return bottom - top; }
};

[[nodiscard]] PixelRect normalized_to_pixels(const NormalizedRect& value,
                                             const std::uint32_t width,
                                             const std::uint32_t height) noexcept {
    return {
        value.x * static_cast<double>(width),
        value.y * static_cast<double>(height),
        value.right() * static_cast<double>(width),
        value.bottom() * static_cast<double>(height),
    };
}

[[nodiscard]] bool valid_pixel_rect(const PixelRect& value,
                                    const std::uint32_t width,
                                    const std::uint32_t height) noexcept {
    return std::isfinite(value.left) && std::isfinite(value.top) &&
           std::isfinite(value.right) && std::isfinite(value.bottom) &&
           value.left >= 0.0 && value.top >= 0.0 &&
           value.right <= static_cast<double>(width) &&
           value.bottom <= static_cast<double>(height) && value.width() >= 4.0 &&
           value.height() >= 4.0;
}

[[nodiscard]] std::vector<float> tensor_for(const CpuFrame& frame,
                                            const PixelRect& crop) {
    constexpr std::array<float, 3U> mean{-0.485F / 0.229F,
                                         -0.456F / 0.224F,
                                         -0.406F / 0.225F};
    constexpr std::array<float, 3U> scale{1.0F / (0.229F * 255.0F),
                                          1.0F / (0.224F * 255.0F),
                                          1.0F / (0.225F * 255.0F)};
    const std::size_t plane = detector_input_size * detector_input_size;
    std::vector<float> output(plane * 3U);
    const auto width = static_cast<std::size_t>(frame.lease.width);
    const auto height = static_cast<std::size_t>(frame.lease.height);
    const auto stride = static_cast<std::size_t>(frame.lease.stride_bytes);
    for (std::size_t y = 0U; y < detector_input_size; ++y) {
        const double source_y = crop.top +
            (static_cast<double>(y) + 0.5) * crop.height() /
                static_cast<double>(detector_input_size) - 0.5;
        const auto y0 = static_cast<std::size_t>(std::clamp(
            std::floor(source_y), 0.0, static_cast<double>(height - 1U)));
        const auto y1 = std::min(y0 + 1U, height - 1U);
        const float fy = static_cast<float>(std::clamp(source_y - std::floor(source_y),
                                                      0.0, 1.0));
        for (std::size_t x = 0U; x < detector_input_size; ++x) {
            const double source_x = crop.left +
                (static_cast<double>(x) + 0.5) * crop.width() /
                    static_cast<double>(detector_input_size) - 0.5;
            const auto x0 = static_cast<std::size_t>(std::clamp(
                std::floor(source_x), 0.0, static_cast<double>(width - 1U)));
            const auto x1 = std::min(x0 + 1U, width - 1U);
            const float fx = static_cast<float>(std::clamp(source_x - std::floor(source_x),
                                                          0.0, 1.0));
            const auto sample = [&](const std::size_t sx, const std::size_t sy,
                                    const std::size_t channel) {
                return static_cast<float>(frame.bgra[sy * stride + sx * 4U + channel]);
            };
            const std::size_t destination = y * detector_input_size + x;
            for (std::size_t rgb = 0U; rgb < 3U; ++rgb) {
                const std::size_t bgra_channel = 2U - rgb;
                const float top = sample(x0, y0, bgra_channel) * (1.0F - fx) +
                                  sample(x1, y0, bgra_channel) * fx;
                const float bottom = sample(x0, y1, bgra_channel) * (1.0F - fx) +
                                     sample(x1, y1, bgra_channel) * fx;
                const float value = top * (1.0F - fy) + bottom * fy;
                output[rgb * plane + destination] = value * scale[rgb] + mean[rgb];
            }
        }
    }
    return output;
}

struct DetectorInput {
    std::vector<float> tensor;
    double pixels_to_model{};
    double model_padding_x{};
    double model_padding_y{};
};

struct YuNetInput {
    std::vector<float> tensor;
    std::uint32_t content_width{};
    std::uint32_t content_height{};
};

[[nodiscard]] YuNetInput yunet_tensor_for(const CpuFrame& frame) {
    const auto source_width = static_cast<std::size_t>(frame.lease.width);
    const auto source_height = static_cast<std::size_t>(frame.lease.height);
    const auto stride = static_cast<std::size_t>(frame.lease.stride_bytes);
    const double scale = std::min(
        static_cast<double>(yunet_input_size) / source_width,
        static_cast<double>(yunet_input_size) / source_height);
    YuNetInput result{};
    result.content_width = static_cast<std::uint32_t>(std::clamp(
        std::llround(source_width * scale), 1LL,
        static_cast<long long>(yunet_input_size)));
    result.content_height = static_cast<std::uint32_t>(std::clamp(
        std::llround(source_height * scale), 1LL,
        static_cast<long long>(yunet_input_size)));
    const std::size_t plane = yunet_input_size * yunet_input_size;
    // OpenCV's official YuNet preprocessing is raw BGR float planar data. The
    // pinned graph has a fixed 640x640 input, so the aspect-preserving OpenCV
    // input is placed at the top left and the unused grid rows/columns remain
    // zero. This is convolutionally identical to the smaller OpenCV DNN input
    // away from its padded boundary and preserves source x/y scaling.
    result.tensor.assign(plane * 3U, 0.0F);
    for (std::size_t y = 0U; y < result.content_height; ++y) {
        const double source_y =
            (static_cast<double>(y) + 0.5) * source_height / result.content_height - 0.5;
        const auto y0 = static_cast<std::size_t>(std::clamp(
            std::floor(source_y), 0.0, static_cast<double>(source_height - 1U)));
        const auto y1 = std::min(y0 + 1U, source_height - 1U);
        const float fy = static_cast<float>(std::clamp(
            source_y - std::floor(source_y), 0.0, 1.0));
        for (std::size_t x = 0U; x < result.content_width; ++x) {
            const double source_x =
                (static_cast<double>(x) + 0.5) * source_width / result.content_width - 0.5;
            const auto x0 = static_cast<std::size_t>(std::clamp(
                std::floor(source_x), 0.0, static_cast<double>(source_width - 1U)));
            const auto x1 = std::min(x0 + 1U, source_width - 1U);
            const float fx = static_cast<float>(std::clamp(
                source_x - std::floor(source_x), 0.0, 1.0));
            const auto sample = [&](const std::size_t sx, const std::size_t sy,
                                    const std::size_t channel) {
                return static_cast<float>(frame.bgra[sy * stride + sx * 4U + channel]);
            };
            const std::size_t destination = y * yunet_input_size + x;
            for (std::size_t bgr = 0U; bgr < 3U; ++bgr) {
                const float top = sample(x0, y0, bgr) * (1.0F - fx) +
                                  sample(x1, y0, bgr) * fx;
                const float bottom = sample(x0, y1, bgr) * (1.0F - fx) +
                                     sample(x1, y1, bgr) * fx;
                result.tensor[bgr * plane + destination] =
                    top * (1.0F - fy) + bottom * fy;
            }
        }
    }
    return result;
}

[[nodiscard]] DetectorInput detector_tensor_for(const CpuFrame& frame,
                                                const PixelRect& crop) {
    constexpr std::array<float, 3U> mean{-0.485F / 0.229F,
                                         -0.456F / 0.224F,
                                         -0.406F / 0.225F};
    constexpr std::array<float, 3U> scale{1.0F / (0.229F * 255.0F),
                                          1.0F / (0.224F * 255.0F),
                                          1.0F / (0.225F * 255.0F)};
    const double model_size = static_cast<double>(detector_input_size);
    DetectorInput result{};
    result.pixels_to_model = std::min(model_size / crop.width(),
                                      model_size / crop.height());
    const double content_width = crop.width() * result.pixels_to_model;
    const double content_height = crop.height() * result.pixels_to_model;
    result.model_padding_x = (model_size - content_width) * 0.5;
    result.model_padding_y = (model_size - content_height) * 0.5;

    const std::size_t plane = detector_input_size * detector_input_size;
    // Zero after channel normalization is a neutral mean-color letterbox. It
    // retains every pixel in the caller-authorized seed without stretching or
    // searching outside it.
    result.tensor.assign(plane * 3U, 0.0F);
    const auto width = static_cast<std::size_t>(frame.lease.width);
    const auto height = static_cast<std::size_t>(frame.lease.height);
    const auto stride = static_cast<std::size_t>(frame.lease.stride_bytes);
    for (std::size_t y = 0U; y < detector_input_size; ++y) {
        const double model_y = static_cast<double>(y) + 0.5;
        if (model_y < result.model_padding_y ||
            model_y >= result.model_padding_y + content_height) {
            continue;
        }
        const double source_y = crop.top +
            (model_y - result.model_padding_y) / result.pixels_to_model - 0.5;
        const auto y0 = static_cast<std::size_t>(std::clamp(
            std::floor(source_y), 0.0, static_cast<double>(height - 1U)));
        const auto y1 = std::min(y0 + 1U, height - 1U);
        const float fy = static_cast<float>(std::clamp(
            source_y - std::floor(source_y), 0.0, 1.0));
        for (std::size_t x = 0U; x < detector_input_size; ++x) {
            const double model_x = static_cast<double>(x) + 0.5;
            if (model_x < result.model_padding_x ||
                model_x >= result.model_padding_x + content_width) {
                continue;
            }
            const double source_x = crop.left +
                (model_x - result.model_padding_x) / result.pixels_to_model - 0.5;
            const auto x0 = static_cast<std::size_t>(std::clamp(
                std::floor(source_x), 0.0, static_cast<double>(width - 1U)));
            const auto x1 = std::min(x0 + 1U, width - 1U);
            const float fx = static_cast<float>(std::clamp(
                source_x - std::floor(source_x), 0.0, 1.0));
            const auto sample = [&](const std::size_t sx, const std::size_t sy,
                                    const std::size_t channel) {
                return static_cast<float>(frame.bgra[sy * stride + sx * 4U + channel]);
            };
            const std::size_t destination = y * detector_input_size + x;
            for (std::size_t rgb = 0U; rgb < 3U; ++rgb) {
                const std::size_t bgra_channel = 2U - rgb;
                const float top = sample(x0, y0, bgra_channel) * (1.0F - fx) +
                                  sample(x1, y0, bgra_channel) * fx;
                const float bottom = sample(x0, y1, bgra_channel) * (1.0F - fx) +
                                     sample(x1, y1, bgra_channel) * fx;
                const float value = top * (1.0F - fy) + bottom * fy;
                result.tensor[rgb * plane + destination] =
                    value * scale[rgb] + mean[rgb];
            }
        }
    }
    return result;
}

[[nodiscard]] double clamped_probability(const float value) noexcept {
    return std::clamp(static_cast<double>(value), 0.0, 1.0);
}

class WindowsOrtLandmarkProviderV1 final : public NativeLandmarkProviderV1 {
public:
    explicit WindowsOrtLandmarkProviderV1(YuNetTrackingPolicyV1 yunet_tracking) noexcept
        : yunet_tracking_(yunet_tracking) {}

    ~WindowsOrtLandmarkProviderV1() override { unload(); }

    [[nodiscard]] bool load(const AdmittedLandmarkProviderLaunchV1& launch,
                            const std::uint64_t generation,
                            std::string& failure) override {
        unload();
        if (generation == 0U || !validate_landmark_provider_launch_v1(launch) ||
            !validate_yunet_tracking_policy_v1(yunet_tracking_)) {
            failure = "provider_launch_invalid";
            return false;
        }
        auto detector = verify_and_lock_file(launch.detector_model,
                                             launch.detector_size_bytes,
                                             launch.detector_sha256);
        auto landmark = verify_and_lock_file(launch.landmark_model,
                                             launch.landmark_size_bytes,
                                             launch.landmark_sha256);
        auto runtime = verify_and_lock_file(launch.runtime_library,
                                            launch.runtime_size_bytes,
                                            launch.runtime_sha256);
        auto runtime_shared = verify_and_lock_file(launch.runtime_shared_library,
                                                   launch.runtime_shared_size_bytes,
                                                   launch.runtime_shared_sha256);
        if (!detector || !landmark || !runtime || !runtime_shared) {
            failure = "provider_file_authority_mismatch";
            return false;
        }

        module_.reset(LoadLibraryExW(launch.runtime_library.c_str(), nullptr,
                                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR |
                                    LOAD_LIBRARY_SEARCH_SYSTEM32));
        if (!module_) {
            failure = "provider_runtime_load_failed";
            return false;
        }
        const auto get_base = reinterpret_cast<OrtGetApiBaseFn>(
            GetProcAddress(module_.get(), "OrtGetApiBase"));
        const OrtApiBaseV22* base = get_base ? get_base() : nullptr;
        const char* version = base && base->get_version_string
            ? base->get_version_string()
            : nullptr;
        api_ = base && base->get_api ? base->get_api(ort_api_version_v22) : nullptr;
        if (!api_ || !version || std::string_view(version) != admitted_openseeface_runtime_revision_v1 ||
            !required_api_present()) {
            failure = "provider_runtime_abi_mismatch";
            unload();
            return false;
        }

        if (!call(api_function<CreateEnvFn>(api_, ApiSlot::create_env)(
                      ort_logging_warning, "npc-mouth-worker", &environment_), failure,
                  "provider_environment_create_failed")) {
            unload();
            return false;
        }
        OrtSessionOptions* options{};
        if (!call(api_function<CreateSessionOptionsFn>(api_, ApiSlot::create_session_options)(
                      &options), failure, "provider_session_options_failed") || !options) {
            unload();
            return false;
        }
        const auto release_options = api_function<ReleaseSessionOptionsFn>(
            api_, ApiSlot::release_session_options);
        const bool configured =
            call(api_function<SetSessionExecutionModeFn>(
                     api_, ApiSlot::set_session_execution_mode)(options, ort_sequential),
                 failure, "provider_execution_mode_failed") &&
            call(api_function<SetSessionGraphOptimizationLevelFn>(
                     api_, ApiSlot::set_session_graph_optimization_level)(options,
                                                                          ort_enable_all),
                 failure, "provider_graph_optimization_failed") &&
            call(api_function<SetThreadCountFn>(api_, ApiSlot::set_intra_op_num_threads)(
                     options, 1), failure, "provider_intra_threads_failed") &&
            call(api_function<SetThreadCountFn>(api_, ApiSlot::set_inter_op_num_threads)(
                     options, 1), failure, "provider_inter_threads_failed");
        bool sessions_created = false;
        if (configured) {
            sessions_created =
                call(api_function<CreateSessionFn>(api_, ApiSlot::create_session)(
                         environment_, launch.detector_model.c_str(), options,
                         &detector_session_), failure, "provider_detector_session_failed") &&
                call(api_function<CreateSessionFn>(api_, ApiSlot::create_session)(
                         environment_, launch.landmark_model.c_str(), options,
                         &landmark_session_), failure, "provider_landmark_session_failed");
        }
        release_options(options);
        if (!sessions_created || !detector_session_ || !landmark_session_ ||
            !call(api_function<CreateCpuMemoryInfoFn>(api_, ApiSlot::create_cpu_memory_info)(
                      ort_arena_allocator, ort_mem_type_default, &memory_info_), failure,
                  "provider_memory_info_failed")) {
            unload();
            return false;
        }

        detector_file_ = std::move(*detector);
        landmark_file_ = std::move(*landmark);
        runtime_file_ = std::move(*runtime);
        runtime_shared_file_ = std::move(*runtime_shared);
        detector_kind_ = launch.pack_id == admitted_yunet_openseeface_pack_id_v1
            ? DetectorKind::yunet640
            : DetectorKind::mnv3;
        generation_ = generation;
        provider_instance_id_ = provider_instance_from_digest(launch.measured_envelope_sha256);
        loaded_ = provider_instance_id_ != 0U;
        if (!loaded_) {
            failure = "provider_instance_invalid";
            unload();
        }
        return loaded_;
    }

    [[nodiscard]] std::optional<OpenSeeFaceLandmarkPacketV1> infer(
        const LandmarkInferenceWorkV1& work,
        const Nanoseconds now_ns,
        std::string& failure) override {
        if (!loaded_ || work.track.cancellation_generation != generation_ ||
            now_ns <= 0 || now_ns > work.deadline_ns) {
            failure = "provider_work_cancelled_or_stale";
            return std::nullopt;
        }
        last_diagnostics_ = {};
        const PixelRect requested_seed = normalized_to_pixels(
            work.seed_face_bounds, work.source.lease.width, work.source.lease.height);
        if (!valid_pixel_rect(requested_seed, work.source.lease.width,
                              work.source.lease.height)) {
            failure = "provider_seed_roi_invalid";
            return std::nullopt;
        }
        const auto generation_before = generation_;
        std::optional<std::pair<PixelRect, double>> detection;
        std::optional<LandmarkDecode> decoded;
        NormalizedRect packet_face{};
        double detector_confidence{};

        const auto run_detector = [&](const PixelRect& seed) {
            last_diagnostics_.detector_ran = true;
            const Nanoseconds started = monotonic_ns();
            auto value = detect(work.source, seed, failure);
            last_diagnostics_.detector_ms +=
                static_cast<double>(monotonic_ns() - started) / 1'000'000.0;
            return value;
        };
        const auto run_landmarks = [&](const PixelRect& face) {
            ++last_diagnostics_.landmark_runs;
            const Nanoseconds started = monotonic_ns();
            auto value = landmarks(work.source, face, failure);
            last_diagnostics_.landmark_ms +=
                static_cast<double>(monotonic_ns() - started) / 1'000'000.0;
            return value;
        };
        const auto normalized_face = [&](const PixelRect& face) {
            const double width = static_cast<double>(work.source.lease.width);
            const double height = static_cast<double>(work.source.lease.height);
            return NormalizedRect{face.left / width, face.top / height,
                                  face.width() / width, face.height() / height};
        };
        const auto pixel_face = [&](const NormalizedRect& face) {
            return normalized_to_pixels(face, work.source.lease.width,
                                        work.source.lease.height);
        };
        const auto acceptable_landmarks = [](const LandmarkDecode& value) {
            return value.mean_confidence >= 0.62 && value.visibility_ratio >= 0.80;
        };

        if (detector_kind_ != DetectorKind::yunet640) {
            detection = run_detector(requested_seed);
            if (!detection) return std::nullopt;
            decoded = run_landmarks(detection->first);
            if (!decoded) return std::nullopt;
            packet_face = normalized_face(detection->first);
            detector_confidence = detection->second;
        } else {
            if (tracked_yunet_ &&
                (tracked_yunet_->track != work.track ||
                 tracked_yunet_->device_generation != work.frame.device_generation ||
                 tracked_yunet_->geometry_epoch != work.frame.geometry_epoch ||
                 tracked_yunet_->last_frame_sequence >= work.frame.sequence)) {
                tracked_yunet_.reset();
            }
            const auto action = choose_yunet_tracking_action_v1(
                tracked_yunet_.has_value(),
                tracked_yunet_ ? tracked_yunet_->frames_since_detector : 0U,
                yunet_tracking_);
            if (action == YuNetTrackingActionV1::reacquire_face) {
                last_diagnostics_.detector_refresh_due = tracked_yunet_.has_value();
                const NormalizedRect identity_seed = tracked_yunet_
                    ? tracked_yunet_->face_bounds
                    : work.seed_face_bounds;
                detection = run_detector(pixel_face(identity_seed));
                if (!detection) {
                    tracked_yunet_.reset();
                    return std::nullopt;
                }
                packet_face = normalized_face(detection->first);
                if (tracked_yunet_ && !yunet_face_matches_locked_actor_v1(
                        packet_face, tracked_yunet_->face_bounds)) {
                    failure = "provider_reacquired_face_identity_mismatch";
                    tracked_yunet_.reset();
                    return std::nullopt;
                }
                decoded = run_landmarks(detection->first);
                if (!decoded || !acceptable_landmarks(*decoded)) {
                    if (decoded) failure = "provider_reacquired_landmark_quality_low";
                    tracked_yunet_.reset();
                    return std::nullopt;
                }
                detector_confidence = detection->second;
                tracked_yunet_ = TrackedYuNetFace{
                    work.track, packet_face, decoded->points, detector_confidence, 1U,
                    work.frame.sequence, work.frame.device_generation,
                    work.frame.geometry_epoch};
            } else {
                last_diagnostics_.used_tracked_roi = true;
                const auto locked_face = tracked_yunet_->face_bounds;
                decoded = run_landmarks(pixel_face(locked_face));
                const auto tracked_update = decoded && acceptable_landmarks(*decoded)
                    ? update_yunet_tracked_face_v1(
                          locked_face, tracked_yunet_->landmarks, decoded->points)
                    : std::nullopt;
                if (!tracked_update) {
                    last_diagnostics_.quality_reacquisition = true;
                    failure.clear();
                    detection = run_detector(pixel_face(locked_face));
                    if (!detection) {
                        tracked_yunet_.reset();
                        return std::nullopt;
                    }
                    packet_face = normalized_face(detection->first);
                    if (!yunet_face_matches_locked_actor_v1(packet_face, locked_face)) {
                        failure = "provider_reacquired_face_identity_mismatch";
                        tracked_yunet_.reset();
                        return std::nullopt;
                    }
                    decoded = run_landmarks(detection->first);
                    if (!decoded || !acceptable_landmarks(*decoded)) {
                        if (decoded) failure = "provider_reacquired_landmark_quality_low";
                        tracked_yunet_.reset();
                        return std::nullopt;
                    }
                    detector_confidence = detection->second;
                    tracked_yunet_ = TrackedYuNetFace{
                        work.track, packet_face, decoded->points, detector_confidence, 1U,
                        work.frame.sequence, work.frame.device_generation,
                        work.frame.geometry_epoch};
                } else {
                    packet_face = tracked_update->face_bounds;
                    detector_confidence = tracked_yunet_->detector_confidence;
                    tracked_yunet_->face_bounds = packet_face;
                    tracked_yunet_->landmarks = decoded->points;
                    ++tracked_yunet_->frames_since_detector;
                    tracked_yunet_->last_frame_sequence = work.frame.sequence;
                }
            }
        }
        const Nanoseconds completed = monotonic_ns();
        if (generation_ != generation_before || generation_before != work.track.cancellation_generation ||
            completed < work.frame.captured_at_ns || completed > work.deadline_ns) {
            failure = "provider_result_cancelled_or_stale";
            return std::nullopt;
        }

        OpenSeeFaceLandmarkPacketV1 packet{};
        packet.provider_instance_id = provider_instance_id_;
        packet.track = work.track;
        packet.frame = work.frame;
        packet.source_frame_qpc = work.source_frame_qpc;
        packet.qpc_frequency = work.qpc_frequency;
        packet.face_bounds = packet_face;
        packet.landmarks = decoded->points;
        packet.detector_confidence = detector_confidence;
        packet.landmark_confidence = decoded->mean_confidence;
        packet.visibility_ratio = decoded->visibility_ratio;
        packet.pose = decoded->pose;
        packet.mouth_occluded = decoded->mouth_occluded;
        packet.measured_at_ns = completed;
        return packet;
    }

    [[nodiscard]] bool cancel_to(const std::uint64_t generation) noexcept override {
        if (!loaded_ || generation <= generation_) return false;
        generation_ = generation;
        tracked_yunet_.reset();
        return true;
    }

    void unload() noexcept override {
        loaded_ = false;
        generation_ = 0U;
        provider_instance_id_ = 0U;
        if (api_) {
            if (landmark_session_) {
                api_function<ReleaseSessionFn>(api_, ApiSlot::release_session)(landmark_session_);
            }
            if (detector_session_) {
                api_function<ReleaseSessionFn>(api_, ApiSlot::release_session)(detector_session_);
            }
            if (memory_info_) {
                api_function<ReleaseMemoryInfoFn>(api_, ApiSlot::release_memory_info)(memory_info_);
            }
            if (environment_) {
                api_function<ReleaseEnvFn>(api_, ApiSlot::release_env)(environment_);
            }
        }
        landmark_session_ = nullptr;
        detector_session_ = nullptr;
        memory_info_ = nullptr;
        environment_ = nullptr;
        api_ = nullptr;
        module_.reset();
        detector_file_ = {};
        landmark_file_ = {};
        runtime_file_ = {};
        runtime_shared_file_ = {};
        detector_kind_ = DetectorKind::mnv3;
        tracked_yunet_.reset();
        last_diagnostics_ = {};
    }

    [[nodiscard]] bool loaded() const noexcept override { return loaded_; }

    [[nodiscard]] LandmarkProviderInferenceDiagnosticsV1
    last_inference_diagnostics() const noexcept override {
        return last_diagnostics_;
    }

private:
    struct LandmarkDecode {
        std::array<NormalizedLandmark, openseeface_landmark_count_v1> points{};
        HeadPoseDegrees pose{};
        double mean_confidence{};
        double visibility_ratio{};
        bool mouth_occluded{};
    };

    struct TrackedYuNetFace {
        TrackBinding track;
        NormalizedRect face_bounds;
        std::array<NormalizedLandmark, openseeface_landmark_count_v1> landmarks{};
        double detector_confidence{};
        std::uint32_t frames_since_detector{};
        std::uint64_t last_frame_sequence{};
        std::uint64_t device_generation{};
        std::uint64_t geometry_epoch{};
    };

    enum class DetectorKind : std::uint8_t {
        mnv3,
        yunet640,
    };

    [[nodiscard]] bool required_api_present() const noexcept {
        constexpr std::array<ApiSlot, 24U> required{
            ApiSlot::get_error_message, ApiSlot::create_env, ApiSlot::create_session,
            ApiSlot::run, ApiSlot::create_session_options,
            ApiSlot::set_session_execution_mode,
            ApiSlot::set_session_graph_optimization_level,
            ApiSlot::set_intra_op_num_threads, ApiSlot::set_inter_op_num_threads,
            ApiSlot::create_tensor_with_data, ApiSlot::is_tensor,
            ApiSlot::get_tensor_mutable_data, ApiSlot::get_tensor_element_type,
            ApiSlot::get_dimensions_count, ApiSlot::get_dimensions,
            ApiSlot::get_tensor_shape_element_count, ApiSlot::get_tensor_type_and_shape,
            ApiSlot::create_cpu_memory_info, ApiSlot::release_env,
            ApiSlot::release_status, ApiSlot::release_memory_info,
            ApiSlot::release_session, ApiSlot::release_value,
            ApiSlot::release_tensor_type_and_shape_info,
        };
        return std::all_of(required.begin(), required.end(), [&](const ApiSlot slot) {
            return api_->slots[static_cast<std::size_t>(slot)] != nullptr;
        }) && api_->slots[static_cast<std::size_t>(ApiSlot::release_session_options)] != nullptr;
    }

    [[nodiscard]] bool call(OrtStatus* status,
                            std::string& failure,
                            const std::string_view fallback) const {
        if (!status) return true;
        const auto message = api_function<GetErrorMessageFn>(api_, ApiSlot::get_error_message)(
            status);
        failure.assign(fallback);
        if (message && *message) {
            failure.append(":");
            failure.append(std::string_view(message).substr(0U, 160U));
        }
        api_function<ReleaseStatusFn>(api_, ApiSlot::release_status)(status);
        return false;
    }

    [[nodiscard]] static std::uint64_t provider_instance_from_digest(
        const std::string_view digest) noexcept {
        std::uint64_t value{};
        const auto parsed = std::from_chars(digest.data(), digest.data() + 16U, value, 16);
        return parsed.ec == std::errc{} && parsed.ptr == digest.data() + 16U && value != 0U
            ? value
            : 0U;
    }

    [[nodiscard]] bool validate_tensor(OrtValue* value,
                                       const std::span<const std::int64_t> expected,
                                       float*& data,
                                       std::string& failure) const {
        int is_tensor{};
        OrtTensorTypeAndShapeInfo* shape{};
        int element_type{};
        std::size_t count{};
        std::size_t dimensions{};
        std::vector<std::int64_t> actual;
        void* raw{};
        bool accepted = value &&
            call(api_function<IsTensorFn>(api_, ApiSlot::is_tensor)(value, &is_tensor), failure,
                 "provider_output_type_failed") && is_tensor == 1 &&
            call(api_function<GetTensorTypeAndShapeFn>(
                     api_, ApiSlot::get_tensor_type_and_shape)(value, &shape), failure,
                 "provider_output_shape_failed") && shape &&
            call(api_function<GetTensorElementTypeFn>(api_, ApiSlot::get_tensor_element_type)(
                     shape, &element_type), failure, "provider_output_element_type_failed") &&
            element_type == onnx_tensor_float &&
            call(api_function<GetDimensionsCountFn>(api_, ApiSlot::get_dimensions_count)(
                     shape, &dimensions), failure, "provider_output_rank_failed");
        if (accepted) {
            actual.resize(dimensions);
            accepted = dimensions == expected.size() &&
                call(api_function<GetDimensionsFn>(api_, ApiSlot::get_dimensions)(
                         shape, actual.data(), actual.size()), failure,
                     "provider_output_dimensions_failed") &&
                std::equal(actual.begin(), actual.end(), expected.begin()) &&
                call(api_function<GetTensorShapeElementCountFn>(
                         api_, ApiSlot::get_tensor_shape_element_count)(shape, &count), failure,
                     "provider_output_count_failed") &&
                count == std::accumulate(expected.begin(), expected.end(), std::size_t{1U},
                    [](const std::size_t product, const std::int64_t dimension) {
                        return product * static_cast<std::size_t>(dimension);
                    }) &&
                call(api_function<GetTensorMutableDataFn>(api_, ApiSlot::get_tensor_mutable_data)(
                         value, &raw), failure, "provider_output_data_failed") && raw;
        }
        if (shape) {
            api_function<ReleaseTensorTypeAndShapeInfoFn>(
                api_, ApiSlot::release_tensor_type_and_shape_info)(shape);
        }
        data = accepted ? static_cast<float*>(raw) : nullptr;
        if (!accepted && failure.empty()) failure = "provider_output_contract_mismatch";
        return accepted;
    }

    [[nodiscard]] std::optional<std::pair<PixelRect, double>> detect(
        const CpuFrame& frame,
        const PixelRect& seed,
        std::string& failure) const {
        return detector_kind_ == DetectorKind::yunet640
            ? detect_yunet(frame, seed, failure)
            : detect_mnv3(frame, seed, failure);
    }

    [[nodiscard]] std::optional<std::pair<PixelRect, double>> detect_yunet(
        const CpuFrame& frame,
        const PixelRect& seed,
        std::string& failure) const {
        auto prepared = yunet_tensor_for(frame);
        const std::array<std::int64_t, 4U> input_shape{1, 3, 640, 640};
        OrtValue* input{};
        if (!call(api_function<CreateTensorWithDataFn>(api_, ApiSlot::create_tensor_with_data)(
                      memory_info_, prepared.tensor.data(),
                      prepared.tensor.size() * sizeof(float), input_shape.data(),
                      input_shape.size(), onnx_tensor_float, &input), failure,
                  "provider_detector_input_failed") || !input) {
            return std::nullopt;
        }
        const char* input_names[]{"input"};
        const char* output_names[]{
            "cls_8", "cls_16", "cls_32", "obj_8", "obj_16", "obj_32",
            "bbox_8", "bbox_16", "bbox_32", "kps_8", "kps_16", "kps_32",
        };
        const OrtValue* inputs[]{input};
        std::array<OrtValue*, 12U> outputs{};
        const bool ran = call(api_function<RunFn>(api_, ApiSlot::run)(
                                  detector_session_, nullptr, input_names, inputs, 1U,
                                  output_names, outputs.size(), outputs.data()), failure,
                              "provider_detector_run_failed");
        api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(input);
        if (!ran) {
            for (auto* output : outputs) if (output) {
                api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(output);
            }
            return std::nullopt;
        }

        constexpr std::array<std::uint32_t, 3U> strides{8U, 16U, 32U};
        constexpr std::array<std::size_t, 3U> cells{6'400U, 1'600U, 400U};
        std::array<float*, 3U> classes{};
        std::array<float*, 3U> objects{};
        std::array<float*, 3U> boxes{};
        std::array<float*, 3U> keypoints{};
        bool valid = true;
        for (std::size_t level = 0U; level < strides.size(); ++level) {
            const std::array<std::int64_t, 3U> scalar_shape{
                1, static_cast<std::int64_t>(cells[level]), 1};
            const std::array<std::int64_t, 3U> box_shape{
                1, static_cast<std::int64_t>(cells[level]), 4};
            const std::array<std::int64_t, 3U> keypoint_shape{
                1, static_cast<std::int64_t>(cells[level]), 10};
            valid = validate_tensor(outputs[level], scalar_shape, classes[level], failure) &&
                    validate_tensor(outputs[3U + level], scalar_shape, objects[level], failure) &&
                    validate_tensor(outputs[6U + level], box_shape, boxes[level], failure) &&
                    validate_tensor(outputs[9U + level], keypoint_shape,
                                    keypoints[level], failure) && valid;
        }

        std::optional<std::pair<PixelRect, double>> result;
        if (valid) {
            std::array<YuNetDetectorLevelV1, 3U> levels{};
            for (std::size_t level = 0U; level < levels.size(); ++level) {
                const auto grid = static_cast<std::uint32_t>(yunet_input_size / strides[level]);
                levels[level] = {
                    strides[level], grid, grid,
                    std::span<const float>(classes[level], cells[level]),
                    std::span<const float>(objects[level], cells[level]),
                    std::span<const float>(boxes[level], cells[level] * 4U),
                };
            }
            const NormalizedRect normalized_seed{
                seed.left / frame.lease.width,
                seed.top / frame.lease.height,
                seed.width() / frame.lease.width,
                seed.height() / frame.lease.height,
            };
            const auto selected = decode_and_select_yunet_face_v1(
                levels, static_cast<std::uint32_t>(yunet_input_size),
                static_cast<std::uint32_t>(yunet_input_size), prepared.content_width,
                prepared.content_height, frame.lease.width, frame.lease.height,
                normalized_seed);
            if (selected) {
                PixelRect box{
                    selected->bounds.x * frame.lease.width,
                    selected->bounds.y * frame.lease.height,
                    selected->bounds.right() * frame.lease.width,
                    selected->bounds.bottom() * frame.lease.height,
                };
                if (valid_pixel_rect(box, frame.lease.width, frame.lease.height)) {
                    result = std::pair{box, selected->confidence};
                }
            }
        }
        for (auto* output : outputs) if (output) {
            api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(output);
        }
        if (!result && failure.empty()) failure = "provider_face_not_found_in_seed_roi";
        return result;
    }

    [[nodiscard]] std::optional<std::pair<PixelRect, double>> detect_mnv3(
        const CpuFrame& frame,
        const PixelRect& seed,
        std::string& failure) const {
        auto prepared = detector_tensor_for(frame, seed);
        const std::array<std::int64_t, 4U> input_shape{1, 3, 224, 224};
        OrtValue* input{};
        if (!call(api_function<CreateTensorWithDataFn>(api_, ApiSlot::create_tensor_with_data)(
                      memory_info_, prepared.tensor.data(),
                      prepared.tensor.size() * sizeof(float),
                      input_shape.data(), input_shape.size(), onnx_tensor_float, &input), failure,
                  "provider_detector_input_failed") || !input) {
            return std::nullopt;
        }
        const char* input_names[]{"input"};
        const char* output_names[]{"output", "maxpool"};
        const OrtValue* inputs[]{input};
        std::array<OrtValue*, 2U> outputs{};
        const bool ran = call(api_function<RunFn>(api_, ApiSlot::run)(
                                  detector_session_, nullptr, input_names, inputs, 1U,
                                  output_names, outputs.size(), outputs.data()), failure,
                              "provider_detector_run_failed");
        api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(input);
        if (!ran) {
            for (auto* output : outputs) if (output) {
                api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(output);
            }
            return std::nullopt;
        }
        float* output_data{};
        float* maxpool_data{};
        const std::array<std::int64_t, 4U> output_shape{1, 2, 56, 56};
        // OpenSeeFace's pinned MNV3 detector retains both output channels in
        // the max-pool tensor. Detection intentionally consumes channel zero,
        // but the ONNX Runtime contract must validate the complete tensor.
        const std::array<std::int64_t, 4U> maxpool_shape{1, 2, 56, 56};
        const bool valid = validate_tensor(outputs[0], output_shape, output_data, failure) &&
                           validate_tensor(outputs[1], maxpool_shape, maxpool_data, failure);
        std::optional<std::pair<PixelRect, double>> result;
        if (valid) {
            const std::size_t cells = detector_grid_size * detector_grid_size;
            float best = -std::numeric_limits<float>::infinity();
            std::size_t best_index{};
            for (std::size_t index = 0U; index < cells; ++index) {
                const float score = output_data[index];
                if (std::isfinite(score) && score == maxpool_data[index] && score > best) {
                    best = score;
                    best_index = index;
                }
            }
            const float radius = output_data[cells + best_index] * 112.0F;
            if (best >= detector_threshold && std::isfinite(radius) && radius >= 2.0F) {
                const double grid_x = static_cast<double>(best_index % detector_grid_size) * 4.0;
                const double grid_y = static_cast<double>(best_index / detector_grid_size) * 4.0;
                const auto to_source_x = [&](const double model_x) {
                    return seed.left +
                        (model_x - prepared.model_padding_x) /
                            prepared.pixels_to_model;
                };
                const auto to_source_y = [&](const double model_y) {
                    return seed.top +
                        (model_y - prepared.model_padding_y) /
                            prepared.pixels_to_model;
                };
                PixelRect box{to_source_x(grid_x - radius),
                              to_source_y(grid_y - radius),
                              to_source_x(grid_x + radius),
                              to_source_y(grid_y + radius)};
                box.left = std::clamp(box.left, seed.left, seed.right);
                box.top = std::clamp(box.top, seed.top, seed.bottom);
                box.right = std::clamp(box.right, seed.left, seed.right);
                box.bottom = std::clamp(box.bottom, seed.top, seed.bottom);
                if (valid_pixel_rect(box, frame.lease.width, frame.lease.height)) {
                    result = std::pair{box, clamped_probability(best)};
                }
            }
        }
        for (auto* output : outputs) if (output) {
            api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(output);
        }
        if (!result && failure.empty()) failure = "provider_face_not_found_in_seed_roi";
        return result;
    }

    [[nodiscard]] std::optional<LandmarkDecode> landmarks(
        const CpuFrame& frame,
        const PixelRect& face,
        std::string& failure) const {
        PixelRect crop{
            std::max(0.0, std::floor(face.left - face.width() * 0.10)),
            std::max(0.0, std::floor(face.top - face.height() * 0.125)),
            std::min(static_cast<double>(frame.lease.width),
                     std::floor(face.right + face.width() * 0.10)),
            std::min(static_cast<double>(frame.lease.height),
                     std::floor(face.bottom + face.height() * 0.125)),
        };
        if (!valid_pixel_rect(crop, frame.lease.width, frame.lease.height)) {
            failure = "provider_landmark_crop_invalid";
            return std::nullopt;
        }
        auto input_data = tensor_for(frame, crop);
        const std::array<std::int64_t, 4U> input_shape{1, 3, 224, 224};
        OrtValue* input{};
        if (!call(api_function<CreateTensorWithDataFn>(api_, ApiSlot::create_tensor_with_data)(
                      memory_info_, input_data.data(), input_data.size() * sizeof(float),
                      input_shape.data(), input_shape.size(), onnx_tensor_float, &input), failure,
                  "provider_landmark_input_failed") || !input) {
            return std::nullopt;
        }
        const char* input_names[]{"input"};
        const char* output_names[]{"output"};
        const OrtValue* inputs[]{input};
        OrtValue* output{};
        const bool ran = call(api_function<RunFn>(api_, ApiSlot::run)(
                                  landmark_session_, nullptr, input_names, inputs, 1U,
                                  output_names, 1U, &output), failure,
                              "provider_landmark_run_failed");
        api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(input);
        float* data{};
        const std::array<std::int64_t, 4U> output_shape{1, 198, 28, 28};
        if (!ran || !validate_tensor(output, output_shape, data, failure)) {
            if (output) api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(output);
            return std::nullopt;
        }
        LandmarkDecode result{};
        constexpr std::size_t cells = landmark_grid_size * landmark_grid_size;
        double confidence_sum{};
        double mouth_confidence_sum{};
        std::size_t visible{};
        bool finite = true;
        for (std::size_t landmark = 0U; landmark < openseeface_landmark_count_v1; ++landmark) {
            const float* heatmap = data + landmark * cells;
            const auto maximum = std::max_element(heatmap, heatmap + cells);
            const std::size_t index = static_cast<std::size_t>(maximum - heatmap);
            const float raw_offset_x = std::clamp(
                data[(66U + landmark) * cells + index], 1.0e-7F, 0.9999999F);
            const float raw_offset_y = std::clamp(
                data[(132U + landmark) * cells + index], 1.0e-7F, 0.9999999F);
            const double offset_x = 223.0 * std::log(
                static_cast<double>(raw_offset_x) / (1.0 - raw_offset_x)) / 16.0;
            const double offset_y = 223.0 * std::log(
                static_cast<double>(raw_offset_y) / (1.0 - raw_offset_y)) / 16.0;
            const double image_y = crop.top + crop.height() / 224.0 *
                (223.0 * static_cast<double>(index / landmark_grid_size) / 27.0 + offset_x);
            const double image_x = crop.left + crop.width() / 224.0 *
                (223.0 * static_cast<double>(index % landmark_grid_size) / 27.0 + offset_y);
            const double confidence = clamped_probability(*maximum);
            const bool in_frame = std::isfinite(image_x) && std::isfinite(image_y) &&
                image_x >= 0.0 && image_y >= 0.0 &&
                image_x <= static_cast<double>(frame.lease.width) &&
                image_y <= static_cast<double>(frame.lease.height);
            finite = finite && in_frame;
            result.points[landmark] = {
                std::clamp(image_x / static_cast<double>(frame.lease.width), 0.0, 1.0),
                std::clamp(image_y / static_cast<double>(frame.lease.height), 0.0, 1.0),
                confidence,
            };
            confidence_sum += confidence;
            if (confidence >= landmark_visibility_threshold && in_frame) ++visible;
            if (landmark >= 48U) mouth_confidence_sum += confidence;
        }
        api_function<ReleaseValueFn>(api_, ApiSlot::release_value)(output);
        if (!finite) {
            failure = "provider_landmark_nonfinite_or_out_of_frame";
            return std::nullopt;
        }
        result.mean_confidence = confidence_sum /
                                 static_cast<double>(openseeface_landmark_count_v1);
        result.visibility_ratio = static_cast<double>(visible) /
                                  static_cast<double>(openseeface_landmark_count_v1);
        const double mouth_confidence = mouth_confidence_sum / 18.0;
        const auto eye_center = [&](const std::size_t first) {
            double x{};
            double y{};
            for (std::size_t index = first; index < first + 6U; ++index) {
                x += result.points[index].x;
                y += result.points[index].y;
            }
            return std::pair{x / 6.0, y / 6.0};
        };
        const auto left_eye = eye_center(36U);
        const auto right_eye = eye_center(42U);
        const double eye_dx = right_eye.first - left_eye.first;
        const double eye_dy = right_eye.second - left_eye.second;
        const double eye_distance = std::hypot(eye_dx, eye_dy);
        const double eye_mid_x = (left_eye.first + right_eye.first) * 0.5;
        const double yaw = eye_distance > 1.0e-6
            ? std::clamp((result.points[30U].x - eye_mid_x) / eye_distance * 35.0,
                         -60.0, 60.0)
            : 999.0;
        const double roll = std::atan2(eye_dy, eye_dx) * 180.0 /
                            std::numbers::pi_v<double>;
        result.pose = {yaw, 0.0, roll};
        result.mouth_occluded = mouth_confidence < landmark_visibility_threshold ||
                                eye_distance <= 1.0e-6 || std::abs(yaw) > 35.0 ||
                                std::abs(roll) > 45.0;
        return result;
    }

    bool loaded_{};
    std::uint64_t generation_{};
    std::uint64_t provider_instance_id_{};
    DetectorKind detector_kind_{DetectorKind::mnv3};
    YuNetTrackingPolicyV1 yunet_tracking_{};
    std::optional<TrackedYuNetFace> tracked_yunet_;
    LandmarkProviderInferenceDiagnosticsV1 last_diagnostics_{};
    UniqueModule module_;
    const OrtApiSlots* api_{};
    OrtEnv* environment_{};
    OrtSession* detector_session_{};
    OrtSession* landmark_session_{};
    OrtMemoryInfo* memory_info_{};
    VerifiedFile detector_file_;
    VerifiedFile landmark_file_;
    VerifiedFile runtime_file_;
    VerifiedFile runtime_shared_file_;
};

} // namespace

std::unique_ptr<NativeLandmarkProviderV1> make_windows_ort_landmark_provider_v1(
    const YuNetTrackingPolicyV1 yunet_tracking) {
    return std::make_unique<WindowsOrtLandmarkProviderV1>(yunet_tracking);
}

} // namespace npc::mouth

#else

namespace npc::mouth {
std::unique_ptr<NativeLandmarkProviderV1> make_windows_ort_landmark_provider_v1(
    const YuNetTrackingPolicyV1) {
    return {};
}
} // namespace npc::mouth

#endif
