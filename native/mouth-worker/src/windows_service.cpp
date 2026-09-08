#include "npc/mouth_worker/windows_service.hpp"

#ifdef _WIN32

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <bcrypt.h>
#include <d3d11_1.h>
#include <dxgi1_2.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cwctype>
#include <filesystem>
#include <limits>
#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace npc::mouth::service {
namespace {

using Microsoft::WRL::ComPtr;

constexpr DWORD keyed_mutex_timeout_ms = 2U;
constexpr std::size_t maximum_retained_residuals = 8U;

class UniqueHandle final {
public:
    UniqueHandle() = default;
    explicit UniqueHandle(HANDLE value) : value_(value) {}
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
        const HANDLE result = value_;
        value_ = nullptr;
        return result;
    }
    void reset(HANDLE value = nullptr) noexcept {
        if (value_ && value_ != INVALID_HANDLE_VALUE) CloseHandle(value_);
        value_ = value;
    }

private:
    HANDLE value_{};
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

[[nodiscard]] std::uint64_t pack_file_time(const FILETIME value) noexcept {
    return (static_cast<std::uint64_t>(value.dwHighDateTime) << 32U) |
           static_cast<std::uint64_t>(value.dwLowDateTime);
}

[[nodiscard]] std::wstring lowercase(std::wstring value) {
    std::transform(value.begin(), value.end(), value.begin(),
                   [](const wchar_t character) {
                       return static_cast<wchar_t>(std::towlower(character));
                   });
    return value;
}

[[nodiscard]] bool process_identity_matches(const std::uint32_t process_id,
                                            const std::uint64_t creation_time,
                                            const std::string& executable_name) {
    if (process_id == 0U || creation_time == 0U || executable_name.empty() ||
        executable_name.find('/') != std::string::npos ||
        executable_name.find('\\') != std::string::npos) {
        return false;
    }
    UniqueHandle process(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                                     FALSE, process_id));
    if (!process || WaitForSingleObject(process.get(), 0U) == WAIT_OBJECT_0) return false;
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(process.get(), &created, &exited, &kernel, &user) ||
        pack_file_time(created) != creation_time) {
        return false;
    }
    std::array<wchar_t, 32'768> path{};
    DWORD size = static_cast<DWORD>(path.size());
    if (!QueryFullProcessImageNameW(process.get(), 0U, path.data(), &size) || size == 0U) {
        return false;
    }
    std::wstring expected(executable_name.begin(), executable_name.end());
    return lowercase(std::filesystem::path(std::wstring(path.data(), size)).filename().wstring()) ==
           lowercase(expected);
}

[[nodiscard]] bool fill_random(std::span<std::byte> destination) noexcept {
    return BCryptGenRandom(nullptr, reinterpret_cast<PUCHAR>(destination.data()),
                           static_cast<ULONG>(destination.size()),
                           BCRYPT_USE_SYSTEM_PREFERRED_RNG) == 0;
}

[[nodiscard]] bool nonzero_nonce(const std::uint64_t high,
                                 const std::uint64_t low) noexcept {
    return high != 0U || low != 0U;
}

[[nodiscard]] std::uint64_t adapter_luid_value(const LUID luid) noexcept {
    return static_cast<std::uint64_t>(static_cast<std::uint32_t>(luid.LowPart)) |
           (static_cast<std::uint64_t>(static_cast<std::uint32_t>(luid.HighPart)) << 32U);
}

[[nodiscard]] bool read_exact(const HANDLE pipe, const std::span<std::byte> destination) {
    std::size_t offset = 0U;
    while (offset < destination.size()) {
        DWORD transferred{};
        const DWORD requested = static_cast<DWORD>(std::min<std::size_t>(
            destination.size() - offset, std::numeric_limits<DWORD>::max()));
        if (!ReadFile(pipe, destination.data() + offset, requested, &transferred, nullptr) ||
            transferred == 0U) {
            return false;
        }
        offset += transferred;
    }
    return true;
}

[[nodiscard]] bool write_exact(const HANDLE pipe, const std::span<const std::byte> source) {
    std::size_t offset = 0U;
    while (offset < source.size()) {
        DWORD transferred{};
        const DWORD requested = static_cast<DWORD>(std::min<std::size_t>(
            source.size() - offset, std::numeric_limits<DWORD>::max()));
        if (!WriteFile(pipe, source.data() + offset, requested, &transferred, nullptr) ||
            transferred == 0U) {
            return false;
        }
        offset += transferred;
    }
    return FlushFileBuffers(pipe) != FALSE;
}

