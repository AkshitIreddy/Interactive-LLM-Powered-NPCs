#include "npc/media_broker/service_config.hpp"

#include <algorithm>
#include <charconv>
#include <cctype>

namespace npc::media {

namespace {

[[nodiscard]] std::optional<std::string_view> option(std::span<const std::string_view> arguments,
                                                     const std::string_view name) {
    const std::string prefix = "--" + std::string(name) + "=";
    std::optional<std::string_view> result;
    for (const auto argument : arguments) {
        if (argument.starts_with(prefix)) {
            if (result) return std::nullopt;
            result = argument.substr(prefix.size());
        }
    }
    return result;
}

[[nodiscard]] int hex_digit(const char value) noexcept {
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return 10 + value - 'a';
    if (value >= 'A' && value <= 'F') return 10 + value - 'A';
    return -1;
}

} // namespace

bool valid_service_session_id(const std::string_view value) noexcept {
    return value.size() >= 8 && value.size() <= 64 &&
           std::all_of(value.begin(), value.end(), [](const unsigned char c) {
               return std::isalnum(c) != 0 || c == '-';
           });
}

std::optional<ServiceLaunchConfig> parse_service_launch_arguments(
    const std::span<const std::string_view> arguments) {
    // The service has a closed launch surface. In particular, there is no path,
    // module, hook, adapter, or plugin option that could become an executable
    // per-game integration route.
    if (arguments.size() != 3 ||
        std::any_of(arguments.begin(), arguments.end(), [](const std::string_view argument) {
            return !argument.starts_with("--parent-pid=") &&
                   !argument.starts_with("--session=") &&
                   !argument.starts_with("--nonce=");
        })) {
        return std::nullopt;
    }
    const auto parent = option(arguments, "parent-pid");
    const auto session = option(arguments, "session");
    const auto nonce = option(arguments, "nonce");
    if (!parent || !session || !nonce || !valid_service_session_id(*session) || nonce->size() != 64) {
        return std::nullopt;
    }

    ServiceLaunchConfig result;
    const auto parsed = std::from_chars(parent->data(), parent->data() + parent->size(), result.parent_process_id);
    if (parsed.ec != std::errc{} || parsed.ptr != parent->data() + parent->size() || result.parent_process_id == 0) {
        return std::nullopt;
    }
    result.session_id = std::string(*session);
    for (std::size_t index = 0; index < result.nonce.size(); ++index) {
        const int high = hex_digit((*nonce)[index * 2]);
        const int low = hex_digit((*nonce)[index * 2 + 1]);
        if (high < 0 || low < 0) return std::nullopt;
        result.nonce[index] = static_cast<std::byte>((high << 4) | low);
    }
    if (std::none_of(result.nonce.begin(), result.nonce.end(),
                     [](const std::byte value) { return value != std::byte{}; })) return std::nullopt;
    result.pipe_name = "\\\\.\\pipe\\npc-media-broker-" + result.session_id;
    return result;
}

} // namespace npc::media
