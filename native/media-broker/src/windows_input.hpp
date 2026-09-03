#pragma once

#ifdef _WIN32

#include "npc/media_broker/input_transport.hpp"
#include "npc/media_broker/types.hpp"

#include <cstdint>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace npc::media::windows {

enum class AudioInputState : std::uint32_t {
    active = 1,
    disabled = 2,
    not_present = 3,
    unplugged = 4,
};

struct AudioInputEndpoint {
    std::string endpoint_id;
    std::string friendly_name;
    AudioInputState state{AudioInputState::not_present};
    bool system_default{};
    std::uint64_t generation{};
};

struct AudioInputSnapshot {
    std::uint32_t schema_version{1};
    std::uint64_t catalog_generation{};
    std::vector<AudioInputEndpoint> endpoints;
};

struct AudioInputSelection {
    playback::AudioOutputSelectionMode mode{
        playback::AudioOutputSelectionMode::system_default};
    std::string endpoint_id;
};

struct SelectedAudioInput {
    std::uint32_t schema_version{1};
    AudioInputSelection selection;
    AudioInputEndpoint resolved;
};

struct InputRehearsalAllocation {
    std::string session_id;
    std::string turn_id;
    std::uint64_t generation{};
    std::uint32_t duration_ms{};
    std::uint32_t sample_rate{};
    std::uint16_t channels{};
    std::uint64_t max_frames{};
    std::uint32_t expected_producer_process_id{};
    input::ActivationSource activation_source{input::ActivationSource::explicit_rehearsal};
};

struct PttActivationSnapshot {
    std::uint32_t virtual_key{};
    PttState state{PttState::released};
    std::uint64_t transition_sequence{};
    std::uint64_t transition_qpc{};
    std::uint64_t release_transition_sequence{};
    std::uint64_t released_qpc{};
};

class PttActivationTracker final {
public:
    void configure(std::uint32_t virtual_key, std::uint64_t at_qpc) noexcept;
    void transition(PttState state, std::uint64_t at_qpc) noexcept;
    [[nodiscard]] PttActivationSnapshot snapshot() const noexcept;

private:
    mutable std::mutex mutex_;
    PttActivationSnapshot snapshot_;
};

class InputRehearsalService final {
public:
    explicit InputRehearsalService(
        std::string broker_session_id,
        std::shared_ptr<PttActivationTracker> ptt_tracker = {});
    ~InputRehearsalService();
    InputRehearsalService(const InputRehearsalService&) = delete;
    InputRehearsalService& operator=(const InputRehearsalService&) = delete;

    [[nodiscard]] std::optional<AudioInputSnapshot> enumerate_audio_inputs(
        std::string& error) const;
    [[nodiscard]] std::optional<SelectedAudioInput> select_audio_input(
        const AudioInputSelection& selection, std::string& error);
    [[nodiscard]] std::optional<SelectedAudioInput> selected_audio_input(
        std::string& error) const;
    [[nodiscard]] std::optional<input::RehearsalLease> allocate(
        const InputRehearsalAllocation& request,
        std::uint64_t now_qpc,
        std::uint64_t qpc_frequency,
        std::string& error);
    [[nodiscard]] bool cancel(std::string_view stream_id,
                              std::uint64_t generation) noexcept;
    void cancel_all() noexcept;
    void reap_finished() noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace npc::media::windows

#endif