class WindowsResidualRenderer final {
public:
    struct RetainedResidual {
        std::uint64_t nonce_high{};
        std::uint64_t nonce_low{};
        Nanoseconds expires_at_ns{};
        UniqueHandle shared_handle;
        ComPtr<ID3D11Texture2D> texture;
    };

    [[nodiscard]] bool read_source(const SourceTextureLeaseV1& source,
                                   const FrameIdentity& frame,
                                   CpuFrame& output,
                                   std::string& failure) {
        cleanup(monotonic_ns());
        const auto& lease = source.texture;
        if (source.schema_version != 1U || lease.schema_version != 1U ||
            lease.transport != LeaseTransport::d3d11_shared_nt_handle ||
            !nonzero_nonce(lease.lease_nonce_high, lease.lease_nonce_low) ||
            lease.owner_process_id != source.broker_process_id ||
            lease.intended_consumer_process_id != GetCurrentProcessId() ||
            lease.native_handle_value == 0U || lease.keyed_mutex_acquire_key == 0U ||
            lease.keyed_mutex_release_key == 0U ||
            lease.keyed_mutex_acquire_key == lease.keyed_mutex_release_key ||
            lease.width == 0U || lease.height == 0U ||
            lease.stride_bytes < lease.width * 4U ||
            lease.format != PixelFormat::bgra8_unorm_premultiplied ||
            lease.expires_at_ns < monotonic_ns() || source.source_frame_qpc == 0U ||
            source.qpc_frequency == 0U || frame.sequence == 0U) {
            failure = "source_texture_contract_invalid";
            return false;
        }
        if (!process_identity_matches(source.broker_process_id,
                                      source.broker_process_creation_time,
                                      source.broker_executable_name)) {
            failure = "source_broker_identity_changed";
            return false;
        }
        if (!ensure_device(lease, failure)) return false;

        UniqueHandle source_handle(reinterpret_cast<HANDLE>(lease.native_handle_value));
        ComPtr<ID3D11Texture2D> texture;
        const HRESULT opened = device1_->OpenSharedResource1(
            source_handle.get(), IID_PPV_ARGS(&texture));
        source_handle.reset();
        if (FAILED(opened) || !texture) {
            failure = "source_texture_open_failed";
            return false;
        }
        D3D11_TEXTURE2D_DESC description{};
        texture->GetDesc(&description);
        if (description.Width != lease.width || description.Height != lease.height ||
            description.Format != DXGI_FORMAT_B8G8R8A8_UNORM || description.ArraySize != 1U ||
            description.MipLevels != 1U || description.SampleDesc.Count != 1U ||
            (description.MiscFlags & D3D11_RESOURCE_MISC_SHARED_NTHANDLE) == 0U ||
            (description.MiscFlags & D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX) == 0U) {
            failure = "source_texture_description_mismatch";
            return false;
        }
        ComPtr<IDXGIKeyedMutex> mutex;
        if (FAILED(texture.As(&mutex)) || !mutex) {
            failure = "source_texture_missing_keyed_mutex";
            return false;
        }
        D3D11_TEXTURE2D_DESC staging_description = description;
        staging_description.Usage = D3D11_USAGE_STAGING;
        staging_description.BindFlags = 0U;
        staging_description.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        staging_description.MiscFlags = 0U;
        ComPtr<ID3D11Texture2D> staging;
        if (FAILED(device_->CreateTexture2D(&staging_description, nullptr, &staging))) {
            failure = "source_staging_allocation_failed";
            return false;
        }
        if (mutex->AcquireSync(lease.keyed_mutex_acquire_key, keyed_mutex_timeout_ms) != S_OK) {
            failure = "source_keyed_mutex_timeout";
            return false;
        }
        context_->CopyResource(staging.Get(), texture.Get());
        const HRESULT released = mutex->ReleaseSync(lease.keyed_mutex_release_key);
        if (FAILED(released)) {
            failure = "source_keyed_mutex_release_failed";
            return false;
        }
        D3D11_MAPPED_SUBRESOURCE mapped{};
        if (FAILED(context_->Map(staging.Get(), 0U, D3D11_MAP_READ, 0U, &mapped)) ||
            mapped.RowPitch < lease.width * 4U) {
            failure = "source_staging_map_failed";
            return false;
        }
        output = {};
        output.identity = frame;
        output.lease = lease;
        output.lease.transport = LeaseTransport::cpu_reference;
        output.lease.native_handle_value = 0U;
        output.lease.owner_process_id = 0U;
        output.lease.intended_consumer_process_id = 0U;
        output.lease.keyed_mutex_acquire_key = 0U;
        output.lease.keyed_mutex_release_key = 0U;
        output.lease.stride_bytes = lease.width * 4U;
        output.bgra.resize(static_cast<std::size_t>(output.lease.stride_bytes) * lease.height);
        for (std::uint32_t row = 0U; row < lease.height; ++row) {
            std::memcpy(output.bgra.data() + static_cast<std::size_t>(row) * output.lease.stride_bytes,
                        static_cast<const std::byte*>(mapped.pData) +
                            static_cast<std::size_t>(row) * mapped.RowPitch,
                        output.lease.stride_bytes);
        }
        context_->Unmap(staging.Get(), 0U);
        return true;
    }

