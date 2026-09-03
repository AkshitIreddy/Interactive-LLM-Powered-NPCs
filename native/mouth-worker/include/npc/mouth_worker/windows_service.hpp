#pragma once

#include "npc/mouth_worker/landmark_provider.hpp"
#include "npc/mouth_worker/service_protocol.hpp"

#include <cstdint>
#include <memory>
#include <optional>
#include <string>

namespace npc::mouth::service {

struct WindowsServiceConfig {
    std::wstring pipe_name;
    SessionBindingV1 session;
    std::uint32_t expected_controller_process_id{};
    std::uint64_t initial_generation{1};
    std::optional<AdmittedLandmarkProviderLaunchV1> landmark_provider_launch;
};

// Runs one authenticated local named-pipe service until Shutdown or peer loss.
// Returns zero only after a clean authenticated shutdown.
[[nodiscard]] int run_windows_service(
    const WindowsServiceConfig& config,
    std::unique_ptr<NativeLandmarkProviderV1> landmark_provider = {});

} // namespace npc::mouth::service
