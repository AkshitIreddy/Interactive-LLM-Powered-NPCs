#pragma once

#ifdef _WIN32

#include "npc/media_broker/audio_ring.hpp"
#include "npc/media_broker/types.hpp"

#include <memory>
#include <optional>

namespace npc::media::windows {

class EventDrivenAudio final {
public:
    EventDrivenAudio();
    ~EventDrivenAudio();
    EventDrivenAudio(const EventDrivenAudio&) = delete;
    EventDrivenAudio& operator=(const EventDrivenAudio&) = delete;

    [[nodiscard]] bool start(Failure& failure);
    void stop() noexcept;
    [[nodiscard]] bool restart(Failure& failure);
    [[nodiscard]] SharedPcmRing* capture_ring() noexcept;
    [[nodiscard]] SharedPcmRing* render_ring() noexcept;
    [[nodiscard]] std::optional<Failure> take_failure();

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace npc::media::windows

#endif