    [[nodiscard]] std::optional<TextureLeaseDescriptor> retain_residual(
        const ResidualPatch& patch,
        const SourceTextureLeaseV1& source,
        std::string& failure) {
        cleanup(monotonic_ns());
        if (!device_ || patch.premultiplied_bgra.empty() || patch.width == 0U ||
            patch.height == 0U || patch.stride_bytes != patch.width * 4U ||
            retained_.size() >= maximum_retained_residuals) {
            failure = "residual_capacity_or_payload_invalid";
            return std::nullopt;
        }
        D3D11_TEXTURE2D_DESC description{};
        description.Width = patch.width;
        description.Height = patch.height;
        description.MipLevels = 1U;
        description.ArraySize = 1U;
        description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        description.SampleDesc.Count = 1U;
        description.Usage = D3D11_USAGE_DEFAULT;
        description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        description.MiscFlags = D3D11_RESOURCE_MISC_SHARED_NTHANDLE |
                                D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX;
        ComPtr<ID3D11Texture2D> texture;
        if (FAILED(device_->CreateTexture2D(&description, nullptr, &texture))) {
            failure = "residual_texture_allocation_failed";
            return std::nullopt;
        }
        ComPtr<IDXGIKeyedMutex> mutex;
        if (FAILED(texture.As(&mutex)) || !mutex || mutex->AcquireSync(0U, 0U) != S_OK) {
            failure = "residual_keyed_mutex_initialization_failed";
            return std::nullopt;
        }
        context_->UpdateSubresource(texture.Get(), 0U, nullptr, patch.premultiplied_bgra.data(),
                                    patch.stride_bytes, 0U);
        if (FAILED(mutex->ReleaseSync(1U))) {
            failure = "residual_keyed_mutex_publish_failed";
            return std::nullopt;
        }
        ComPtr<IDXGIResource1> resource;
        HANDLE shared_handle{};
        if (FAILED(texture.As(&resource)) || !resource ||
            FAILED(resource->CreateSharedHandle(nullptr,
                                                DXGI_SHARED_RESOURCE_READ |
                                                    DXGI_SHARED_RESOURCE_WRITE,
                                                nullptr, &shared_handle)) ||
            !shared_handle) {
            failure = "residual_shared_handle_creation_failed";
            return std::nullopt;
        }
        std::array<std::byte, 16> random{};
        if (!fill_random(random)) {
            CloseHandle(shared_handle);
            failure = "residual_nonce_generation_failed";
            return std::nullopt;
        }
        std::uint64_t nonce_high{};
        std::uint64_t nonce_low{};
        std::memcpy(&nonce_high, random.data(), sizeof(nonce_high));
        std::memcpy(&nonce_low, random.data() + sizeof(nonce_high), sizeof(nonce_low));
        if (!nonzero_nonce(nonce_high, nonce_low)) nonce_low = 1U;
        const Nanoseconds expires = std::min(source.texture.expires_at_ns,
                                             monotonic_ns() + 80'000'000);
        retained_.push_back({nonce_high, nonce_low, expires,
                             UniqueHandle(shared_handle), texture});
        TextureLeaseDescriptor lease{};
        lease.schema_version = 1U;
        lease.transport = LeaseTransport::d3d11_shared_nt_handle;
        lease.lease_nonce_high = nonce_high;
        lease.lease_nonce_low = nonce_low;
        lease.owner_process_id = GetCurrentProcessId();
        lease.intended_consumer_process_id = source.broker_process_id;
        lease.native_handle_value = reinterpret_cast<std::uint64_t>(shared_handle);
        lease.adapter_luid_low = source.texture.adapter_luid_low;
        lease.adapter_luid_high = source.texture.adapter_luid_high;
        lease.keyed_mutex_acquire_key = 1U;
        lease.keyed_mutex_release_key = 2U;
        lease.width = patch.width;
        lease.height = patch.height;
        lease.stride_bytes = patch.stride_bytes;
        lease.format = PixelFormat::bgra8_unorm_premultiplied;
        lease.expires_at_ns = expires;
        return lease;
    }

