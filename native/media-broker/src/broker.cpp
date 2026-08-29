#include "npc/media_broker/broker.hpp"

#include <algorithm>
#include <cmath>
#include <utility>

namespace npc::media {

namespace {

constexpr auto hard_frame_stale_after = std::chrono::milliseconds{100};
constexpr auto hard_patch_stale_after = std::chrono::milliseconds{80};
constexpr auto hard_evidence_stale_after = std::chrono::milliseconds{120};
constexpr auto hard_source_to_patch_latency = std::chrono::milliseconds{80};
constexpr auto hard_source_timestamp_tolerance = std::chrono::milliseconds{2};
constexpr double hard_maximum_residual_width = 0.45;
constexpr double hard_maximum_residual_height = 0.35;
constexpr double hard_maximum_residual_area = 0.12;

template <typename Rep, typename Period>
[[nodiscard]] bool older_than(const MonotonicTime value,
                              const MonotonicTime now,
                              const std::chrono::duration<Rep, Period> limit) noexcept {
    return value > now || now - value > limit;
}

[[nodiscard]] std::chrono::milliseconds recovery_backoff(const TimingPolicy& policy,
                                                         const std::uint32_t failure_count) noexcept {
    const auto shift = std::min<std::uint32_t>(failure_count > 0 ? failure_count - 1 : 0, 10);
    const auto factor = static_cast<std::int64_t>(1ULL << shift);
    return std::min(policy.recovery_initial_backoff * factor, policy.recovery_max_backoff);
}

[[nodiscard]] bool unit_confidence(const double value) noexcept {
    return std::isfinite(value) && value >= 0.0 && value <= 1.0;
}

[[nodiscard]] bool meets_confidence(const double value,
                                    const double configured_minimum,
                                    const double hard_minimum) noexcept {
    return unit_confidence(value) && unit_confidence(configured_minimum) &&
           value >= std::max(configured_minimum, hard_minimum);
}

template <typename DurationType>
[[nodiscard]] DurationType strict_limit(const DurationType configured,
                                        const DurationType hard_limit) noexcept {
    return configured < DurationType::zero() ? DurationType::zero()
                                             : std::min(configured, hard_limit);
}

[[nodiscard]] bool residual_bounds_safe(const RectF bounds,
                                        const ResidualBoundsPolicy& policy) noexcept {
    if (!std::isfinite(policy.maximum_width) || !std::isfinite(policy.maximum_height) ||
        !std::isfinite(policy.maximum_area) || policy.maximum_width <= 0.0 ||
        policy.maximum_height <= 0.0 || policy.maximum_area <= 0.0) {
        return false;
    }
    const auto maximum_width = std::min(policy.maximum_width, hard_maximum_residual_width);
    const auto maximum_height = std::min(policy.maximum_height, hard_maximum_residual_height);
    const auto maximum_area = std::min(policy.maximum_area, hard_maximum_residual_area);
    return normalized_rect_valid(bounds) &&
           bounds.width() <= maximum_width &&
           bounds.height() <= maximum_height &&
           bounds.width() * bounds.height() <= maximum_area;
}

[[nodiscard]] Duration absolute_delta(const MonotonicTime left,
                                      const MonotonicTime right) noexcept {
    return left >= right ? left - right : right - left;
}

} // namespace

MediaBroker::MediaBroker(std::unique_ptr<IMediaPlatform> platform,
                         BrokerPolicy policy,
                         BrokerEventSink sink)
    : platform_(std::move(platform)), policy_(policy), sink_(std::move(sink)) {
    configure_callbacks();
}

MediaBroker::~MediaBroker() { stop(); }

void MediaBroker::configure_callbacks() {
    platform_->set_callbacks({
        [this](FrameDescriptor frame) { handle_frame(std::move(frame)); },
        [this](const PttState state, const MonotonicTime at) {
            if (sink_.on_ptt) {
                sink_.on_ptt(state, at);
            }
        },
        [this](Failure failure) { handle_failure(std::move(failure)); },
        [this](const TargetState state, std::optional<TargetGeometry> geometry) {
            handle_target_state(state, std::move(geometry));
        },
    });
}

bool MediaBroker::start(const std::uint32_t ptt_virtual_key) {
    if (running_) {
        return true;
    }
    diagnostics_ = {};
    diagnostics_.state = BrokerState::starting;
    publish();

    Failure failure;
    diagnostics_.capture_audio = AudioState::initializing;
    diagnostics_.render_audio = AudioState::initializing;
    if (!platform_->initialize_audio(failure)) {
        fail(std::move(failure), BrokerState::failed, RecoveryAction::recreate_audio_client);
        return false;
    }
    diagnostics_.capture_audio = AudioState::ready;
    diagnostics_.render_audio = AudioState::ready;

    if (!platform_->register_ptt_hotkey(ptt_virtual_key, failure)) {
        platform_->shutdown_audio();
        diagnostics_.capture_audio = AudioState::stopped;
        diagnostics_.render_audio = AudioState::stopped;
        fail(std::move(failure), BrokerState::failed, RecoveryAction::none);
        return false;
    }

    running_ = true;
    diagnostics_.state = BrokerState::awaiting_target;
    publish();
    return true;
}

void MediaBroker::stop() noexcept {
    if (!running_ && diagnostics_.state == BrokerState::stopped) {
        return;
    }
    diagnostics_.state = BrokerState::stopping;
    publish();
    ++diagnostics_.cancellation_generation;
    platform_->stop_overlay();
    platform_->stop_capture();
    platform_->unregister_ptt_hotkey();
    platform_->shutdown_audio();
    frames_.clear();
    patches_.clear();
    target_.reset();
    target_geometry_.reset();
    overlay_geometry_.reset();
    occlusion_evidence_.reset();
    diagnostics_.state = BrokerState::stopped;
    diagnostics_.capture_backend = CaptureBackend::none;
    diagnostics_.overlay_backend = OverlayBackend::none;
    diagnostics_.capture_audio = AudioState::stopped;
    diagnostics_.render_audio = AudioState::stopped;
    diagnostics_.target_state = TargetState::none;
    diagnostics_.pending_recovery = RecoveryAction::none;
    running_ = false;
    publish();
}

bool MediaBroker::select_target(GameTarget target) {
    if (!running_) {
        fail({FailureDomain::target, FailureCode::internal_error, false, "Broker is not running"},
             BrokerState::failed,
             RecoveryAction::none);
        return false;
    }

    Failure failure;
    if (!platform_->validate_target(target, failure)) {
        fail(std::move(failure), BrokerState::awaiting_target, RecoveryAction::await_target);
        return false;
    }

    ++diagnostics_.cancellation_generation;
    platform_->stop_overlay();
    platform_->stop_capture();
    frames_.clear();
    patches_.clear();
    occlusion_evidence_.reset();
    target_geometry_.reset();
    overlay_geometry_.reset();
    target_ = std::move(target);
    diagnostics_.target_state = TargetState::selected;
    diagnostics_.last_failure.reset();
    diagnostics_.pending_recovery = RecoveryAction::none;
    diagnostics_.consecutive_failures = 0;
    same_backend_retries_ = 0;

    return activate_capture(CaptureBackend::windows_graphics_capture);
}

void MediaBroker::clear_target() noexcept {
    ++diagnostics_.cancellation_generation;
    platform_->stop_overlay();
    platform_->stop_capture();
    frames_.clear();
    patches_.clear();
    target_.reset();
    target_geometry_.reset();
    overlay_geometry_.reset();
    occlusion_evidence_.reset();
    diagnostics_.target_state = TargetState::none;
    diagnostics_.capture_backend = CaptureBackend::none;
    diagnostics_.overlay_backend = OverlayBackend::none;
    diagnostics_.pending_recovery = RecoveryAction::none;
    diagnostics_.state = running_ ? BrokerState::awaiting_target : BrokerState::stopped;
    publish();
}

void MediaBroker::submit_patch(MouthPatch patch) {
    ++diagnostics_.patches_received;
    const bool source_timing_invalid =
        patch.produced_at < patch.source_frame_captured_at ||
        patch.produced_at - patch.source_frame_captured_at >
            strict_limit(policy_.timing.maximum_source_to_patch_latency,
                         hard_source_to_patch_latency);
    if (patch.cancellation_generation != diagnostics_.cancellation_generation ||
        patch.source_device_generation != diagnostics_.device_generation ||
        source_timing_invalid || patch.native_texture == 0 ||
        !unit_confidence(patch.confidence) ||
        !residual_bounds_safe(patch.normalized_bounds, policy_.residual_bounds)) {
        // Do not let a previously accepted residual remain visible while an
        // unsafe newer result waits for the next capture frame.
        platform_->suppress_residual();
    }
    const auto result = patches_.push(std::move(patch));
    (void)result;
}

void MediaBroker::submit_occlusion_evidence(OcclusionEvidence evidence) {
    if (!unit_confidence(evidence.face_confidence) ||
        !unit_confidence(evidence.landmark_confidence) ||
        !unit_confidence(evidence.visibility_ratio) ||
        evidence.source_device_generation != diagnostics_.device_generation) {
        platform_->suppress_residual();
    }
    occlusion_evidence_ = std::move(evidence);
}

void MediaBroker::cancel_generation(const std::uint64_t new_generation) noexcept {
    diagnostics_.cancellation_generation = std::max(diagnostics_.cancellation_generation + 1, new_generation);
    patches_.clear();
    if (auto* render = platform_->render_pcm_ring()) {
        render->clear();
    }
    platform_->suppress_residual();
    ++diagnostics_.overlays_suppressed;
    publish();
}

void MediaBroker::tick(const MonotonicTime now) {
    if (!running_) {
        return;
    }
    platform_->poll();
    attempt_recovery(now);

    auto frame = frames_.take_latest();
    if (!frame || !overlay_geometry_) {
        return;
    }
    auto patch = patches_.take_latest();
    auto decision = decide_compositing(*frame, patch, now);
    if (decision == CompositingDecision::patch && patch) {
        const auto patch_bounds = map_normalized_source_rect(patch->normalized_bounds, *overlay_geometry_);
        if (patch_bounds) {
            platform_->present_patch(*frame, *patch, *patch_bounds, *overlay_geometry_);
            ++diagnostics_.frames_presented;
            ++diagnostics_.patches_presented;
        } else {
            decision = CompositingDecision::pristine_unsafe_bounds;
            platform_->present_pristine(*frame, *overlay_geometry_);
            ++diagnostics_.frames_presented;
            ++diagnostics_.overlays_suppressed;
            ++diagnostics_.patches_rejected;
        }
    } else {
        platform_->present_pristine(*frame, *overlay_geometry_);
        ++diagnostics_.frames_presented;
        ++diagnostics_.overlays_suppressed;
        if (patch) {
            ++diagnostics_.patches_rejected;
        }
    }
    if (sink_.on_compositing_decision) {
        sink_.on_compositing_decision(decision);
    }
    publish();
}

const Diagnostics& MediaBroker::diagnostics() const noexcept { return diagnostics_; }
IMediaPlatform& MediaBroker::platform() noexcept { return *platform_; }

void MediaBroker::handle_frame(FrameDescriptor frame) {
    if (!running_ || frame.device_generation != diagnostics_.device_generation) {
        ++diagnostics_.frames_dropped;
        return;
    }
    if (frame.protected_content) {
        handle_failure({FailureDomain::capture, FailureCode::protected_content, false,
                        "Capture backend reported protected content"});
        ++diagnostics_.frames_dropped;
        return;
    }
    same_backend_retries_ = 0;
    diagnostics_.consecutive_failures = 0;
    ++diagnostics_.frames_received;
    const auto result = frames_.push(std::move(frame));
    if (result.replaced_unread) {
        ++diagnostics_.frames_dropped;
    }
}

void MediaBroker::handle_failure(Failure failure) {
    if (failure.domain == FailureDomain::audio_capture || failure.domain == FailureDomain::audio_render) {
        diagnostics_.capture_audio = AudioState::recovering;
        diagnostics_.render_audio = AudioState::recovering;
        fail(std::move(failure), BrokerState::recovering_device, RecoveryAction::recreate_audio_client);
        recovery_due_ = std::chrono::steady_clock::now() +
                        recovery_backoff(policy_.timing, diagnostics_.consecutive_failures);
        return;
    }
    switch (failure.code) {
    case FailureCode::target_lost:
    case FailureCode::target_minimized:
        platform_->stop_overlay();
        platform_->stop_capture();
        frames_.clear();
        patches_.clear();
        diagnostics_.overlay_backend = OverlayBackend::none;
        diagnostics_.capture_backend = CaptureBackend::none;
        fail(std::move(failure), BrokerState::awaiting_target, RecoveryAction::await_target);
        break;
    case FailureCode::protected_content:
        platform_->stop_overlay();
        platform_->stop_capture();
        frames_.clear();
        patches_.clear();
        diagnostics_.overlay_backend = OverlayBackend::none;
        diagnostics_.capture_backend = CaptureBackend::none;
        fail(std::move(failure), BrokerState::degraded_audio_only, RecoveryAction::degrade_to_audio_only);
        break;
    case FailureCode::device_removed:
    case FailureCode::device_reset:
        fail(std::move(failure), BrokerState::recovering_device, RecoveryAction::recreate_graphics_device);
        recovery_due_ = std::chrono::steady_clock::now() +
                        recovery_backoff(policy_.timing, diagnostics_.consecutive_failures);
        break;
    default:
        fail(std::move(failure), BrokerState::recovering_device, RecoveryAction::retry_same_backend);
        recovery_due_ = std::chrono::steady_clock::now() +
                        recovery_backoff(policy_.timing, diagnostics_.consecutive_failures);
        break;
    }
}

void MediaBroker::handle_target_state(const TargetState state, std::optional<TargetGeometry> geometry) {
    diagnostics_.target_state = state;
    if (state == TargetState::protected_content) {
        handle_failure({FailureDomain::target, FailureCode::protected_content, false, "Target exposes protected content"});
        return;
    }
    if (state == TargetState::closed || state == TargetState::unavailable) {
        handle_failure({FailureDomain::target, FailureCode::target_lost, true, "Target window is unavailable"});
        return;
    }
    if (state == TargetState::minimized) {
        platform_->stop_overlay();
        diagnostics_.overlay_backend = OverlayBackend::none;
        diagnostics_.state = BrokerState::awaiting_target;
        publish();
        return;
    }
    if (!geometry) {
        publish();
        return;
    }

    target_geometry_ = std::move(geometry);
    overlay_geometry_ = calculate_overlay_geometry(*target_geometry_);
    if (!overlay_geometry_) {
        handle_failure({FailureDomain::overlay, FailureCode::invalid_geometry, true, "Target geometry cannot be mapped"});
        return;
    }

    Failure failure;
    platform_->stop_overlay();
    if (!platform_->start_overlay(*target_geometry_, failure)) {
        if (policy_.permit_audio_only_fallback) {
            fail(std::move(failure), BrokerState::degraded_audio_only, RecoveryAction::degrade_to_audio_only);
        } else {
            fail(std::move(failure), BrokerState::failed, RecoveryAction::none);
        }
        return;
    }
    diagnostics_.overlay_backend = OverlayBackend::d3d11_direct_composition;
    diagnostics_.state = diagnostics_.capture_backend == CaptureBackend::windows_graphics_capture
                             ? BrokerState::capturing_primary
                             : BrokerState::capturing_fallback;
    publish();
}

void MediaBroker::attempt_recovery(const MonotonicTime now) {
    if (diagnostics_.pending_recovery == RecoveryAction::none || now < recovery_due_) {
        return;
    }

    const auto action = diagnostics_.pending_recovery;
    diagnostics_.pending_recovery = RecoveryAction::none;
    Failure failure;
    switch (action) {
    case RecoveryAction::recreate_graphics_device:
        platform_->stop_overlay();
        platform_->stop_capture();
        frames_.clear();
        patches_.clear();
        ++diagnostics_.device_generation;
        if (!platform_->recreate_graphics_device(diagnostics_.device_generation, failure)) {
            handle_failure(std::move(failure));
            return;
        }
        if (target_) {
            (void)activate_capture(CaptureBackend::windows_graphics_capture);
        }
        break;
    case RecoveryAction::recreate_audio_client:
        diagnostics_.capture_audio = AudioState::recovering;
        diagnostics_.render_audio = AudioState::recovering;
        if (!platform_->recreate_audio_clients(failure)) {
            fail(std::move(failure), BrokerState::degraded_audio_only, RecoveryAction::none);
            return;
        }
        diagnostics_.capture_audio = AudioState::ready;
        diagnostics_.render_audio = AudioState::ready;
        ++diagnostics_.audio_device_generation;
        diagnostics_.state = diagnostics_.capture_backend == CaptureBackend::windows_graphics_capture
                                 ? BrokerState::capturing_primary
                                 : diagnostics_.capture_backend == CaptureBackend::desktop_duplication
                                       ? BrokerState::capturing_fallback
                                       : target_ ? BrokerState::degraded_audio_only : BrokerState::awaiting_target;
        break;
    case RecoveryAction::retry_same_backend:
        if (same_backend_retries_ < policy_.timing.max_same_backend_retries) {
            ++same_backend_retries_;
            (void)activate_capture(diagnostics_.capture_backend);
        } else if (diagnostics_.capture_backend == CaptureBackend::windows_graphics_capture &&
                   policy_.permit_desktop_duplication_fallback) {
            same_backend_retries_ = 0;
            (void)activate_capture(CaptureBackend::desktop_duplication);
        } else if (policy_.permit_audio_only_fallback) {
            diagnostics_.state = BrokerState::degraded_audio_only;
            diagnostics_.pending_recovery = RecoveryAction::degrade_to_audio_only;
        } else {
            diagnostics_.state = BrokerState::failed;
        }
        break;
    case RecoveryAction::switch_to_desktop_duplication:
        (void)activate_capture(CaptureBackend::desktop_duplication);
        break;
    case RecoveryAction::await_target:
    case RecoveryAction::degrade_to_audio_only:
    case RecoveryAction::block_session:
    case RecoveryAction::none:
        break;
    }
    publish();
}

bool MediaBroker::activate_capture(const CaptureBackend backend) {
    if (!target_) {
        return false;
    }
    Failure failure;
    platform_->stop_capture();
    if (!platform_->start_capture(*target_, backend, failure)) {
        diagnostics_.capture_backend = backend;
        if (backend == CaptureBackend::windows_graphics_capture &&
            policy_.permit_desktop_duplication_fallback &&
            (failure.code == FailureCode::backend_unavailable || failure.code == FailureCode::access_denied)) {
            diagnostics_.last_failure = failure;
            return activate_capture(CaptureBackend::desktop_duplication);
        }
        handle_failure(std::move(failure));
        return false;
    }
    diagnostics_.capture_backend = backend;
    diagnostics_.state = backend == CaptureBackend::windows_graphics_capture
                             ? BrokerState::capturing_primary
                             : BrokerState::capturing_fallback;
    diagnostics_.pending_recovery = RecoveryAction::none;
    diagnostics_.last_failure.reset();
    publish();
    return true;
}

CompositingDecision MediaBroker::decide_compositing(const FrameDescriptor& frame,
                                                     const std::optional<MouthPatch>& patch,
                                                     const MonotonicTime now) const noexcept {
    if (older_than(frame.captured_at, now,
                   strict_limit(policy_.timing.frame_stale_after, hard_frame_stale_after))) {
        return CompositingDecision::pristine_stale_frame;
    }
    if (frame.content_occluded || frame.protected_content) {
        return CompositingDecision::pristine_occluded;
    }
    if (!patch) {
        return CompositingDecision::pristine_no_patch;
    }
    if (patch->cancellation_generation != diagnostics_.cancellation_generation) {
        return CompositingDecision::pristine_wrong_generation;
    }
    if (patch->source_device_generation != frame.device_generation ||
        patch->source_device_generation != diagnostics_.device_generation) {
        return CompositingDecision::pristine_wrong_epoch;
    }
    if (patch->source_frame_sequence != frame.sequence) {
        return CompositingDecision::pristine_wrong_frame;
    }
    if (absolute_delta(patch->source_frame_captured_at, frame.captured_at) >
            strict_limit(policy_.timing.source_timestamp_tolerance,
                         hard_source_timestamp_tolerance) ||
        patch->produced_at < patch->source_frame_captured_at ||
        patch->produced_at - patch->source_frame_captured_at >
            strict_limit(policy_.timing.maximum_source_to_patch_latency,
                         hard_source_to_patch_latency)) {
        return CompositingDecision::pristine_wrong_source_time;
    }
    if (older_than(patch->produced_at, now,
                   strict_limit(policy_.timing.patch_stale_after, hard_patch_stale_after))) {
        return CompositingDecision::pristine_stale_patch;
    }
    if (!occlusion_evidence_ ||
        older_than(occlusion_evidence_->measured_at, now,
                   strict_limit(policy_.timing.evidence_stale_after,
                                hard_evidence_stale_after))) {
        return CompositingDecision::pristine_stale_evidence;
    }
    if (occlusion_evidence_->mouth_region_occluded) {
        return CompositingDecision::pristine_occluded;
    }
    if (occlusion_evidence_->source_device_generation != frame.device_generation ||
        occlusion_evidence_->source_frame_sequence != frame.sequence) {
        return CompositingDecision::pristine_wrong_frame;
    }
    if (!meets_confidence(patch->confidence,
                          policy_.occlusion.minimum_patch_confidence, 0.82) ||
        !meets_confidence(occlusion_evidence_->face_confidence,
                          policy_.occlusion.minimum_face_confidence, 0.78) ||
        !meets_confidence(occlusion_evidence_->landmark_confidence,
                          policy_.occlusion.minimum_landmark_confidence, 0.82) ||
        !meets_confidence(occlusion_evidence_->visibility_ratio,
                          policy_.occlusion.minimum_visibility_ratio, 0.72)) {
        return CompositingDecision::pristine_low_confidence;
    }
    if (!residual_bounds_safe(patch->normalized_bounds, policy_.residual_bounds)) {
        return CompositingDecision::pristine_unsafe_bounds;
    }
    if (patch->native_texture == 0) {
        return CompositingDecision::pristine_missing_texture;
    }
    return CompositingDecision::patch;
}

void MediaBroker::publish() {
    if (sink_.on_diagnostics) {
        sink_.on_diagnostics(diagnostics_);
    }
}

void MediaBroker::fail(Failure failure, const BrokerState state, const RecoveryAction recovery) {
    diagnostics_.last_failure = std::move(failure);
    ++diagnostics_.consecutive_failures;
    diagnostics_.state = state;
    diagnostics_.pending_recovery = recovery;
    publish();
}

std::string_view to_string(const BrokerState state) noexcept {
    switch (state) {
    case BrokerState::stopped: return "stopped";
    case BrokerState::starting: return "starting";
    case BrokerState::awaiting_target: return "awaiting_target";
    case BrokerState::capturing_primary: return "capturing_primary";
    case BrokerState::capturing_fallback: return "capturing_fallback";
    case BrokerState::recovering_device: return "recovering_device";
    case BrokerState::blocked_by_policy: return "blocked_by_policy";
    case BrokerState::degraded_audio_only: return "degraded_audio_only";
    case BrokerState::stopping: return "stopping";
    case BrokerState::failed: return "failed";
    }
    return "unknown";
}

std::string_view to_string(const CaptureBackend backend) noexcept {
    switch (backend) {
    case CaptureBackend::none: return "none";
    case CaptureBackend::windows_graphics_capture: return "windows_graphics_capture";
    case CaptureBackend::desktop_duplication: return "desktop_duplication";
    }
    return "unknown";
}

std::string_view to_string(const RecoveryAction action) noexcept {
    switch (action) {
    case RecoveryAction::none: return "none";
    case RecoveryAction::retry_same_backend: return "retry_same_backend";
    case RecoveryAction::switch_to_desktop_duplication: return "switch_to_desktop_duplication";
    case RecoveryAction::recreate_graphics_device: return "recreate_graphics_device";
    case RecoveryAction::recreate_audio_client: return "recreate_audio_client";
    case RecoveryAction::await_target: return "await_target";
    case RecoveryAction::degrade_to_audio_only: return "degrade_to_audio_only";
    case RecoveryAction::block_session: return "block_session";
    }
    return "unknown";
}

std::string_view to_string(const CompositingDecision decision) noexcept {
    switch (decision) {
    case CompositingDecision::no_frame: return "no_frame";
    case CompositingDecision::pristine_no_patch: return "pristine_no_patch";
    case CompositingDecision::pristine_stale_frame: return "pristine_stale_frame";
    case CompositingDecision::pristine_stale_patch: return "pristine_stale_patch";
    case CompositingDecision::pristine_stale_evidence: return "pristine_stale_evidence";
    case CompositingDecision::pristine_low_confidence: return "pristine_low_confidence";
    case CompositingDecision::pristine_occluded: return "pristine_occluded";
    case CompositingDecision::pristine_wrong_generation: return "pristine_wrong_generation";
    case CompositingDecision::pristine_wrong_epoch: return "pristine_wrong_epoch";
    case CompositingDecision::pristine_wrong_frame: return "pristine_wrong_frame";
    case CompositingDecision::pristine_wrong_source_time: return "pristine_wrong_source_time";
    case CompositingDecision::pristine_unsafe_bounds: return "pristine_unsafe_bounds";
    case CompositingDecision::pristine_missing_texture: return "pristine_missing_texture";
    case CompositingDecision::patch: return "patch";
    }
    return "unknown";
}

} // namespace npc::media
