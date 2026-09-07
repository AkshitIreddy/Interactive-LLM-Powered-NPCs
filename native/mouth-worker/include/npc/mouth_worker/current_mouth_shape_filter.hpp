#pragma once

#include "npc/mouth_worker/types.hpp"

#include <array>
#include <cstdint>
#include <optional>

namespace npc::mouth {

// Smooths only normalized mouth-local contour shape. Every returned sample is
// reconstructed from the newest centre, corner span, roll, and scale, so
// character/camera motion never inherits the delay of landmark noise damping.
class CurrentMouthShapeFilter {
public:
  [[nodiscard]] TrackingEvidence filter(const TrackingEvidence &current,
                                        std::uint32_t frame_width,
                                        std::uint32_t frame_height);

  void reset() noexcept;

private:
  struct LocalPoint {
    double x{};
    double y{};
  };

  struct State {
    TrackBinding track;
    Nanoseconds measured_at_ns{};
    std::uint32_t frame_width{};
    std::uint32_t frame_height{};
    std::array<LocalPoint, mouth_contour_point_count> contour{};
  };

  std::optional<State> previous_;
};

} // namespace npc::mouth
