#pragma once

#include "npc/mouth_worker/atlas.hpp"

#include <optional>

namespace npc::mouth {

class ReferenceMouthWorker final {
public:
    explicit ReferenceMouthWorker(std::uint64_t initial_generation = 1,
                                  WorkerPolicy policy = {});

    // Queue depth is exactly one. A newer item replaces unprocessed work.
    [[nodiscard]] bool submit(WorkItem item);

    // Generations only advance. Cancellation clears pending work synchronously.
    [[nodiscard]] bool cancel_to(std::uint64_t new_generation) noexcept;

    // Atomically replace the optional identity-bound runtime atlas. Invalid,
    // oversized, cross-generation, or cross-actor artifacts are rejected.
    [[nodiscard]] bool install_atlas(CharacterMouthAtlas atlas);
    void clear_atlas() noexcept;

    // The caller supplies the frame currently eligible for presentation. A job
    // for any other frame is consumed and fails open with no retained residual.
    [[nodiscard]] ProcessResult process_latest(const FrameIdentity& current_frame,
                                               Nanoseconds now_ns);

    [[nodiscard]] std::uint64_t active_generation() const noexcept;
    [[nodiscard]] bool has_pending_work() const noexcept;
    [[nodiscard]] const WorkerStats& stats() const noexcept;

private:
    [[nodiscard]] ProcessResult bypass(Disposition disposition) noexcept;
    [[nodiscard]] Disposition validate(const WorkItem& item,
                                       const FrameIdentity& current_frame,
                                       Nanoseconds now_ns) const noexcept;
    [[nodiscard]] MouthCoefficients smooth_drive_coefficients(
        const MouthCoefficients& target,
        const WorkItem& item) noexcept;
    [[nodiscard]] const CanonicalMouthPatch& smooth_atlas_appearance(
        const MouthAtlasState& target,
        const WorkItem& item);
    void reset_drive_smoothing() noexcept;
    void reset_atlas_selection() noexcept;

    std::uint64_t active_generation_{};
    WorkerPolicy policy_;
    std::optional<WorkItem> pending_;
    std::optional<CharacterMouthAtlas> atlas_;
    std::optional<MouthCoefficients> smoothed_drive_coefficients_;
    std::optional<TrackBinding> smoothed_drive_track_;
    std::uint64_t smoothed_drive_segment_id_{};
    Nanoseconds smoothed_drive_source_at_ns_{};
    Nanoseconds smoothed_drive_playback_at_ns_{};
    std::optional<CanonicalMouthPatch> smoothed_atlas_appearance_;
    std::optional<MouthCoefficients> smoothed_atlas_target_coefficients_;
    std::optional<TrackBinding> smoothed_atlas_track_;
    std::uint64_t smoothed_atlas_segment_id_{};
    Nanoseconds smoothed_atlas_source_at_ns_{};
    Nanoseconds smoothed_atlas_playback_at_ns_{};
    WorkerStats stats_;
};

} // namespace npc::mouth