    [[nodiscard]] bool acknowledge(const std::uint64_t nonce_high,
                                   const std::uint64_t nonce_low) noexcept {
        const auto iterator = std::find_if(retained_.begin(), retained_.end(),
            [&](const RetainedResidual& value) {
                return value.nonce_high == nonce_high && value.nonce_low == nonce_low;
            });
        if (iterator == retained_.end()) return false;
        retained_.erase(iterator);
        return true;
    }

    void cancel_all() noexcept { retained_.clear(); }

private:
    [[nodiscard]] bool ensure_device(const TextureLeaseDescriptor& lease,
                                     std::string& failure) {
        const std::uint64_t requested_luid = static_cast<std::uint64_t>(lease.adapter_luid_low) |
            (static_cast<std::uint64_t>(static_cast<std::uint32_t>(lease.adapter_luid_high)) << 32U);
        if (device_ && adapter_luid_ == requested_luid) return true;
        cancel_all();
        context_.Reset();
        device1_.Reset();
        device_.Reset();
        ComPtr<IDXGIFactory1> factory;
        if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory)))) {
            failure = "dxgi_factory_unavailable";
            return false;
        }
        ComPtr<IDXGIAdapter1> selected;
        for (UINT index = 0U;; ++index) {
            ComPtr<IDXGIAdapter1> adapter;
            if (factory->EnumAdapters1(index, &adapter) == DXGI_ERROR_NOT_FOUND) break;
            DXGI_ADAPTER_DESC1 description{};
            if (SUCCEEDED(adapter->GetDesc1(&description)) &&
                adapter_luid_value(description.AdapterLuid) == requested_luid) {
                selected = std::move(adapter);
                break;
            }
        }
        if (!selected) {
            failure = "source_adapter_not_found";
            return false;
        }
        D3D_FEATURE_LEVEL selected_level{};
        constexpr std::array levels{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
        UINT flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        if (FAILED(D3D11CreateDevice(selected.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, flags,
                                     levels.data(), static_cast<UINT>(levels.size()),
                                     D3D11_SDK_VERSION, &device_, &selected_level, &context_)) ||
            FAILED(device_.As(&device1_)) || !device1_) {
            failure = "source_adapter_device_creation_failed";
            return false;
        }
        adapter_luid_ = requested_luid;
        return true;
    }

    void cleanup(const Nanoseconds now_ns) {
        retained_.erase(std::remove_if(retained_.begin(), retained_.end(),
            [&](const RetainedResidual& value) { return value.expires_at_ns < now_ns; }),
            retained_.end());
    }

    std::uint64_t adapter_luid_{};
    ComPtr<ID3D11Device> device_;
    ComPtr<ID3D11Device1> device1_;
    ComPtr<ID3D11DeviceContext> context_;
    std::vector<RetainedResidual> retained_;
};

[[nodiscard]] WorkerResponseV1 base_response(const EnvelopeV1& envelope,
                                             const StatusCode status,
                                             const std::uint64_t generation,
                                             std::string detail = {}) {
    WorkerResponseV1 response{};
    response.status = status;
    response.response_to_sequence = envelope.sequence;
    response.cancellation_generation = generation;
    response.detail = std::move(detail);
    return response;
}

