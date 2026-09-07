#include "npc/mouth_worker/landmark_provider.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
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

[[nodiscard]] double intersection_area(const NormalizedRect& first,
                                       const NormalizedRect& second) noexcept {
    return std::max(0.0, std::min(first.right(), second.right()) -
                             std::max(first.x, second.x)) *
           std::max(0.0, std::min(first.bottom(), second.bottom()) -
                             std::max(first.y, second.y));
}

[[nodiscard]] NormalizedRect expanded_seed(const NormalizedRect& seed) noexcept {
    constexpr double margin = 0.75;
    const double left = std::max(0.0, seed.x - seed.width * margin);
    const double top = std::max(0.0, seed.y - seed.height * margin);
    const double right = std::min(1.0, seed.right() + seed.width * margin);
    const double bottom = std::min(1.0, seed.bottom() + seed.height * margin);
    return {left, top, right - left, bottom - top};
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

std::optional<YuNetFaceCandidateV1> decode_and_select_yunet_face_v1(
    const std::span<const YuNetDetectorLevelV1> levels,
    const std::uint32_t model_width,
    const std::uint32_t model_height,
    const std::uint32_t content_width,
    const std::uint32_t content_height,
    const std::uint32_t source_width,
    const std::uint32_t source_height,
    const NormalizedRect& seed_face_bounds,
    const double score_threshold,
    const double nms_threshold) noexcept {
    if (levels.empty() || levels.size() > 8U || model_width < 4U || model_height < 4U ||
        content_width < 4U || content_height < 4U || content_width > model_width ||
        content_height > model_height || source_width < 4U || source_height < 4U ||
        !valid_rect(seed_face_bounds) || !std::isfinite(score_threshold) ||
        !std::isfinite(nms_threshold) || score_threshold < 0.0 || score_threshold > 1.0 ||
        nms_threshold < 0.0 || nms_threshold > 1.0) {
        return std::nullopt;
    }

    const NormalizedRect search_region = expanded_seed(seed_face_bounds);
    const double seed_diagonal = std::hypot(seed_face_bounds.width * source_width,
                                            seed_face_bounds.height * source_height);
    const double seed_center_x = (seed_face_bounds.x + seed_face_bounds.width * 0.5) *
                                 source_width;
    const double seed_center_y = (seed_face_bounds.y + seed_face_bounds.height * 0.5) *
                                 source_height;
    const double seed_area = seed_face_bounds.width * seed_face_bounds.height;
    const auto eligible_for_seed = [&](const NormalizedRect& bounds) {
        const double candidate_area = bounds.width * bounds.height;
        const double search_overlap = intersection_area(bounds, search_region);
        const double dx = (bounds.x + bounds.width * 0.5) * source_width - seed_center_x;
        const double dy = (bounds.y + bounds.height * 0.5) * source_height - seed_center_y;
        return candidate_area > 0.0 && search_overlap / candidate_area >= 0.50 &&
               seed_diagonal > 0.0 && std::hypot(dx, dy) / seed_diagonal <= 1.0;
    };

    std::vector<YuNetFaceCandidateV1> decoded;
    constexpr std::size_t maximum_pre_nms_candidates = 1'024U;
    decoded.reserve(maximum_pre_nms_candidates);
    for (const auto& level : levels) {
        const std::size_t cells = static_cast<std::size_t>(level.grid_width) *
                                  static_cast<std::size_t>(level.grid_height);
        if (level.stride == 0U || level.grid_width == 0U || level.grid_height == 0U ||
            cells > 65'536U || level.class_scores.size() != cells ||
            level.object_scores.size() != cells || level.boxes.size() != cells * 4U ||
            static_cast<std::uint64_t>(level.grid_width) * level.stride != model_width ||
            static_cast<std::uint64_t>(level.grid_height) * level.stride != model_height) {
            return std::nullopt;
        }
        for (std::size_t index = 0U; index < cells; ++index) {
            const float class_score = level.class_scores[index];
            const float object_score = level.object_scores[index];
            const float* box = level.boxes.data() + index * 4U;
            if (!std::isfinite(class_score) || !std::isfinite(object_score) ||
                !std::all_of(box, box + 4U, [](const float value) {
                    return std::isfinite(value);
                })) {
                continue;
            }
            const double confidence = std::sqrt(
                std::clamp(static_cast<double>(class_score), 0.0, 1.0) *
                std::clamp(static_cast<double>(object_score), 0.0, 1.0));
            if (confidence < score_threshold || box[2] < -20.0F || box[2] > 20.0F ||
                box[3] < -20.0F || box[3] > 20.0F) {
                continue;
            }
            const double column = static_cast<double>(index % level.grid_width);
            const double row = static_cast<double>(index / level.grid_width);
            const double center_x = (column + static_cast<double>(box[0])) * level.stride;
            const double center_y = (row + static_cast<double>(box[1])) * level.stride;
            if (center_x < 0.0 || center_y < 0.0 ||
                center_x >= static_cast<double>(content_width) ||
                center_y >= static_cast<double>(content_height)) {
                continue;
            }
            const double width = std::exp(static_cast<double>(box[2])) * level.stride;
            const double height = std::exp(static_cast<double>(box[3])) * level.stride;
            const double left = std::clamp(
                (center_x - width * 0.5) / content_width, 0.0, 1.0);
            const double top = std::clamp(
                (center_y - height * 0.5) / content_height, 0.0, 1.0);
            const double right = std::clamp(
                (center_x + width * 0.5) / content_width, 0.0, 1.0);
            const double bottom = std::clamp(
                (center_y + height * 0.5) / content_height, 0.0, 1.0);
            NormalizedRect bounds{left, top, right - left, bottom - top};
            if (!valid_rect(bounds) || bounds.width * source_width < 4.0 ||
                bounds.height * source_height < 4.0) {
                continue;
            }
            if (eligible_for_seed(bounds)) decoded.push_back({bounds, confidence});
        }
    }
    if (decoded.empty()) return std::nullopt;

    const auto confidence_order = [](const auto& first, const auto& second) {
        return first.confidence > second.confidence;
    };
    if (decoded.size() > maximum_pre_nms_candidates) {
        std::partial_sort(decoded.begin(),
                          decoded.begin() + maximum_pre_nms_candidates,
                          decoded.end(), confidence_order);
        decoded.resize(maximum_pre_nms_candidates);
    } else {
        std::stable_sort(decoded.begin(), decoded.end(), confidence_order);
    }
    std::vector<YuNetFaceCandidateV1> retained;
    retained.reserve(decoded.size());
    for (const auto& candidate : decoded) {
        const double candidate_area = candidate.bounds.width * candidate.bounds.height;
        const bool suppressed = std::any_of(
            retained.begin(), retained.end(), [&](const auto& accepted) {
                const double accepted_area = accepted.bounds.width * accepted.bounds.height;
                const double overlap = intersection_area(candidate.bounds, accepted.bounds);
                const double union_area = candidate_area + accepted_area - overlap;
                return union_area > 0.0 && overlap / union_area > nms_threshold;
            });
        if (!suppressed) retained.push_back(candidate);
    }

    const YuNetFaceCandidateV1* selected{};
    double selected_rank = -std::numeric_limits<double>::infinity();
    for (const auto& candidate : retained) {
        const double candidate_area = candidate.bounds.width * candidate.bounds.height;
        const double seed_overlap = intersection_area(candidate.bounds, seed_face_bounds);
        const double union_area = candidate_area + seed_area - seed_overlap;
        const double iou = union_area > 0.0 ? seed_overlap / union_area : 0.0;
        const double rank = candidate.confidence + iou * 0.25;
        if (!selected || rank > selected_rank) {
            selected = &candidate;
            selected_rank = rank;
        }
    }
    return selected ? std::optional<YuNetFaceCandidateV1>{*selected} : std::nullopt;
}

bool validate_yunet_tracking_policy_v1(
    const YuNetTrackingPolicyV1& policy) noexcept {
    return policy.detector_refresh_interval_frames >=
               yunet_min_detector_refresh_interval_v1 &&
           policy.detector_refresh_interval_frames <=
               yunet_max_detector_refresh_interval_v1;
}

YuNetTrackingActionV1 choose_yunet_tracking_action_v1(
    const bool has_tracked_face,
    const std::uint32_t frames_since_detector,
    const YuNetTrackingPolicyV1& policy) noexcept {
    if (!has_tracked_face || !validate_yunet_tracking_policy_v1(policy) ||
        frames_since_detector >= policy.detector_refresh_interval_frames) {
        return YuNetTrackingActionV1::reacquire_face;
    }
    return YuNetTrackingActionV1::track_landmarks;
}

bool yunet_face_matches_locked_actor_v1(
    const NormalizedRect& candidate,
    const NormalizedRect& locked_face) noexcept {
    if (!valid_rect(candidate) || !valid_rect(locked_face)) return false;
    const double locked_diagonal = std::hypot(locked_face.width, locked_face.height);
    const double dx = candidate.x + candidate.width * 0.5 -
                      (locked_face.x + locked_face.width * 0.5);
    const double dy = candidate.y + candidate.height * 0.5 -
                      (locked_face.y + locked_face.height * 0.5);
    const double candidate_area = candidate.width * candidate.height;
    const double locked_area = locked_face.width * locked_face.height;
    const double overlap = intersection_area(candidate, locked_face);
    const double union_area = candidate_area + locked_area - overlap;
    const double iou = union_area > 0.0 ? overlap / union_area : 0.0;
    const double area_ratio = candidate_area / locked_area;
    return locked_diagonal > 0.0 && std::hypot(dx, dy) / locked_diagonal <= 0.65 &&
           area_ratio >= 0.45 && area_ratio <= 2.20 && iou >= 0.04;
}

std::optional<YuNetTrackedFaceUpdateV1> update_yunet_tracked_face_v1(
    const NormalizedRect& previous_face,
    const std::span<const NormalizedLandmark> previous_landmarks,
    const std::span<const NormalizedLandmark> current_landmarks) noexcept {
    constexpr std::size_t stable_face_landmark_count = 48U;
    constexpr double minimum_confidence = 0.55;
    constexpr double maximum_center_motion_face_fraction = 0.08;
    if (!valid_rect(previous_face) ||
        previous_landmarks.size() < stable_face_landmark_count ||
        current_landmarks.size() < stable_face_landmark_count) {
        return std::nullopt;
    }

    double previous_center_x{};
    double previous_center_y{};
    double current_center_x{};
    double current_center_y{};
    std::size_t accepted{};
    const auto valid_pair = [&](const std::size_t index) {
        const auto& before = previous_landmarks[index];
        const auto& after = current_landmarks[index];
        return std::isfinite(before.x) && std::isfinite(before.y) &&
               std::isfinite(before.confidence) && std::isfinite(after.x) &&
               std::isfinite(after.y) && std::isfinite(after.confidence) &&
               before.confidence >= minimum_confidence &&
               after.confidence >= minimum_confidence;
    };
    for (std::size_t index = 0U; index < stable_face_landmark_count; ++index) {
        const auto& before = previous_landmarks[index];
        const auto& after = current_landmarks[index];
        if (!valid_pair(index)) continue;
        previous_center_x += before.x;
        previous_center_y += before.y;
        current_center_x += after.x;
        current_center_y += after.y;
        ++accepted;
    }
    if (accepted < 24U) return std::nullopt;
    const double divisor = static_cast<double>(accepted);
    previous_center_x /= divisor;
    previous_center_y /= divisor;
    current_center_x /= divisor;
    current_center_y /= divisor;

    double previous_radius_squared{};
    double current_radius_squared{};
    for (std::size_t index = 0U; index < stable_face_landmark_count; ++index) {
        const auto& before = previous_landmarks[index];
        const auto& after = current_landmarks[index];
        if (!valid_pair(index)) continue;
        previous_radius_squared += std::pow(before.x - previous_center_x, 2.0) +
                                   std::pow(before.y - previous_center_y, 2.0);
        current_radius_squared += std::pow(after.x - current_center_x, 2.0) +
                                  std::pow(after.y - current_center_y, 2.0);
    }
    if (!(previous_radius_squared > 1.0e-12) || !(current_radius_squared > 1.0e-12)) {
        return std::nullopt;
    }
    const double scale = std::sqrt(current_radius_squared / previous_radius_squared);
    const double dx = current_center_x - previous_center_x;
    const double dy = current_center_y - previous_center_y;
    const double diagonal = std::hypot(previous_face.width, previous_face.height);
    const double motion = diagonal > 0.0 ? std::hypot(dx, dy) / diagonal
                                         : std::numeric_limits<double>::infinity();
    if (!std::isfinite(scale) || !std::isfinite(motion) || scale < 0.82 || scale > 1.22 ||
        motion > maximum_center_motion_face_fraction) {
        return std::nullopt;
    }

    const double center_x = previous_face.x + previous_face.width * 0.5 + dx;
    const double center_y = previous_face.y + previous_face.height * 0.5 + dy;
    const double width = previous_face.width * scale;
    const double height = previous_face.height * scale;
    const double left = center_x - width * 0.5;
    const double top = center_y - height * 0.5;
    const NormalizedRect updated{left, top, width, height};
    if (!valid_rect(updated) || !yunet_face_matches_locked_actor_v1(updated, previous_face)) {
        return std::nullopt;
    }
    return YuNetTrackedFaceUpdateV1{updated, motion, scale};
}

bool validate_landmark_provider_launch_v1(
    const AdmittedLandmarkProviderLaunchV1& launch) noexcept {
    const bool mnv3_pack =
        launch.pack_id == admitted_openseeface_pack_id_v1 &&
        launch.detector_model.filename() == "mnv3_detection_opt.onnx";
    const bool yunet_pack =
        launch.pack_id == admitted_yunet_openseeface_pack_id_v1 &&
        launch.detector_model.filename() == "face_detection_yunet_2023mar.onnx" &&
        launch.detector_sha256 == admitted_yunet_detector_sha256_v1 &&
        launch.landmark_sha256 == admitted_openseeface_lm1_sha256_v1;
    const bool runtime_launch = !launch.provider_load_self_test &&
                                launch.exact_target_process_id != 0U;
    const bool setup_self_test = launch.schema_version == 2U &&
                                 launch.provider_load_self_test &&
                                 launch.exact_target_process_id == 0U;
    return (launch.schema_version == 1U || launch.schema_version == 2U) &&
           (runtime_launch || setup_self_test) && (mnv3_pack || yunet_pack) &&
           launch.pack_revision == admitted_openseeface_revision_v1 &&
           launch.runtime_revision == admitted_openseeface_runtime_revision_v1 &&
           launch.backend == admitted_openseeface_backend_v1 &&
           launch.maximum_signal_rate_hz > 0U && launch.maximum_signal_rate_hz <= 15U &&
           launch.inference_threads == 1U &&
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
