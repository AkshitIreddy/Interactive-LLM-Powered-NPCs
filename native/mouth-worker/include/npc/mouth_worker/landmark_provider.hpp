#pragma once

#include "npc/mouth_worker/signal_adapter.hpp"
#include "npc/mouth_worker/worker.hpp"

#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <string_view>

namespace npc::mouth {

inline constexpr std::string_view admitted_openseeface_pack_id_v1 =
    "openseeface-mnv3-lm1-mouth-signal";
inline constexpr std::string_view admitted_openseeface_revision_v1 =
    "85aa70fc67582d046e771ea73625182a0d8f7475";
inline constexpr std::string_view admitted_openseeface_runtime_revision_v1 = "1.22.1";
inline constexpr std::string_view admitted_openseeface_backend_v1 =
    "cpu-execution-provider-one-thread";

struct AdmittedLandmarkProviderLaunchV1 {
    std::uint32_t schema_version{1};
    std::string pack_id;
    std::string pack_revision;
    std::filesystem::path artifact_root;
    std::filesystem::path detector_model;
    std::filesystem::path landmark_model;
    std::filesystem::path runtime_library;
    std::filesystem::path runtime_shared_library;
    std::uint64_t detector_size_bytes{};
    std::uint64_t landmark_size_bytes{};
    std::uint64_t runtime_size_bytes{};
    std::uint64_t runtime_shared_size_bytes{};
    std::string detector_sha256;
    std::string landmark_sha256;
    std::string runtime_sha256;
    std::string runtime_shared_sha256;
    std::string measured_envelope_sha256;
    std::string runtime_revision;
    std::string backend;
    std::uint32_t maximum_signal_rate_hz{};
    std::uint32_t inference_threads{};
    std::uint32_t exact_target_process_id{};
};

struct LandmarkInferenceWorkV1 {
    std::uint32_t schema_version{1};
    TrackBinding track;
    FrameIdentity frame;
    std::uint64_t source_frame_qpc{};
    std::uint64_t qpc_frequency{};
    NormalizedRect seed_face_bounds;
    CpuFrame source;
    Nanoseconds deadline_ns{};
};

enum class LandmarkProviderDispositionV1 : std::uint8_t {
    ready,
    packet_produced,
    replaced_before_processing,
    bypass_not_loaded,
    bypass_invalid,
    bypass_stale,
    bypass_cancelled,
    bypass_provider_failure,
    unloaded,
};

struct LandmarkProviderReceiptV1 {
    std::uint32_t schema_version{1};
    LandmarkProviderDispositionV1 disposition{LandmarkProviderDispositionV1::bypass_not_loaded};
    TrackBinding track;
    FrameIdentity frame;
    std::uint64_t source_frame_qpc{};
    std::uint64_t provider_generation{};
    std::uint64_t queue_replacements{};
    Nanoseconds submitted_at_ns{};
    Nanoseconds completed_at_ns{};
    std::optional<OpenSeeFaceLandmarkPacketV1> packet;
    // Returned only with a valid packet so the service can composite from the
    // exact same CPU frame without a second frame-sized copy or another lease.
    std::optional<LandmarkInferenceWorkV1> completed_work;
    std::string detail;

    [[nodiscard]] bool produced_packet() const noexcept {
        return disposition == LandmarkProviderDispositionV1::packet_produced && packet.has_value();
    }
};

// The real implementation dynamically loads the separately installed ORT
// runtime. Tests inject a fake through this exact boundary; the product never
// substitutes synthetic landmarks when a real provider is unavailable.
class NativeLandmarkProviderV1 {
public:
    virtual ~NativeLandmarkProviderV1() = default;
    [[nodiscard]] virtual bool load(const AdmittedLandmarkProviderLaunchV1& launch,
                                    std::uint64_t generation,
                                    std::string& failure) = 0;
    [[nodiscard]] virtual std::optional<OpenSeeFaceLandmarkPacketV1> infer(
        const LandmarkInferenceWorkV1& work,
        Nanoseconds now_ns,
        std::string& failure) = 0;
    [[nodiscard]] virtual bool cancel_to(std::uint64_t generation) noexcept = 0;
    virtual void unload() noexcept = 0;
    [[nodiscard]] virtual bool loaded() const noexcept = 0;
};

// Queue-depth-one native owner. It revalidates provider output against the
// exact leased actor/track/frame after inference, so even an authenticated
// provider cannot return a packet for another frame or generation.
class NativeLandmarkCoordinatorV1 final {
public:
    explicit NativeLandmarkCoordinatorV1(std::unique_ptr<NativeLandmarkProviderV1> provider);

    [[nodiscard]] LandmarkProviderReceiptV1 load(
        const AdmittedLandmarkProviderLaunchV1& launch,
        std::uint64_t generation,
        Nanoseconds now_ns);
    [[nodiscard]] LandmarkProviderReceiptV1 submit(LandmarkInferenceWorkV1 work,
                                                   Nanoseconds now_ns);
    [[nodiscard]] LandmarkProviderReceiptV1 process_latest(Nanoseconds now_ns);
    [[nodiscard]] LandmarkProviderReceiptV1 cancel_to(std::uint64_t generation,
                                                      Nanoseconds now_ns) noexcept;
    [[nodiscard]] LandmarkProviderReceiptV1 unload(Nanoseconds now_ns) noexcept;
    [[nodiscard]] std::uint64_t active_generation() const noexcept;
    [[nodiscard]] std::uint64_t queue_replacements() const noexcept;

private:
    struct PendingWork {
        LandmarkInferenceWorkV1 work;
        Nanoseconds submitted_at_ns{};
    };

    [[nodiscard]] LandmarkProviderReceiptV1 bypass(
        LandmarkProviderDispositionV1 disposition,
        std::string detail,
        Nanoseconds now_ns,
        const LandmarkInferenceWorkV1* work = nullptr) const;

    std::unique_ptr<NativeLandmarkProviderV1> provider_;
    std::optional<PendingWork> pending_;
    std::uint64_t generation_{};
    std::uint64_t queue_replacements_{};
};

[[nodiscard]] bool validate_landmark_provider_launch_v1(
    const AdmittedLandmarkProviderLaunchV1& launch) noexcept;
[[nodiscard]] bool validate_landmark_inference_work_v1(
    const LandmarkInferenceWorkV1& work,
    std::uint64_t generation,
    Nanoseconds now_ns) noexcept;

// Project-owned native provider. It has no linked ORT import: load() first
// re-hashes and locks the governor-authorized optional files, then resolves
// the exact pinned ORT v22 ABI from that DLL. A missing/changed runtime returns
// a visual-only provider failure.
[[nodiscard]] std::unique_ptr<NativeLandmarkProviderV1>
make_windows_ort_landmark_provider_v1();

} // namespace npc::mouth