void render_resolved_current_frame(MouthProductRuntime& runtime,
                                   WindowsResidualRenderer& renderer,
                                   WorkerResponseV1& response,
                                   const ProductRequestIdentity& identity,
                                   const SourceTextureLeaseV1& source_lease,
                                   const TrackBinding& track,
                                   const FrameIdentity& frame,
                                   OpenSeeFaceLandmarkPacketV1 landmarks,
                                   const AppearanceGateEvidenceV1& appearance,
                                   const VisualResourceStateV1& resources,
                                   const MouthDrive& drive,
                                   CpuFrame source,
                                   const Nanoseconds deadline_ns,
                                   const bool sealed_click_spatial_authority,
                                   const Nanoseconds now) {
    const double detector_confidence = landmarks.detector_confidence;
    const double landmark_confidence = landmarks.landmark_confidence;
    const double visibility_ratio = landmarks.visibility_ratio;
    const bool mouth_occluded = landmarks.mouth_occluded;
    const Nanoseconds landmarks_measured_at_ns = landmarks.measured_at_ns;
    double minimum_point_confidence = 1.0;
    std::size_t invalid_point_count{};
    double mouth_left = 1.0;
    double mouth_top = 1.0;
    double mouth_right{};
    double mouth_bottom{};
    for (std::size_t index = 0; index < landmarks.landmarks.size(); ++index) {
        const auto& point = landmarks.landmarks[index];
        minimum_point_confidence = std::min(minimum_point_confidence, point.confidence);
        if (!std::isfinite(point.x) || !std::isfinite(point.y) ||
            !std::isfinite(point.confidence) || point.x < 0.0 || point.x > 1.0 ||
            point.y < 0.0 || point.y > 1.0 || point.confidence < 0.0 ||
            point.confidence > 1.0) {
            ++invalid_point_count;
        }
        if (index >= 48U) {
            mouth_left = std::min(mouth_left, point.x);
            mouth_top = std::min(mouth_top, point.y);
            mouth_right = std::max(mouth_right, point.x);
            mouth_bottom = std::max(mouth_bottom, point.y);
        }
    }
    const double upper_lip_y = (landmarks.landmarks[59U].y + landmarks.landmarks[60U].y +
                                landmarks.landmarks[61U].y) / 3.0;
    const double lower_lip_y = (landmarks.landmarks[63U].y + landmarks.landmarks[64U].y +
                                landmarks.landmarks[65U].y) / 3.0;
    const std::string packet_metrics =
        ":det=" + std::to_string(detector_confidence) +
        ":lm=" + std::to_string(landmark_confidence) +
        ":vis=" + std::to_string(visibility_ratio) +
        ":minpt=" + std::to_string(minimum_point_confidence) +
        ":badpt=" + std::to_string(invalid_point_count) +
        ":face=" + std::to_string(landmarks.face_bounds.x) + "," +
        std::to_string(landmarks.face_bounds.y) + "," +
        std::to_string(landmarks.face_bounds.width) + "," +
        std::to_string(landmarks.face_bounds.height) +
        ":mouth=" + std::to_string(mouth_left) + "," + std::to_string(mouth_top) + "," +
        std::to_string(mouth_right) + "," + std::to_string(mouth_bottom) +
        ":corners=" + std::to_string(landmarks.landmarks[58U].x) + "," +
        std::to_string(landmarks.landmarks[62U].x) +
        ":lips_y=" + std::to_string(upper_lip_y) + "," + std::to_string(lower_lip_y);
    const auto audio_skew_ns = drive.clock.playback_at_ns >= frame.captured_at_ns
        ? drive.clock.playback_at_ns - frame.captured_at_ns
        : frame.captured_at_ns - drive.clock.playback_at_ns;
    const std::string timing_metrics =
        ":frameage_ns=" + std::to_string(now - frame.captured_at_ns) +
        ":trackage_ns=" + std::to_string(now - landmarks_measured_at_ns) +
        ":audioskew_ns=" + std::to_string(audio_skew_ns);
    ProductFrameRequest request{};
    request.identity = identity;
    request.source = std::move(source);
    request.landmarks = std::move(landmarks);
    request.appearance = appearance;
    request.resources = resources;
    request.drive = drive;
    request.deadline_ns = deadline_ns;
    request.sealed_click_spatial_authority = sealed_click_spatial_authority;
    auto submission = runtime.submit(std::move(request), frame, now);
    if (submission.receipt.disposition != PresentationDisposition::queued) {
        response.receipt = std::move(submission.receipt);
        response.detail = "no_residual:" +
            std::string(to_string(response.receipt.signal_disposition));
        if (response.receipt.signal_disposition == SignalDisposition::bypass_invalid_packet) {
            response.detail += packet_metrics;
        }
        if (response.receipt.signal_disposition == SignalDisposition::bypass_stale) {
            response.detail += timing_metrics;
        }
        if (response.receipt.signal_disposition == SignalDisposition::bypass_unsafe_roi) {
            response.detail += packet_metrics;
        }
        return;
    }
    response.receipt = runtime.process_latest(frame, monotonic_ns());
    if (!response.receipt.proposed_residual()) {
        response.detail = "no_residual:" +
            std::string(npc::mouth::to_string(response.receipt.worker_disposition));
        if (response.receipt.worker_disposition == Disposition::bypass_audio_clock ||
            response.receipt.worker_disposition == Disposition::bypass_stale_frame ||
            response.receipt.worker_disposition == Disposition::bypass_invalid_tracking) {
            response.detail += timing_metrics;
        }
        return;
    }
    std::string failure;
    const auto texture = renderer.retain_residual(*response.receipt.residual,
                                                   source_lease, failure);
    if (!texture) {
        response.status = StatusCode::capability_unavailable;
        response.detail = std::move(failure);
        response.receipt.residual.reset();
        response.receipt.disposition = PresentationDisposition::bypassed_worker;
        return;
    }
    ResidualProposalV1 proposal{};
    proposal.schema_version = 3U;
    proposal.request = identity;
    proposal.track = track;
    proposal.source_frame = frame;
    proposal.source_frame_qpc = source_lease.source_frame_qpc;
    proposal.normalized_bounds = response.receipt.residual->normalized_bounds;
    proposal.residual = *texture;
    proposal.confidence = landmark_confidence;
    proposal.detector_confidence = detector_confidence;
    proposal.landmark_confidence = landmark_confidence;
    proposal.visibility_ratio = visibility_ratio;
    proposal.mouth_occluded = mouth_occluded;
    proposal.landmarks_measured_at_ns = landmarks_measured_at_ns;
    proposal.audio_clock = response.receipt.residual->audio_clock;
    proposal.produced_at_ns = response.receipt.completed_at_ns;
    response.residual = std::move(proposal);
    response.receipt.residual.reset();
    response.detail = "residual_proposed_pending_broker_presentation";
}

} // namespace

