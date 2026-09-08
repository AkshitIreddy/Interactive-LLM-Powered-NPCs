#include "npc/mouth_worker/product_runtime.hpp"

#include <utility>

namespace npc::mouth {
namespace {

[[nodiscard]] bool valid_request_identity(const ProductRequestIdentity& value) noexcept {
    return value.request_id != 0U &&
           (value.session_id_high != 0U || value.session_id_low != 0U) &&
           (value.turn_id_high != 0U || value.turn_id_low != 0U) &&
           value.sentence_id != 0U;
}

} // namespace

MouthProductRuntime::MouthProductRuntime(const std::uint64_t initial_generation,
                                         WorkerPolicy worker_policy,
                                         OpenSeeFaceAdapterPolicy adapter_policy)
    : adapter_(initial_generation, std::move(adapter_policy)),
      worker_(initial_generation, std::move(worker_policy)) {}

ProductSubmissionResult MouthProductRuntime::submit(ProductFrameRequest request,
                                                    const FrameIdentity& current_frame,
                                                    const Nanoseconds now_ns) {
    const auto actor = request.landmarks.track.actor_id;
    const bool actor_has_no_pack = !installed_atlas_actor_.has_value() ||
                                   *installed_atlas_actor_ != actor;
    adapter_.set_current_frame_geometry(actor_has_no_pack ||
        (current_pixel_actor_.has_value() && *current_pixel_actor_ == actor));
    const bool admitted_source_only_selection = request.sealed_click_source_only &&
                                                !installed_atlas_actor_.has_value();
    const auto signal = admitted_source_only_selection
        ? adapter_.adapt_source_only_selected(request.landmarks, request.appearance,
                                              request.resources, current_frame, now_ns)
        : adapter_.adapt(request.landmarks, request.appearance,
                         request.resources, current_frame, now_ns);
    if (!valid_request_identity(request.identity) || !signal.accepted()) {
        // A 30 Hz capture source routinely lands between the admitted 15 Hz
        // inference samples. That cadence drop does not break the audio or
        // geometry timeline, so retain schema-four shape/trajectory history.
        // An untraceable request remains a hard reset even when its signal
        // happens to be rate limited.
        if (!valid_request_identity(request.identity) ||
            signal.disposition != SignalDisposition::bypass_rate_limited) {
            worker_.reset_current_pixel_history();
        }
        auto bypass = make_signal_bypass(request, signal, now_ns);
        if (!valid_request_identity(request.identity)) {
            bypass.signal_disposition = SignalDisposition::bypass_invalid_packet;
        }
        return {std::move(bypass), std::nullopt};
    }

    std::optional<PresentationReceiptV1> replaced;
    if (pending_) {
        replaced = make_pending_disposition(*pending_,
            PresentationDisposition::replaced_before_processing,
            Disposition::bypass_no_work, now_ns);
    }

    const DriveKind drive_kind = request.drive.kind;
    WorkItem work{};
    work.track = request.landmarks.track;
    work.source = std::move(request.source);
    work.tracking = *signal.tracking;
    work.drive = std::move(request.drive);
    work.deadline_ns = request.deadline_ns;
    if (!worker_.submit(std::move(work))) {
        PresentationReceiptV1 receipt{};
        receipt.request = request.identity;
        receipt.track = request.landmarks.track;
        receipt.source_frame = request.landmarks.frame;
        receipt.disposition = PresentationDisposition::cancelled;
        receipt.signal_disposition = signal.disposition;
        receipt.worker_disposition = Disposition::bypass_cancelled;
        receipt.drive_kind = drive_kind;
        receipt.admitted_signal_rate_hz = signal.admitted_signal_rate_hz;
        receipt.latch_generation = signal.latch_generation;
        receipt.submitted_at_ns = now_ns;
        receipt.completed_at_ns = now_ns;
        receipt.queue_replacements = worker_.stats().replaced_before_processing;
        return {std::move(receipt), std::move(replaced)};
    }

    pending_ = PendingReceipt{
        request.identity,
        request.landmarks.track,
        request.landmarks.frame,
        drive_kind,
        signal.admitted_signal_rate_hz,
        signal.latch_generation,
        now_ns,
    };
    auto queued = make_pending_disposition(*pending_, PresentationDisposition::queued,
                                           Disposition::bypass_no_work, now_ns);
    queued.signal_disposition = signal.disposition;
    return {std::move(queued), std::move(replaced)};
}

PresentationReceiptV1 MouthProductRuntime::process_latest(const FrameIdentity& current_frame,
                                                          const Nanoseconds now_ns) {
    if (!pending_) {
        PresentationReceiptV1 receipt{};
        receipt.disposition = PresentationDisposition::bypassed_worker;
        receipt.worker_disposition = Disposition::bypass_no_work;
        receipt.completed_at_ns = now_ns;
        receipt.queue_replacements = worker_.stats().replaced_before_processing;
        return receipt;
    }
    const PendingReceipt pending = *pending_;
    pending_.reset();
    auto result = worker_.process_latest(current_frame, now_ns);
    auto receipt = make_pending_disposition(
        pending,
        result.has_residual() ? PresentationDisposition::residual_proposed
                              : PresentationDisposition::bypassed_worker,
        result.disposition,
        now_ns);
    receipt.signal_disposition = SignalDisposition::accepted;
    if (result.has_residual()) {
        receipt.residual = std::move(result.residual);
    }
    return receipt;
}

std::optional<PresentationReceiptV1> MouthProductRuntime::cancel_to(
    const std::uint64_t new_generation,
    const Nanoseconds now_ns) noexcept {
    if (!adapter_.cancel_to(new_generation) || !worker_.cancel_to(new_generation)) {
        return std::nullopt;
    }
    current_pixel_actor_.reset();
    installed_atlas_actor_.reset();
    adapter_.set_current_frame_geometry(false);
    if (!pending_) {
        return std::nullopt;
    }
    auto receipt = make_pending_disposition(*pending_, PresentationDisposition::cancelled,
                                            Disposition::bypass_cancelled, now_ns);
    receipt.signal_disposition = SignalDisposition::bypass_cancelled;
    pending_.reset();
    return receipt;
}

bool MouthProductRuntime::install_atlas(CharacterMouthAtlas atlas) {
    const auto actor_id = atlas.actor_id;
    const auto actor = atlas.schema_version == 4U
        ? std::optional<std::uint64_t>{atlas.actor_id} : std::nullopt;
    if (!worker_.install_atlas(std::move(atlas))) return false;
    installed_atlas_actor_ = actor_id;
    current_pixel_actor_ = actor;
    return true;
}

void MouthProductRuntime::clear_atlas() noexcept {
    worker_.clear_atlas();
    installed_atlas_actor_.reset();
    current_pixel_actor_.reset();
    adapter_.set_current_frame_geometry(false);
}

std::uint64_t MouthProductRuntime::active_generation() const noexcept {
    return worker_.active_generation();
}

const WorkerStats& MouthProductRuntime::worker_stats() const noexcept {
    return worker_.stats();
}

PresentationReceiptV1 MouthProductRuntime::make_signal_bypass(
    const ProductFrameRequest& request,
    const SignalDecision& signal,
    const Nanoseconds now_ns) const {
    PresentationReceiptV1 receipt{};
    receipt.request = request.identity;
    receipt.track = request.landmarks.track;
    receipt.source_frame = request.landmarks.frame;
    receipt.disposition = signal.disposition == SignalDisposition::bypass_cancelled
        ? PresentationDisposition::cancelled
        : PresentationDisposition::bypassed_signal;
    receipt.signal_disposition = signal.disposition;
    receipt.worker_disposition = signal.disposition == SignalDisposition::bypass_cancelled
        ? Disposition::bypass_cancelled
        : Disposition::bypass_no_work;
    receipt.drive_kind = request.drive.kind;
    receipt.admitted_signal_rate_hz = signal.admitted_signal_rate_hz;
    receipt.latch_generation = signal.latch_generation;
    receipt.submitted_at_ns = now_ns;
    receipt.completed_at_ns = now_ns;
    receipt.queue_replacements = worker_.stats().replaced_before_processing;
    return receipt;
}

PresentationReceiptV1 MouthProductRuntime::make_pending_disposition(
    const PendingReceipt& pending,
    const PresentationDisposition disposition,
    const Disposition worker_disposition,
    const Nanoseconds now_ns) const {
    PresentationReceiptV1 receipt{};
    receipt.request = pending.request;
    receipt.track = pending.track;
    receipt.source_frame = pending.frame;
    receipt.disposition = disposition;
    receipt.worker_disposition = worker_disposition;
    receipt.drive_kind = pending.drive_kind;
    receipt.admitted_signal_rate_hz = pending.admitted_signal_rate_hz;
    receipt.latch_generation = pending.latch_generation;
    receipt.submitted_at_ns = pending.submitted_at_ns;
    receipt.completed_at_ns = now_ns;
    receipt.queue_replacements = worker_.stats().replaced_before_processing;
    return receipt;
}

} // namespace npc::mouth
