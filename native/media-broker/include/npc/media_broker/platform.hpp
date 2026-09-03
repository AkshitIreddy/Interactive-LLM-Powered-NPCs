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
    [[nodiscard]] virtual bool overlay_capture_excluded() const noexcept = 0;
    // The default keeps portable/simulated platforms fail-closed. Windows is
    // the sole implementation that duplicates and opens worker GPU resources.
    [[nodiscard]] virtual bool import_shared_residual(const SharedResidualLease&,
                                                      std::uintptr_t& native_texture,
                                                      Failure& failure) {
        native_texture = 0;
        failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                   "Cross-process residual import is unavailable on this platform"};
        return false;
    }
    virtual void release_shared_residual() noexcept {}
    [[nodiscard]] virtual bool shared_residual_active() const noexcept { return false; }
    [[nodiscard]] virtual bool allocate_visual_source(const VisualSourceLeaseRequest&,
                                                      VisualSourceLease& lease,
                                                      Failure& failure) {
        lease = {};
        failure = {FailureDomain::capture, FailureCode::unsupported_path, false,
                   "Current-frame visual source leasing is unavailable on this platform"};
        return false;
    }
    [[nodiscard]] virtual bool release_visual_source(std::uint32_t,
                                                     std::uint64_t,
                                                     std::uint64_t) noexcept {
        return false;
    }
    virtual void cancel_visual_source_leases() noexcept {}
    [[nodiscard]] virtual bool allocate_identity_frame(const IdentityFrameLeaseRequest&,
                                                       IdentityFrameLease& lease,
                                                       Failure& failure) {
        lease = {};
        failure = {FailureDomain::capture, FailureCode::unsupported_path, false,
                   "CPU identity frame leasing is unavailable on this platform"};
        return false;
    }
    [[nodiscard]] virtual bool release_identity_frame(std::uint32_t,
                                                      std::string_view,
                                                      std::string_view) noexcept {
        return false;
    }
    virtual void cancel_identity_frame_leases() noexcept {}
    [[nodiscard]] virtual bool allocate_identity_reference_import(
        const IdentityReferenceImportRequest&,
        IdentityReferenceImportLease& lease,
        Failure& failure) {
        lease = {};
        failure = {FailureDomain::capture, FailureCode::unsupported_path, false,
                   "Native identity reference import is unavailable on this platform"};
        return false;
    }
    [[nodiscard]] virtual bool release_identity_reference_import(
        std::uint32_t, std::string_view, std::string_view) noexcept {
        return false;
    }
    [[nodiscard]] virtual bool begin_manual_actor_picker(
        const ManualActorPickerRequest&,
        ManualActorPickerReceipt& receipt,
        Failure& failure) {
        receipt = {};
        failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                   "Native manual actor selection is unavailable on this platform"};
        return false;
    }
    [[nodiscard]] virtual bool query_manual_actor_picker(
        std::string_view,
        ManualActorPickerReceipt& receipt,
        Failure& failure) {
        receipt = {};
        failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                   "Native manual actor selection is unavailable on this platform"};
        return false;
    }
    [[nodiscard]] virtual bool cancel_manual_actor_picker(
        std::string_view,
        ManualActorPickerReceipt& receipt,
        Failure& failure) {
        receipt = {};
        failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                   "Native manual actor selection is unavailable on this platform"};
        return false;
    }
    virtual void cancel_manual_actor_picker() noexcept {}
    [[nodiscard]] virtual bool present_shared_residual(const MouthPatch&, Failure& failure) {
        failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                   "Cross-process residual presentation is unavailable on this platform"};
        return false;
    }
    [[nodiscard]] virtual bool initialize_audio(Failure& failure) = 0;
    virtual void shutdown_audio() noexcept = 0;
    [[nodiscard]] virtual bool register_ptt_hotkey(std::uint32_t virtual_key, Failure& failure) = 0;
    virtual void unregister_ptt_hotkey() noexcept = 0;
    [[nodiscard]] virtual bool recreate_graphics_device(std::uint64_t generation, Failure& failure) = 0;
    [[nodiscard]] virtual bool recreate_audio_clients(Failure& failure) = 0;
    [[nodiscard]] virtual SharedPcmRing* capture_pcm_ring() noexcept = 0;
    [[nodiscard]] virtual SharedPcmRing* render_pcm_ring() noexcept = 0;
    // Immediately removes any previously visible residual without retaining a
    // game frame. Cancellation and malformed asynchronous output use this
    // fail-open path.
    virtual void suppress_residual() noexcept = 0;
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
        std::uint64_t residual_suppressions{};
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
    [[nodiscard]] bool overlay_capture_excluded() const noexcept override;
    [[nodiscard]] bool initialize_audio(Failure& failure) override;
    void shutdown_audio() noexcept override;
    [[nodiscard]] bool register_ptt_hotkey(std::uint32_t virtual_key, Failure& failure) override;
    void unregister_ptt_hotkey() noexcept override;
    [[nodiscard]] bool recreate_graphics_device(std::uint64_t generation, Failure& failure) override;
    [[nodiscard]] bool recreate_audio_clients(Failure& failure) override;
    [[nodiscard]] SharedPcmRing* capture_pcm_ring() noexcept override;
    [[nodiscard]] SharedPcmRing* render_pcm_ring() noexcept override;
    void suppress_residual() noexcept override;
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
    [[nodiscard]] bool residual_visible() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

#ifdef _WIN32
[[nodiscard]] std::unique_ptr<IMediaPlatform> create_windows_media_platform();
#endif

} // namespace npc::media
