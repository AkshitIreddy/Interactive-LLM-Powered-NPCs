#pragma once

#include "npc/mouth_worker/signal_adapter.hpp"
#include "npc/mouth_worker/worker.hpp"

#include <optional>
#include <string_view>

namespace npc::mouth {

struct ProductRequestIdentity {
    std::uint64_t request_id{};
    std::uint64_t session_id_high{};
    std::uint64_t session_id_low{};
    std::uint64_t turn_id_high{};
    std::uint64_t turn_id_low{};
    std::uint64_t sentence_id{};

    [[nodiscard]] constexpr bool operator==(const ProductRequestIdentity&) const noexcept = default;
};

struct ProductFrameRequest {
    ProductRequestIdentity identity;
    CpuFrame source;
    OpenSeeFaceLandmarkPacketV1 landmarks;
    AppearanceGateEvidenceV1 appearance;
    VisualResourceStateV1 resources;
    MouthDrive drive;
    Nanoseconds deadline_ns{};
};

enum class PresentationDisposition : std::uint8_t {
    queued,
    replaced_before_processing,
    residual_proposed,
    bypassed_signal,
    bypassed_worker,
    cancelled,
};

[[nodiscard]] constexpr std::string_view to_string(
    const PresentationDisposition value) noexcept {
    switch (value) {
    case PresentationDisposition::queued: return "queued";
    case PresentationDisposition::replaced_before_processing: return "replaced_before_processing";
    case PresentationDisposition::residual_proposed: return "residual_proposed";
    case PresentationDisposition::bypassed_signal: return "bypassed_signal";
    case PresentationDisposition::bypassed_worker: return "bypassed_worker";
    case PresentationDisposition::cancelled: return "cancelled";
    }
    return "unknown";
}

struct PresentationReceiptV1 {
    std::uint32_t schema_version{1};
    ProductRequestIdentity request;
    TrackBinding track;
    FrameIdentity source_frame;
    PresentationDisposition disposition{PresentationDisposition::bypassed_worker};
    SignalDisposition signal_disposition{SignalDisposition::bypass_invalid_packet};
    Disposition worker_disposition{Disposition::bypass_no_work};
    DriveKind drive_kind{DriveKind::explicit_coefficients};
    std::uint32_t admitted_signal_rate_hz{};
    std::uint64_t latch_generation{};
    Nanoseconds submitted_at_ns{};
    Nanoseconds completed_at_ns{};
    std::uint64_t queue_replacements{};
    std::optional<ResidualPatch> residual;

    [[nodiscard]] bool proposed_residual() const noexcept {
        return disposition == PresentationDisposition::residual_proposed && residual.has_value();
    }
};

struct ProductSubmissionResult {
    PresentationReceiptV1 receipt;
    std::optional<PresentationReceiptV1> replaced;
};

// Product-level queue/receipt owner. It couples the exact selected actor and
// appearance evidence to one source frame and one audio clock, while leaving
// GPU import/presentation authority to the media broker.
class MouthProductRuntime final {
public:
    explicit MouthProductRuntime(std::uint64_t initial_generation = 1,
                                 WorkerPolicy worker_policy = {},
                                 OpenSeeFaceAdapterPolicy adapter_policy = {});

    [[nodiscard]] ProductSubmissionResult submit(ProductFrameRequest request,
                                                 const FrameIdentity& current_frame,
                                                 Nanoseconds now_ns);

    [[nodiscard]] PresentationReceiptV1 process_latest(const FrameIdentity& current_frame,
                                                       Nanoseconds now_ns);

    [[nodiscard]] std::optional<PresentationReceiptV1> cancel_to(
        std::uint64_t new_generation,
        Nanoseconds now_ns) noexcept;

    [[nodiscard]] std::uint64_t active_generation() const noexcept;
    [[nodiscard]] const WorkerStats& worker_stats() const noexcept;

private:
    struct PendingReceipt {
        ProductRequestIdentity request;
        TrackBinding track;
        FrameIdentity frame;
        DriveKind drive_kind{DriveKind::explicit_coefficients};
        std::uint32_t admitted_signal_rate_hz{};
        std::uint64_t latch_generation{};
        Nanoseconds submitted_at_ns{};
    };

    [[nodiscard]] PresentationReceiptV1 make_signal_bypass(
        const ProductFrameRequest& request,
        const SignalDecision& signal,
        Nanoseconds now_ns) const;
    [[nodiscard]] PresentationReceiptV1 make_pending_disposition(
        const PendingReceipt& pending,
        PresentationDisposition disposition,
        Disposition worker_disposition,
        Nanoseconds now_ns) const;

    OpenSeeFaceSignalAdapter adapter_;
    ReferenceMouthWorker worker_;
    std::optional<PendingReceipt> pending_;
};

} // namespace npc::mouth