int run_windows_service(const WindowsServiceConfig& config,
                        std::unique_ptr<NativeLandmarkProviderV1> landmark_provider) {
    if (config.pipe_name.rfind(L"\\\\.\\pipe\\", 0U) != 0U ||
        config.pipe_name.size() > 240U || config.expected_controller_process_id == 0U ||
        config.initial_generation == 0U) {
        return 2;
    }
    UniqueHandle pipe(CreateNamedPipeW(
        config.pipe_name.c_str(), PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
        1U, maximum_message_bytes + 4U, maximum_message_bytes + 4U, 0U, nullptr));
    if (!pipe) return 3;
    if (!ConnectNamedPipe(pipe.get(), nullptr) && GetLastError() != ERROR_PIPE_CONNECTED) return 4;
    ULONG client_pid{};
    if (!GetNamedPipeClientProcessId(pipe.get(), &client_pid) ||
        client_pid != config.expected_controller_process_id) {
        return 5;
    }

    EnvelopeValidator validator({config.session, 5'000'000'000});
    MouthProductRuntime runtime(config.initial_generation);
    WindowsResidualRenderer renderer;
    std::unique_ptr<NativeLandmarkCoordinatorV1> landmark_coordinator;
    if (landmark_provider) {
        landmark_coordinator =
            std::make_unique<NativeLandmarkCoordinatorV1>(std::move(landmark_provider));
        if (config.landmark_provider_launch) {
            const auto loaded = landmark_coordinator->load(*config.landmark_provider_launch,
                                                           config.initial_generation,
                                                           monotonic_ns());
            if (loaded.disposition != LandmarkProviderDispositionV1::ready) {
                const auto ignored = landmark_coordinator->unload(monotonic_ns());
                (void)ignored;
            }
        }
    }
    bool clean_shutdown = false;

    for (;;) {
        std::array<std::byte, 4> prefix{};
        if (!read_exact(pipe.get(), prefix)) break;
        const auto size = decode_frame_size(prefix);
        if (!size) break;
        std::vector<std::byte> message(*size);
        if (!read_exact(pipe.get(), message)) break;
        const auto envelope = decode_envelope(message);
        if (!envelope) break;
        const Nanoseconds now = monotonic_ns();
        const StatusCode validation = validator.validate(*envelope, now, runtime.active_generation());
        WorkerResponseV1 response = base_response(*envelope, validation, runtime.active_generation());
        bool should_shutdown = false;
        if (validation == StatusCode::ok) {
            switch (envelope->command) {
            case CommandKind::health:
                response.detail = "ready";
                break;
            case CommandKind::render_current_frame: {
                const auto command = decode_render_command(envelope->payload);
                if (!command || command->source.session.nonce != config.session.nonce ||
                    command->source.session.session_id_high != config.session.session_id_high ||
                    command->source.session.session_id_low != config.session.session_id_low ||
                    command->track != command->landmarks.track || command->track.actor_id == 0U ||
                    command->frame != command->landmarks.frame ||
                    command->source.source_frame_qpc != command->landmarks.source_frame_qpc ||
                    command->source.qpc_frequency != command->landmarks.qpc_frequency ||
                    command->frame.sequence == 0U || command->deadline_ns < now) {
                    response.status = StatusCode::payload_invalid;
                    response.detail = "render_contract_invalid";
                    break;
                }
                CpuFrame source;
                std::string failure;
                if (!renderer.read_source(command->source, command->frame, source, failure)) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = std::move(failure);
                    break;
                }
                render_resolved_current_frame(runtime, renderer, response, command->request,
                                              command->source, command->track, command->frame,
                                              command->landmarks, command->appearance,
                                               command->resources, command->drive,
                                               std::move(source), command->deadline_ns, false, now);
                break;
            }
            case CommandKind::render_with_admitted_landmarks: {
                const auto command = decode_admitted_render_command(envelope->payload);
                if (!command || !landmark_coordinator ||
                    command->source.session.nonce != config.session.nonce ||
                    command->source.session.session_id_high != config.session.session_id_high ||
                    command->source.session.session_id_low != config.session.session_id_low ||
                    command->track.actor_id == 0U || command->frame.sequence == 0U ||
                    command->deadline_ns < now) {
                    response.status = landmark_coordinator
                        ? StatusCode::payload_invalid
                        : StatusCode::capability_unavailable;
                    response.detail = landmark_coordinator
                        ? "admitted_render_contract_invalid"
                        : "admitted_landmark_provider_unavailable";
                    break;
                }
                CpuFrame source;
                std::string failure;
                if (!renderer.read_source(command->source, command->frame, source, failure)) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = std::move(failure);
                    break;
                }
                LandmarkInferenceWorkV1 work{};
                work.track = command->track;
                work.frame = command->frame;
                work.source_frame_qpc = command->source.source_frame_qpc;
                work.qpc_frequency = command->source.qpc_frequency;
                work.seed_face_bounds = command->seed_face_bounds;
                work.source = std::move(source);
                work.deadline_ns = command->deadline_ns;
                const auto queued = landmark_coordinator->submit(std::move(work), now);
                if (queued.disposition != LandmarkProviderDispositionV1::ready) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = "landmark_provider_bypass:" + queued.detail;
                    break;
                }
                auto produced = landmark_coordinator->process_latest(monotonic_ns());
                if (!produced.produced_packet() || !produced.completed_work) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = "landmark_provider_bypass:" + produced.detail;
                    break;
                }
                auto resolved_work = std::move(*produced.completed_work);
                const Nanoseconds resolved_now = monotonic_ns();
                render_resolved_current_frame(
                    runtime, renderer, response, command->request, command->source,
                    command->track, command->frame, std::move(*produced.packet),
                    command->appearance, command->resources, command->drive,
                    std::move(resolved_work.source), command->deadline_ns,
                    command->sealed_click_spatial_authority, resolved_now);
                break;
            }
            case CommandKind::discover_actor_candidates: {
                const auto command = decode_actor_candidate_discovery(envelope->payload);
                if (!command || !landmark_coordinator ||
                    command->source.session.nonce != config.session.nonce ||
                    command->source.session.session_id_high != config.session.session_id_high ||
                    command->source.session.session_id_low != config.session.session_id_low ||
                    command->discovery_track.actor_id == 0U ||
                    command->discovery_track.track_id == 0U ||
                    command->discovery_track.track_epoch == 0U ||
                    command->discovery_track.cancellation_generation !=
                        runtime.active_generation() ||
                    command->frame.sequence == 0U || command->deadline_ns < now) {
                    response.status = landmark_coordinator
                        ? StatusCode::payload_invalid
                        : StatusCode::capability_unavailable;
                    response.detail = landmark_coordinator
                        ? "actor_candidate_discovery_contract_invalid"
                        : "admitted_landmark_provider_unavailable";
                    break;
                }
                CpuFrame source;
                std::string failure;
                if (!renderer.read_source(command->source, command->frame, source, failure)) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = std::move(failure);
                    break;
                }
                LandmarkInferenceWorkV1 work{};
                work.track = command->discovery_track;
                work.frame = command->frame;
                work.source_frame_qpc = command->source.source_frame_qpc;
                work.qpc_frequency = command->source.qpc_frequency;
                work.seed_face_bounds = {0.0, 0.0, 1.0, 1.0};
                work.source = std::move(source);
                work.deadline_ns = command->deadline_ns;
                const auto queued = landmark_coordinator->submit(std::move(work), now);
                if (queued.disposition != LandmarkProviderDispositionV1::ready) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = "actor_candidate_discovery_bypass:" + queued.detail;
                    break;
                }
                auto produced = landmark_coordinator->process_latest(monotonic_ns());
                if (!produced.produced_packet()) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = "actor_candidate_discovery_bypass:" + produced.detail;
                    break;
                }
                const auto& packet = *produced.packet;
                response.receipt.track = command->discovery_track;
                response.receipt.source_frame = command->frame;
                response.actor_candidates.push_back({
                    command->discovery_track.actor_id,
                    command->discovery_track.track_id,
                    command->discovery_track.track_epoch,
                    packet.face_bounds,
                    packet.detector_confidence,
                });
                response.detail = "admitted_actor_candidate_ready";
                break;
            }
            case CommandKind::configure_admitted_landmark_provider: {
                const auto command = decode_provider_configuration(envelope->payload);
                if (!command || command->launch.provider_load_self_test) {
                    response.status = StatusCode::payload_invalid;
                    response.detail = "provider_configuration_invalid";
                    break;
                }
                if (!landmark_coordinator) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = "native_landmark_provider_not_built";
                    break;
                }
                const auto loaded = landmark_coordinator->load(command->launch,
                                                               runtime.active_generation(), now);
                if (loaded.disposition != LandmarkProviderDispositionV1::ready) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = "provider_configuration_bypass:" + loaded.detail;
                    break;
                }
                response.detail = "admitted_landmark_provider_ready";
                break;
            }
            case CommandKind::self_test_admitted_landmark_provider: {
                const auto command = decode_provider_configuration(envelope->payload);
                if (!command || !command->launch.provider_load_self_test ||
                    command->launch.exact_target_process_id != 0U) {
                    response.status = StatusCode::payload_invalid;
                    response.detail = "provider_load_self_test_contract_invalid";
                    break;
                }
                if (!landmark_coordinator) {
                    response.status = StatusCode::capability_unavailable;
                    response.detail = "native_landmark_provider_not_built";
                    break;
                }
                const auto loaded = landmark_coordinator->load(
                    command->launch, runtime.active_generation(), now);
                if (loaded.disposition != LandmarkProviderDispositionV1::ready) {
                    const auto ignored = landmark_coordinator->unload(monotonic_ns());
                    (void)ignored;
                    response.status = StatusCode::capability_unavailable;
                    response.detail = "provider_load_self_test_bypass:" + loaded.detail;
                    break;
                }
                const auto unloaded = landmark_coordinator->unload(monotonic_ns());
                if (unloaded.disposition != LandmarkProviderDispositionV1::unloaded) {
                    response.status = StatusCode::internal_error;
                    response.detail = "provider_load_self_test_unload_failed";
                    break;
                }
                response.detail = "admitted_landmark_provider_load_self_test_passed";
                break;
            }
            case CommandKind::install_character_mouth_atlas: {
                auto command = decode_character_mouth_atlas(envelope->payload);
                if (!command ||
                    command->atlas.cancellation_generation != runtime.active_generation() ||
                    !runtime.install_atlas(std::move(command->atlas))) {
                    response.status = StatusCode::payload_invalid;
                    response.detail = "character_mouth_atlas_invalid";
                    break;
                }
                renderer.cancel_all();
                response.detail = "character_mouth_atlas_ready";
                break;
            }
            case CommandKind::clear_character_mouth_atlas:
                if (!envelope->payload.empty()) {
                    response.status = StatusCode::payload_invalid;
                    response.detail = "character_mouth_atlas_clear_payload_invalid";
                    break;
                }
                runtime.clear_atlas();
                renderer.cancel_all();
                response.detail = "character_mouth_atlas_cleared";
                break;
            case CommandKind::cancel_generation: {
                const auto command = decode_cancel_command(envelope->payload);
                if (!command || command->new_generation <= runtime.active_generation()) {
                    response.status = StatusCode::payload_invalid;
                    response.detail = "cancel_generation_not_monotonic";
                    break;
                }
                response.receipt = runtime.cancel_to(command->new_generation, now)
                    .value_or(PresentationReceiptV1{});
                if (landmark_coordinator) {
                    const auto ignored =
                        landmark_coordinator->cancel_to(command->new_generation, now);
                    (void)ignored;
                }
                renderer.cancel_all();
                response.cancellation_generation = runtime.active_generation();
                response.detail = "cancelled_and_residuals_released";
                break;
            }
            case CommandKind::acknowledge_residual: {
                const auto command = decode_acknowledgement(envelope->payload);
                if (!command || !renderer.acknowledge(command->lease_nonce_high,
                                                       command->lease_nonce_low)) {
                    response.status = StatusCode::payload_invalid;
                    response.detail = "residual_ack_unknown_or_replayed";
                    break;
                }
                response.detail = command->presented
                    ? "broker_presentation_acknowledged"
                    : "broker_bypass_acknowledged";
                break;
            }
            case CommandKind::shutdown:
                if (landmark_coordinator) {
                    const auto ignored = landmark_coordinator->unload(now);
                    (void)ignored;
                }
                renderer.cancel_all();
                response.detail = "shutdown";
                should_shutdown = true;
                break;
            }
        }
        const auto encoded = encode_response(response);
        if (!encoded) break;
        const auto framed = frame_message(*encoded);
        if (framed.empty() || !write_exact(pipe.get(), framed)) break;
        if (should_shutdown) {
            clean_shutdown = true;
            break;
        }
    }
    renderer.cancel_all();
    if (landmark_coordinator) {
        const auto ignored = landmark_coordinator->unload(monotonic_ns());
        (void)ignored;
    }
    DisconnectNamedPipe(pipe.get());
    return clean_shutdown ? 0 : 6;
}

} // namespace npc::mouth::service

#else

namespace npc::mouth::service {
int run_windows_service(const WindowsServiceConfig&,
                        std::unique_ptr<NativeLandmarkProviderV1>) { return 2; }
} // namespace npc::mouth::service

#endif
