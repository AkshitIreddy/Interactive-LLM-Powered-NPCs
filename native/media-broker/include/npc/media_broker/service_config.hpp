#pragma once

#include "npc/media_broker/ipc.hpp"

#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <string_view>

namespace npc::media {

struct ServiceLaunchConfig {
    std::uint32_t parent_process_id{};
    std::string session_id;
    std::array<std::byte, ipc::launch_nonce_bytes> nonce{};
    std::string pipe_name;
};

[[nodiscard]] std::optional<ServiceLaunchConfig> parse_service_launch_arguments(
    std::span<const std::string_view> arguments);
[[nodiscard]] bool valid_service_session_id(std::string_view value) noexcept;

} // namespace npc::media
