#pragma once

#ifdef _WIN32

#include "npc/media_broker/ipc.hpp"
#include "npc/media_broker/service_config.hpp"
#include "npc/media_broker/target_policy.hpp"

#include <Windows.h>

#include <atomic>
#include <memory>
#include <optional>

namespace npc::media::windows {

[[nodiscard]] TargetInspectionEvidence inspect_target(HWND window, std::uint32_t expected_process_id);
[[nodiscard]] bool process_is_current_user_and_session(HANDLE process) noexcept;
[[nodiscard]] bool verify_parent_and_job(const ServiceLaunchConfig& config, HANDLE& parent_handle) noexcept;
[[nodiscard]] std::uint64_t qpc_now() noexcept;
[[nodiscard]] std::uint64_t qpc_frequency() noexcept;

class CurrentUserPipeServer final {
public:
    CurrentUserPipeServer(std::string pipe_name, std::uint32_t expected_client_process_id);
    ~CurrentUserPipeServer();
    CurrentUserPipeServer(const CurrentUserPipeServer&) = delete;
    CurrentUserPipeServer& operator=(const CurrentUserPipeServer&) = delete;

    [[nodiscard]] bool start();
    void stop() noexcept;
    [[nodiscard]] std::optional<ipc::Envelope> take_request();
    [[nodiscard]] bool submit_response(ipc::Response response);
    [[nodiscard]] bool connected() const noexcept;
    [[nodiscard]] bool disconnected() const noexcept;
    [[nodiscard]] bool client_trusted() const noexcept;
    [[nodiscard]] bool response_sent(std::uint64_t sequence) const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace npc::media::windows

#endif
