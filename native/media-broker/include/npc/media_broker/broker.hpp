#pragma once

#include "npc/media_broker/frame_mailbox.hpp"
#include "npc/media_broker/geometry.hpp"
#include "npc/media_broker/platform.hpp"

#include <memory>
#include <optional>

namespace npc::media {

enum class CompositingDecision {
    no_frame,
    pristine_no_patch,
    pristine_stale_frame,
    pristine_stale_patch,
    pristine_stale_evidence,
    pristine_low_confidence,
    pristine_occluded,
    pristine_wrong_generation,
    pristine_wrong_epoch,
    pristine_wrong_frame,
    pristine_wrong_source_time,
    pristine_wrong_track,
    pristine_unsafe_bounds,
    pristine_missing_texture,
    patch,
};

struct BrokerEventSink {
    std::function<void(const Diagnostics&)> on_diagnostics;
    std::function<void(PttState, MonotonicTime)> on_ptt;
    std::function<void(CompositingDecision)> on_compositing_decision;
};

class MediaBroker final {
public:
    explicit MediaBroker(std::unique_ptr<IMediaPlatform> platform,
                         BrokerPolicy policy = {},
                         BrokerEventSink sink = {});
    ~MediaBroker();
    MediaBroker(const MediaBroker&) = delete;
    MediaBroker& operator=(const MediaBroker&) = delete;

    [[nodiscard]] bool start(std::uint32_t ptt_virtual_key = 0x77); // F8
    void stop() noexcept;
    [[nodiscard]] bool select_target(GameTarget target);
    void clear_target() noexcept;
    void submit_patch(MouthPatch patch);
    void submit_occlusion_evidence(OcclusionEvidence evidence);
    void cancel_generation(std::uint64_t new_generation) noexcept;
    void tick(MonotonicTime now = std::chrono::steady_clock::now());

    [[nodiscard]] const Diagnostics& diagnostics() const noexcept;
    [[nodiscard]] IMediaPlatform& platform() noexcept;

private:
    void configure_callbacks();
    void handle_frame(FrameDescriptor frame);
    void handle_failure(Failure failure);
    void handle_target_state(TargetState state, std::optional<TargetGeometry> geometry);
    void invalidate_geometry_epoch(TargetGeometry& geometry);
    void reset_frame_evidence() noexcept;
    void attempt_recovery(MonotonicTime now);
    [[nodiscard]] bool activate_capture(CaptureBackend backend);
    [[nodiscard]] CompositingDecision decide_compositing(const FrameDescriptor& frame,
                                                         const std::optional<MouthPatch>& patch,
                                                         MonotonicTime now) const noexcept;
    void publish();
    void fail(Failure failure, BrokerState state, RecoveryAction recovery);

    std::unique_ptr<IMediaPlatform> platform_;
    BrokerPolicy policy_;
    BrokerEventSink sink_;
    Diagnostics diagnostics_;
    std::optional<GameTarget> target_;
    std::optional<TargetGeometry> target_geometry_;
    std::optional<OverlayGeometry> overlay_geometry_;
    std::optional<OcclusionEvidence> occlusion_evidence_;
    LatestValueMailbox<FrameDescriptor> frames_;
    LatestValueMailbox<MouthPatch> patches_;
    MonotonicTime recovery_due_{};
    std::uint32_t same_backend_retries_{};
    bool running_{};
};

[[nodiscard]] std::string_view to_string(CompositingDecision decision) noexcept;

} // namespace npc::media
