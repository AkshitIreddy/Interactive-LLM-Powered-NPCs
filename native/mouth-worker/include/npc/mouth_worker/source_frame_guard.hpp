#pragma once

#include "npc/mouth_worker/types.hpp"

#include <cstdint>
#include <memory>
#include <string_view>

namespace npc::mouth {

// This guard measures source-frame appearance continuity. It deliberately
// does not assign semantic causes such as smoke, hands, helmets, or speech.
enum class SourceFrameGuardDisposition : std::uint8_t {
  warming_up,
  accepted,
  bypass_cancelled,
  bypass_invalid_source,
  bypass_wrong_frame,
  bypass_tracking_loss,
  bypass_scene_change,
  bypass_mouth_appearance_change,
  bypass_recovering,
};

[[nodiscard]] constexpr std::string_view
to_string(const SourceFrameGuardDisposition value) noexcept {
  switch (value) {
  case SourceFrameGuardDisposition::warming_up:
    return "warming_up";
  case SourceFrameGuardDisposition::accepted:
    return "accepted";
  case SourceFrameGuardDisposition::bypass_cancelled:
    return "bypass_cancelled";
  case SourceFrameGuardDisposition::bypass_invalid_source:
    return "bypass_invalid_source";
  case SourceFrameGuardDisposition::bypass_wrong_frame:
    return "bypass_wrong_frame";
  case SourceFrameGuardDisposition::bypass_tracking_loss:
    return "bypass_tracking_loss";
  case SourceFrameGuardDisposition::bypass_scene_change:
    return "bypass_scene_change";
  case SourceFrameGuardDisposition::bypass_mouth_appearance_change:
    return "bypass_mouth_appearance_change";
  case SourceFrameGuardDisposition::bypass_recovering:
    return "bypass_recovering";
  }
  return "unknown";
}

struct SourceFrameGuardPolicy {
  std::uint32_t warmup_observations{6};
  std::uint32_t recovery_observations{3};
  double baseline_alpha{0.12};

  // Descriptor distances are normalized by these dimensionless limits.
  // They are generic continuity defaults, not character or smoke classes.
  double maximum_luma_histogram_distance{0.22};
  double maximum_chroma_histogram_distance{0.18};
  double maximum_gradient_histogram_distance{0.24};
  double maximum_spatial_color_distance{0.13};
  double maximum_spatial_texture_distance{0.12};

  double appearance_trigger_score{1.0};
  double scene_trigger_score{1.20};
  double recovery_score{0.75};
  double minimum_valid_sample_ratio{0.98};
};

struct SourceFrameGuardEvidence {
  double mouth_baseline_innovation{};
  double mouth_step_innovation{};
  double face_baseline_innovation{};
  double face_step_innovation{};
  double mouth_valid_sample_ratio{};
  double face_valid_sample_ratio{};
  // Conservative continuity score derived from descriptor innovation. It
  // is not a probability that the mouth is visible or unoccluded.
  double appearance_continuity_confidence{};
  std::uint32_t baseline_observations{};
  std::uint32_t consecutive_recovery_observations{};
  std::uint64_t latch_generation{};
};

struct SourceFrameGuardDecision {
  SourceFrameGuardDisposition disposition{
      SourceFrameGuardDisposition::bypass_invalid_source};
  SourceFrameGuardEvidence evidence;

  [[nodiscard]] bool accepted() const noexcept {
    return disposition == SourceFrameGuardDisposition::accepted;
  }
};

class SourceFrameAppearanceGuard final {
public:
  explicit SourceFrameAppearanceGuard(std::uint64_t initial_generation = 1,
                                      SourceFrameGuardPolicy policy = {});
  ~SourceFrameAppearanceGuard();

  SourceFrameAppearanceGuard(SourceFrameAppearanceGuard &&) noexcept;
  SourceFrameAppearanceGuard &operator=(SourceFrameAppearanceGuard &&) noexcept;
  SourceFrameAppearanceGuard(const SourceFrameAppearanceGuard &) = delete;
  SourceFrameAppearanceGuard &
  operator=(const SourceFrameAppearanceGuard &) = delete;

  [[nodiscard]] SourceFrameGuardDecision
  evaluate(const CpuFrame &source, const TrackingEvidence &tracking);

  // Call when the upstream tracker emits no authoritative observation.
  // Recovery starts with a fresh appearance baseline rather than reusing
  // pixels from a stale pose or a possibly different actor.
  void notify_tracking_loss() noexcept;
  [[nodiscard]] bool cancel_to(std::uint64_t generation) noexcept;
  void reset_track() noexcept;

  [[nodiscard]] std::uint64_t active_generation() const noexcept;
  [[nodiscard]] std::uint64_t latch_generation() const noexcept;
  [[nodiscard]] bool appearance_latched() const noexcept;

private:
  struct State;

  SourceFrameGuardPolicy policy_;
  std::uint64_t active_generation_{};
  std::uint64_t latch_generation_{1};
  std::unique_ptr<State> state_;
};

} // namespace npc::mouth
