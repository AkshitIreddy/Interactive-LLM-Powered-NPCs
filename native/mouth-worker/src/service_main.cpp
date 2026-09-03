#include "npc/mouth_worker/windows_service.hpp"

#include <charconv>
#include <array>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <limits>
#include <optional>
#include <span>
#include <string>
#include <string_view>

namespace {

using npc::mouth::service::SessionBindingV1;
using npc::mouth::service::WindowsServiceConfig;

[[nodiscard]] std::optional<std::uint64_t> parse_u64(const std::string_view value) {
    std::uint64_t result{};
    const auto parsed = std::from_chars(value.data(), value.data() + value.size(), result);
    if (parsed.ec != std::errc{} || parsed.ptr != value.data() + value.size()) return std::nullopt;
    return result;
}

[[nodiscard]] std::optional<std::array<std::byte, 32>> parse_nonce(
    const std::string_view value) {
    if (value.size() != 64U) return std::nullopt;
    std::array<std::byte, 32> result{};
    const auto nibble = [](const char character) -> std::optional<std::uint8_t> {
        if (character >= '0' && character <= '9') {
            return static_cast<std::uint8_t>(character - '0');
        }
        if (character >= 'a' && character <= 'f') {
            return static_cast<std::uint8_t>(character - 'a' + 10);
        }
        if (character >= 'A' && character <= 'F') {
            return static_cast<std::uint8_t>(character - 'A' + 10);
        }
        return std::nullopt;
    };
    for (std::size_t index = 0; index < result.size(); ++index) {
        const auto high = nibble(value[index * 2U]);
        const auto low = nibble(value[index * 2U + 1U]);
        if (!high || !low) return std::nullopt;
        result[index] = static_cast<std::byte>((*high << 4U) | *low);
    }
    return result;
}

[[nodiscard]] std::optional<std::string_view> argument(const std::span<char*> arguments,
                                                       const std::string_view name) {
    for (std::size_t index = 1U; index + 1U < arguments.size(); ++index) {
        if (std::string_view(arguments[index]) == name) return arguments[index + 1U];
    }
    return std::nullopt;
}

} // namespace

int main(const int argc, char** argv) {
#ifdef _WIN32
    const std::span<char*> arguments(argv, static_cast<std::size_t>(argc));
    const auto pipe = argument(arguments, "--pipe");
    const auto nonce_text = argument(arguments, "--nonce-hex");
    const auto session_high_text = argument(arguments, "--session-high");
    const auto session_low_text = argument(arguments, "--session-low");
    const auto controller_text = argument(arguments, "--controller-pid");
    const auto generation_text = argument(arguments, "--generation");
    if (!pipe || !nonce_text || !session_high_text || !session_low_text || !controller_text ||
        !generation_text) {
        std::cerr << "mouth-worker requires the supervised service arguments\n";
        return 2;
    }
    const auto nonce = parse_nonce(*nonce_text);
    const auto session_high = parse_u64(*session_high_text);
    const auto session_low = parse_u64(*session_low_text);
    const auto controller = parse_u64(*controller_text);
    const auto generation = parse_u64(*generation_text);
    if (!nonce || !session_high || !session_low || !controller || !generation ||
        *controller == 0U ||
        *controller > std::numeric_limits<std::uint32_t>::max() || *generation == 0U ||
        (*session_high == 0U && *session_low == 0U)) {
        std::cerr << "mouth-worker rejected malformed service arguments\n";
        return 2;
    }
    WindowsServiceConfig config{};
    config.pipe_name.assign(pipe->begin(), pipe->end());
    config.session = SessionBindingV1{*nonce, *session_high, *session_low};
    config.expected_controller_process_id = static_cast<std::uint32_t>(*controller);
    config.initial_generation = *generation;
    return npc::mouth::service::run_windows_service(
        config, npc::mouth::make_windows_ort_landmark_provider_v1());
#else
    (void)argc;
    (void)argv;
    std::cerr << "mouth-worker service is available only on Windows\n";
    return 2;
#endif
}
