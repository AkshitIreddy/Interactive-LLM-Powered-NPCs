#include "npc/media_broker/platform.hpp"

#include "npc/media_broker/geometry.hpp"

#include <map>
#include <utility>

namespace npc::media {

struct SimulatedMediaPlatform::Impl {
    Impl()
        : capture_ring({48000, 2, 32, 8, PcmSampleKind::floating_point}, 4800),
          render_ring({48000, 2, 32, 8, PcmSampleKind::floating_point}, 4800) {}

    PlatformCallbacks callbacks;
    Counters counters;
    CaptureBackend active_capture{CaptureBackend::none};
    bool target_valid{true};
    bool audio_initialized{};
    bool overlay_active{};
    bool hotkey_registered{};
    std::map<CaptureBackend, Failure> capture_failures;
    std::optional<Failure> graphics_recreate_failure;
    SharedPcmRing capture_ring;
    SharedPcmRing render_ring;
};

SimulatedMediaPlatform::SimulatedMediaPlatform() : impl_(std::make_unique<Impl>()) {}
SimulatedMediaPlatform::~SimulatedMediaPlatform() = default;

void SimulatedMediaPlatform::set_callbacks(PlatformCallbacks callbacks) {
    impl_->callbacks = std::move(callbacks);
}

bool SimulatedMediaPlatform::validate_target(const GameTarget& target, Failure& failure) {
    if (!impl_->target_valid || !target.valid()) {
        failure = {FailureDomain::target, FailureCode::target_lost, false, "Simulated target is invalid"};
        return false;
    }
    return true;
}

bool SimulatedMediaPlatform::start_capture(const GameTarget&, const CaptureBackend backend, Failure& failure) {
    ++impl_->counters.capture_starts;
    if (const auto found = impl_->capture_failures.find(backend); found != impl_->capture_failures.end()) {
        failure = found->second;
        impl_->capture_failures.erase(found);
        return false;
    }
    if (backend == CaptureBackend::none) {
        failure = {FailureDomain::capture, FailureCode::backend_unavailable, false, "No capture backend selected"};
        return false;
    }
    impl_->active_capture = backend;
    return true;
}

void SimulatedMediaPlatform::stop_capture() noexcept { impl_->active_capture = CaptureBackend::none; }

bool SimulatedMediaPlatform::start_overlay(const TargetGeometry& geometry, Failure& failure) {
    ++impl_->counters.overlay_starts;
    if (!calculate_overlay_geometry(geometry)) {
        failure = {FailureDomain::overlay, FailureCode::invalid_geometry, true, "Invalid simulated overlay geometry"};
        return false;
    }
    impl_->overlay_active = true;
    return true;
}

void SimulatedMediaPlatform::stop_overlay() noexcept { impl_->overlay_active = false; }

bool SimulatedMediaPlatform::initialize_audio(Failure&) {
    impl_->audio_initialized = true;
    return true;
}

void SimulatedMediaPlatform::shutdown_audio() noexcept { impl_->audio_initialized = false; }

bool SimulatedMediaPlatform::register_ptt_hotkey(const std::uint32_t virtual_key, Failure& failure) {
    if (virtual_key == 0) {
        failure = {FailureDomain::hotkey, FailureCode::hotkey_conflict, true, "Virtual key 0 is invalid"};
        return false;
    }
    impl_->hotkey_registered = true;
    return true;
}

void SimulatedMediaPlatform::unregister_ptt_hotkey() noexcept { impl_->hotkey_registered = false; }

bool SimulatedMediaPlatform::recreate_graphics_device(const std::uint64_t, Failure& failure) {
    ++impl_->counters.graphics_recreates;
    if (impl_->graphics_recreate_failure) {
        failure = std::move(*impl_->graphics_recreate_failure);
        impl_->graphics_recreate_failure.reset();
        return false;
    }
    return true;
}

bool SimulatedMediaPlatform::recreate_audio_clients(Failure&) {
    ++impl_->counters.audio_recreates;
    impl_->capture_ring.clear();
    impl_->render_ring.clear();
    impl_->audio_initialized = true;
    return true;
}

SharedPcmRing* SimulatedMediaPlatform::capture_pcm_ring() noexcept { return &impl_->capture_ring; }
SharedPcmRing* SimulatedMediaPlatform::render_pcm_ring() noexcept { return &impl_->render_ring; }

void SimulatedMediaPlatform::present_pristine(const FrameDescriptor&, const OverlayGeometry&) {
    ++impl_->counters.pristine_presentations;
}

void SimulatedMediaPlatform::present_patch(const FrameDescriptor&,
                                           const MouthPatch&,
                                           const RectI,
                                           const OverlayGeometry&) {
    ++impl_->counters.patch_presentations;
}

void SimulatedMediaPlatform::poll() {}

void SimulatedMediaPlatform::emit_frame(FrameDescriptor frame) {
    if (impl_->callbacks.on_frame) {
        impl_->callbacks.on_frame(std::move(frame));
    }
}

void SimulatedMediaPlatform::emit_ptt(const PttState state, const MonotonicTime at) {
    if (impl_->callbacks.on_ptt) {
        impl_->callbacks.on_ptt(state, at);
    }
}

void SimulatedMediaPlatform::emit_failure(Failure failure) {
    if (impl_->callbacks.on_failure) {
        impl_->callbacks.on_failure(std::move(failure));
    }
}

void SimulatedMediaPlatform::emit_target(const TargetState state, std::optional<TargetGeometry> geometry) {
    if (impl_->callbacks.on_target_state) {
        impl_->callbacks.on_target_state(state, std::move(geometry));
    }
}

void SimulatedMediaPlatform::fail_next_capture(const CaptureBackend backend, Failure failure) {
    impl_->capture_failures[backend] = std::move(failure);
}

void SimulatedMediaPlatform::fail_next_graphics_recreate(Failure failure) {
    impl_->graphics_recreate_failure = std::move(failure);
}

void SimulatedMediaPlatform::set_target_valid(const bool valid) { impl_->target_valid = valid; }
CaptureBackend SimulatedMediaPlatform::active_capture() const noexcept { return impl_->active_capture; }
const SimulatedMediaPlatform::Counters& SimulatedMediaPlatform::counters() const noexcept { return impl_->counters; }

} // namespace npc::media
