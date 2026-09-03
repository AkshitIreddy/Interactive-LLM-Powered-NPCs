#pragma once

#include "npc/mouth_worker/types.hpp"

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

    std::uint64_t active_generation_{};
    WorkerPolicy policy_;
    std::optional<WorkItem> pending_;
    WorkerStats stats_;
};

} // namespace npc::mouth
