#pragma once

#ifdef _WIN32

#include "npc/media_broker/playback_transport.hpp"

#include <memory>
#include <array>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace npc::media::windows {

enum class AudioOutputState : std::uint32_t {
    active = 1,
    disabled = 2,
    not_present = 3,
    unplugged = 4,
};

struct AudioOutputEndpoint {
    std::string endpoint_id;
    std::string friendly_name;
    AudioOutputState state{AudioOutputState::not_present};
    bool system_default{};
    std::uint64_t generation{};
};

struct AudioOutputSnapshot {
    std::uint32_t schema_version{1};
    std::uint64_t catalog_generation{};
    std::vector<AudioOutputEndpoint> endpoints;
};

struct AudioOutputSelection {
    playback::AudioOutputSelectionMode mode{playback::AudioOutputSelectionMode::system_default};
    std::string endpoint_id;
};

struct SelectedAudioOutput {
    std::uint32_t schema_version{1};
    AudioOutputSelection selection;
    AudioOutputEndpoint resolved;
};

struct VisualAudioEnvelope {
    std::uint32_t schema_version{1};
    std::string session_id;
    std::string turn_id;
    std::string stream_id;
    std::string segment_id;
    std::uint64_t generation{};
    std::uint64_t source_sample_start{};
    std::uint32_t source_sample_count{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t device_write_qpc{};
    std::uint64_t qpc_frequency{};
    std::uint64_t source_frames{};
    std::uint64_t device_frames{};
    std::array<std::uint16_t, 8> mono_rms_q15{};
    std::array<std::uint16_t, 8> mono_peak_q15{};
    bool active{};
    bool draining{};
    bool cancelled{};
};

class PlaybackEndpoint final {
public:
    PlaybackEndpoint(playback::PlaybackLease lease,
                     std::uint32_t expected_producer_process_id,
                     std::uint64_t qpc_frequency,
                     std::shared_ptr<struct VisualAudioEnvelopeStore> envelope_store);
    ~PlaybackEndpoint();
    PlaybackEndpoint(const PlaybackEndpoint&) = delete;
    PlaybackEndpoint& operator=(const PlaybackEndpoint&) = delete;

    [[nodiscard]] bool start(std::string& error);
    void cancel() noexcept;
    [[nodiscard]] bool finished() const noexcept;
    [[nodiscard]] const playback::PlaybackLease& lease() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

class PlaybackService final {
public:
    explicit PlaybackService(std::string broker_session_id);
    ~PlaybackService();
    PlaybackService(const PlaybackService&) = delete;
    PlaybackService& operator=(const PlaybackService&) = delete;

    [[nodiscard]] std::optional<playback::PlaybackLease> allocate(
        const playback::AllocationRequest& request,
        std::uint64_t now_qpc,
        std::uint64_t qpc_frequency,
        std::string& error);
    [[nodiscard]] std::optional<AudioOutputSnapshot> enumerate_audio_outputs(
        std::string& error) const;
    [[nodiscard]] std::optional<SelectedAudioOutput> select_audio_output(
        const AudioOutputSelection& selection,
        std::string& error);
    [[nodiscard]] std::optional<SelectedAudioOutput> selected_audio_output(
        std::string& error) const;
    [[nodiscard]] std::optional<VisualAudioEnvelope> query_visual_audio_envelope(
        std::string_view session_id, std::string_view turn_id,
        std::uint64_t generation, std::string_view stream_id,
        std::string_view segment_id) const;
    void cancel_before_generation(std::uint64_t generation) noexcept;
    void cancel_all() noexcept;
    void reap_finished() noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace npc::media::windows

#endif
