#include "npc/mouth_worker/landmark_provider.hpp"

#include <cmath>
#include <utility>

namespace npc::mouth {
namespace {

[[nodiscard]] bool valid_sha256(const std::string_view value) noexcept {
    if (value.size() != 64U) return false;
    for (const unsigned char byte : value) {
        if (!(byte >= '0' && byte <= '9') && !(byte >= 'a' && byte <= 'f')) return false;
    }
    return true;
}

[[nodiscard]] bool absolute_child(const std::filesystem::path& root,
                                  const std::filesystem::path& value) noexcept {
    if (!root.is_absolute() || !value.is_absolute() || value == root) return false;
    const auto relative = value.lexically_relative(root);
    if (relative.empty() || relative.is_absolute()) return false;
    for (const auto& part : relative) {
        if (part == "..") return false;
    }
    return true;
}

[[nodiscard]] bool valid_rect(const NormalizedRect& value) noexcept {
    return std::isfinite(value.x) && std::isfinite(value.y) &&
           std::isfinite(value.width) && std::isfinite(value.height) &&
           value.x >= 0.0 && value.y >= 0.0 && value.width > 0.0 &&
           value.height > 0.0 && value.right() <= 1.0 && value.bottom() <= 1.0;
}

[[nodiscard]] bool packet_matches(const OpenSeeFaceLandmarkPacketV1& packet,
                                  const LandmarkInferenceWorkV1& work,
                                  const std::uint64_t generation) noexcept {
    if (packet.schema_version != 1U || packet.provider_instance_id == 0U ||
        packet.track.cancellation_generation != generation || packet.track != work.track ||
        packet.frame != work.frame || packet.source_frame_qpc != work.source_frame_qpc ||
        packet.qpc_frequency != work.qpc_frequency ||
        packet.measured_at_ns < work.frame.captured_at_ns ||
        packet.measured_at_ns > work.deadline_ns || !valid_rect(packet.face_bounds) ||
        !std::isfinite(packet.detector_confidence) ||
        !std::isfinite(packet.landmark_confidence) ||
        packet.detector_confidence < 0.0 || packet.detector_confidence > 1.0 ||
        packet.landmark_confidence < 0.0 || packet.landmark_confidence > 1.0) {
        return false;
    }
    for (const auto& point : packet.landmarks) {
        if (!std::isfinite(point.x) || !std::isfinite(point.y) ||
            !std::isfinite(point.confidence) || point.x < 0.0 || point.x > 1.0 ||
            point.y < 0.0 || point.y > 1.0 || point.confidence < 0.0 ||
            point.confidence > 1.0) {
            return false;
        }
    }
    return true;
}

} // namespace

bool validate_landmark_provider_launch_v1(
    const AdmittedLandmarkProviderLaunchV1& launch) noexcept {
    return launch.schema_version == 1U &&
           launch.pack_id == admitted_openseeface_pack_id_v1 &&
           launch.pack_revision == admitted_openseeface_revision_v1 &&
           launch.runtime_revision == admitted_openseeface_runtime_revision_v1 &&
           launch.backend == admitted_openseeface_backend_v1 &&
           launch.maximum_signal_rate_hz > 0U && launch.maximum_signal_rate_hz <= 15U &&
           launch.inference_threads == 1U && launch.exact_target_process_id != 0U &&
           valid_sha256(launch.detector_sha256) && valid_sha256(launch.landmark_sha256) &&
           valid_sha256(launch.runtime_sha256) &&
           valid_sha256(launch.runtime_shared_sha256) &&
           valid_sha256(launch.measured_envelope_sha256) &&
           launch.detector_size_bytes != 0U && launch.landmark_size_bytes != 0U &&
           launch.runtime_size_bytes != 0U &&
           launch.runtime_shared_size_bytes != 0U &&
           absolute_child(launch.artifact_root, launch.detector_model) &&
           absolute_child(launch.artifact_root, launch.landmark_model) &&
           absolute_child(launch.artifact_root, launch.runtime_library) &&
           absolute_child(launch.artifact_root, launch.runtime_shared_library) &&
           launch.detector_model.filename() == "mnv3_detection_opt.onnx" &&
           launch.landmark_model.filename() == "lm_model1_opt.onnx" &&
           launch.runtime_library.filename() == "onnxruntime.dll" &&
           launch.runtime_shared_library.filename() == "onnxruntime_providers_shared.dll";
}

bool validate_landmark_inference_work_v1(const LandmarkInferenceWorkV1& work,
                                         const std::uint64_t generation,
                                         const Nanoseconds now_ns) noexcept {
    return work.schema_version == 1U && generation != 0U &&
           work.track.cancellation_generation == generation && work.track.actor_id != 0U &&
           work.track.track_id != 0U && work.track.track_epoch != 0U &&
           work.frame.sequence != 0U && work.frame.device_generation != 0U &&
           work.frame.geometry_epoch != 0U && work.frame.captured_at_ns > 0 &&
           work.source_frame_qpc != 0U && work.qpc_frequency != 0U &&
           valid_rect(work.seed_face_bounds) && work.source.identity == work.frame &&
           work.source.lease.width != 0U && work.source.lease.height != 0U &&
           work.source.lease.stride_bytes >= work.source.lease.width * 4U &&
           work.source.bgra.size() ==
               static_cast<std::size_t>(work.source.lease.stride_bytes) * work.source.lease.height &&
           work.deadline_ns >= now_ns;
}

NativeLandmarkCoordinatorV1::NativeLandmarkCoordinatorV1(
    std::unique_ptr<NativeLandmarkProviderV1> provider)
    : provider_(std::move(provider)) {}

LandmarkProviderReceiptV1 NativeLandmarkCoordinatorV1::load(
    const AdmittedLandmarkProviderLaunchV1& launch,
    const std::uint64_t generation,
    const Nanoseconds now_ns) {
    if (!provider_ || generation == 0U || !validate_landmark_provider_launch_v1(launch)) {
        return bypass(LandmarkProviderDispositionV1::bypass_invalid,
                      "provider_launch_invalid", now_ns);
    }
    provider_->unload();
    pending_.reset();
    generation_ = 0U;
    std::string failure;
    if (!provider_->load(launch, generation, failure) || !provider_->loaded()) {
        provider_->unload();
        return bypass(LandmarkProviderDispositionV1::bypass_provider_failure,
                      failure.empty() ? "provider_load_failed" : std::move(failure), now_ns);
    }
    generation_ = generation;
    return bypass(LandmarkProviderDispositionV1::ready, "provider_ready", now_ns);
}

LandmarkProviderReceiptV1 NativeLandmarkCoordinatorV1::submit(
    LandmarkInferenceWorkV1 work,
    const Nanoseconds now_ns) {
    if (!provider_ || !provider_->loaded()) {
        return bypass(LandmarkProviderDispositionV1::bypass_not_loaded,
                      "provider_not_loaded", now_ns, &work);
    }
    if (!validate_landmark_inference_work_v1(work, generation_, now_ns)) {
        return bypass(LandmarkProviderDispositionV1::bypass_invalid,
                      "provider_work_invalid", now_ns, &work);
    }
    LandmarkProviderReceiptV1 receipt{};
    if (pending_) {
        ++queue_replacements_;
        receipt = bypass(LandmarkProviderDispositionV1::replaced_before_processing,
                         "older_provider_work_replaced", now_ns, &pending_->work);
    } else {
        receipt = bypass(LandmarkProviderDispositionV1::ready,
                         "provider_work_queued", now_ns, &work);
    }
    pending_ = PendingWork{std::move(work), now_ns};
    receipt.queue_replacements = queue_replacements_;
    return receipt;
}

LandmarkProviderReceiptV1 NativeLandmarkCoordinatorV1::process_latest(
    const Nanoseconds now_ns) {
    if (!provider_ || !provider_->loaded()) {
        pending_.reset();
        return bypass(LandmarkProviderDispositionV1::bypass_not_loaded,
                      "provider_not_loaded", now_ns);
    }
    if (!pending_) {
        return bypass(LandmarkProviderDispositionV1::bypass_invalid,
                      "provider_queue_empty", now_ns);
    }
    PendingWork pending = std::move(*pending_);
    pending_.reset();
    if (!validate_landmark_inference_work_v1(pending.work, generation_, now_ns)) {
        return bypass(LandmarkProviderDispositionV1::bypass_stale,
                      "provider_work_stale", now_ns, &pending.work);
    }
    std::string failure;
    auto packet = provider_->infer(pending.work, now_ns, failure);
    if (!packet || !packet_matches(*packet, pending.work, generation_)) {
        return bypass(LandmarkProviderDispositionV1::bypass_provider_failure,
                      failure.empty() ? "provider_packet_binding_invalid" : std::move(failure),
                      now_ns, &pending.work);
    }
    auto receipt = bypass(LandmarkProviderDispositionV1::packet_produced,
                          "provider_packet_produced", now_ns, &pending.work);
    receipt.submitted_at_ns = pending.submitted_at_ns;
    receipt.completed_at_ns = packet->measured_at_ns;
    receipt.packet = std::move(packet);
    receipt.completed_work = std::move(pending.work);
    return receipt;
}

LandmarkProviderReceiptV1 NativeLandmarkCoordinatorV1::cancel_to(
    const std::uint64_t generation,
    const Nanoseconds now_ns) noexcept {
    if (generation <= generation_ || !provider_ || !provider_->cancel_to(generation)) {
        return bypass(LandmarkProviderDispositionV1::bypass_invalid,
                      "provider_cancel_not_monotonic", now_ns);
    }
    const LandmarkInferenceWorkV1* work = pending_ ? &pending_->work : nullptr;
    auto receipt = bypass(LandmarkProviderDispositionV1::bypass_cancelled,
                          "provider_cancelled", now_ns, work);
    pending_.reset();
    generation_ = generation;
    return receipt;
}

LandmarkProviderReceiptV1 NativeLandmarkCoordinatorV1::unload(
    const Nanoseconds now_ns) noexcept {
    pending_.reset();
    if (provider_) provider_->unload();
    generation_ = 0U;
    return bypass(LandmarkProviderDispositionV1::unloaded,
                  "provider_unloaded", now_ns);
}

std::uint64_t NativeLandmarkCoordinatorV1::active_generation() const noexcept {
    return generation_;
}

std::uint64_t NativeLandmarkCoordinatorV1::queue_replacements() const noexcept {
    return queue_replacements_;
}

LandmarkProviderReceiptV1 NativeLandmarkCoordinatorV1::bypass(
    const LandmarkProviderDispositionV1 disposition,
    std::string detail,
    const Nanoseconds now_ns,
    const LandmarkInferenceWorkV1* work) const {
    LandmarkProviderReceiptV1 receipt{};
    receipt.disposition = disposition;
    receipt.provider_generation = generation_;
    receipt.queue_replacements = queue_replacements_;
    receipt.completed_at_ns = now_ns;
    receipt.detail = std::move(detail);
    if (work) {
        receipt.track = work->track;
        receipt.frame = work->frame;
        receipt.source_frame_qpc = work->source_frame_qpc;
    }
    return receipt;
}

} // namespace npc::mouth
