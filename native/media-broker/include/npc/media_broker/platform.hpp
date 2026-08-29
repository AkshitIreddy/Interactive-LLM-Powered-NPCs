#pragma once

#include "npc/media_broker/types.hpp"
#include "npc/media_broker/audio_ring.hpp"

#include <functional>
#include <memory>
#include <span>
#include <string>

namespace npc::media {

struct OverlayGeometry;

struct PlatformCallbacks {
    std::function<void(FrameDescriptor)> on_frame;
    std::function<void(PttState, MonotonicTime)> on_ptt;
    std::function<void(Failure)> on_failure;
    std::function<void(TargetState, std::optional<TargetGeometry>)> on_target_state;
};

class IMediaPlatform {
public:
    virtual ~IMediaPlatform() = default;

    virtual void set_callbacks(PlatformCallbacks callbacks) = 0;
    [[nodiscard]] virtual bool validate_target(const GameTarget& target, Failure& failure) = 0;
    [[nodiscard]] virtual bool start_capture(const GameTarget& target, CaptureBackend backend, Failure& failure) = 0;
    virtual void stop_capture() noexcept = 0;
    [[nodiscard]] virtual bool start_overlay(const TargetGeometry& geometry, Failure& failure) = 0;
    virtual void stop_overlay() noexcept = 0;
    [[nodiscard]] virtual bool initialize_audio(Failure& failure) = 0;
    virtual void shutdown_audio() noexcept = 0;
    [[nodiscard]] virtual bool register_ptt_hotkey(std::uint32_t virtual_key, Failure& failure) = 0;
    virtual void unregister_ptt_hotkey() noexcept = 0;
    [[nodiscard]] virtual bool recreate_graphics_device(std::uint64_t generation, Failure& failure) = 0;
    [[nodiscard]] virtual bool recreate_audio_clients(Failure& failure) = 0;
    [[nodiscard]] virtual SharedPcmRing* capture_pcm_ring() noexcept = 0;
    [[nodiscard]] virtual SharedPcmRing* render_pcm_ring() noexcept = 0;
    virtual void present_pristine(const FrameDescriptor& frame, const OverlayGeometry& geometry) = 0;
    virtual void present_patch(const FrameDescriptor& frame,
                               const MouthPatch& patch,
                               RectI patch_bounds,
                               const OverlayGeometry& geometry) = 0;
    virtual void poll() = 0;
};

class SimulatedMediaPlatform final : public IMediaPlatform {
public:
    struct Counters {
        std::uint64_t capture_starts{};
        std::uint64_t overlay_starts{};
        std::uint64_t pristine_presentations{};
        std::uint64_t patch_presentations{};
        std::uint64_t graphics_recreates{};
        std::uint64_t audio_recreates{};
    };

    SimulatedMediaPlatform();
    ~SimulatedMediaPlatform() override;
    SimulatedMediaPlatform(const SimulatedMediaPlatform&) = delete;
    SimulatedMediaPlatform& operator=(const SimulatedMediaPlatform&) = delete;

    void set_callbacks(PlatformCallbacks callbacks) override;
    [[nodiscard]] bool validate_target(const GameTarget& target, Failure& failure) override;
    [[nodiscard]] bool start_capture(const GameTarget& target, CaptureBackend backend, Failure& failure) override;
    void stop_capture() noexcept override;
    [[nodiscard]] bool start_overlay(const TargetGeometry& geometry, Failure& failure) override;
    void stop_overlay() noexcept override;
    [[nodiscard]] bool initialize_audio(Failure& failure) override;
    void shutdown_audio() noexcept override;
    [[nodiscard]] bool register_ptt_hotkey(std::uint32_t virtual_key, Failure& failure) override;
    void unregister_ptt_hotkey() noexcept override;
    [[nodiscard]] bool recreate_graphics_device(std::uint64_t generation, Failure& failure) override;
    [[nodiscard]] bool recreate_audio_clients(Failure& failure) override;
    [[nodiscard]] SharedPcmRing* capture_pcm_ring() noexcept override;
    [[nodiscard]] SharedPcmRing* render_pcm_ring() noexcept override;
    void present_pristine(const FrameDescriptor& frame, const OverlayGeometry& geometry) override;
    void present_patch(const FrameDescriptor& frame,
                       const MouthPatch& patch,
                       RectI patch_bounds,
                       const OverlayGeometry& geometry) override;
    void poll() override;

    void emit_frame(FrameDescriptor frame);
    void emit_ptt(PttState state, MonotonicTime at = std::chrono::steady_clock::now());
    void emit_failure(Failure failure);
    void emit_target(TargetState state, std::optional<TargetGeometry> geometry = std::nullopt);
    void fail_next_capture(CaptureBackend backend, Failure failure);
    void fail_next_graphics_recreate(Failure failure);
    void set_target_valid(bool valid);
    [[nodiscard]] CaptureBackend active_capture() const noexcept;
    [[nodiscard]] const Counters& counters() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

#ifdef _WIN32
[[nodiscard]] std::unique_ptr<IMediaPlatform> create_windows_media_platform();
#endif

} // namespace npc::media
