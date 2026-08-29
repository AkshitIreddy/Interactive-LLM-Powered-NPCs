#include "npc/media_broker/target_policy.hpp"

#include <algorithm>
#include <array>
#include <cctype>

namespace npc::media {

namespace {

[[nodiscard]] std::string lower(std::string_view value) {
    std::string result(value);
    std::transform(result.begin(), result.end(), result.begin(),
                   [](const unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return result;
}

constexpr std::array anti_cheat_markers{
    "easyanticheat", "easyanticheat_eos", "eac_launcher", "battleye", "beservice",
    "bedaisy", "vgk", "vgc", "ricochet", "faceit", "xigncode", "xhunter",
    "nprotect", "gameguard", "mhyprot", "equ8", "ace-base", "ace-guard",
};

constexpr std::array online_ambiguous_processes{
    "gta5.exe", "playrdr2.exe", "rdr2.exe", "eldenring.exe", "helldivers2.exe",
    "destiny2.exe", "cod.exe", "fortniteclient-win64-shipping.exe", "valorant-win64-shipping.exe",
};

} // namespace

bool contains_anti_cheat_marker(const std::string_view name) {
    const auto normalized = lower(name);
    return std::any_of(anti_cheat_markers.begin(), anti_cheat_markers.end(),
                       [&](const std::string_view marker) { return normalized.find(marker) != std::string::npos; });
}

bool is_online_ambiguous_process(const std::string_view process_name) {
    const auto normalized = lower(process_name);
    return std::any_of(online_ambiguous_processes.begin(), online_ambiguous_processes.end(),
                       [&](const std::string_view value) { return normalized == value; });
}

TargetPolicyDecision evaluate_target_policy(
    const TargetInspectionEvidence& evidence,
    const std::vector<std::string>& trusted_allowed_process_names) {
    const auto block = [](const TargetBlockReason reason, std::string marker = {}) {
        return TargetPolicyDecision{false, reason, std::move(marker)};
    };
    if (!evidence.valid_window) return block(TargetBlockReason::invalid_window);
    if (evidence.minimized) return block(TargetBlockReason::minimized);
    if (evidence.protected_process) return block(TargetBlockReason::protected_process);
    if (!evidence.process_id_matches) return block(TargetBlockReason::process_mismatch);
    if (!evidence.session_matches) return block(TargetBlockReason::cross_session);
    if (!evidence.user_matches) return block(TargetBlockReason::cross_user);
    if (!evidence.inspection_complete || evidence.process_name.empty()) return block(TargetBlockReason::inspection_incomplete);
    if (contains_anti_cheat_marker(evidence.process_name)) return block(TargetBlockReason::anti_cheat_marker, evidence.process_name);
    for (const auto& module : evidence.loaded_module_names) {
        if (contains_anti_cheat_marker(module)) return block(TargetBlockReason::anti_cheat_marker, module);
    }
    if (is_online_ambiguous_process(evidence.process_name)) return block(TargetBlockReason::online_ambiguity, evidence.process_name);
    const auto process = lower(evidence.process_name);
    const bool allowed = std::any_of(trusted_allowed_process_names.begin(), trusted_allowed_process_names.end(),
                                     [&](const std::string& entry) { return lower(entry) == process; });
    if (!allowed) return block(TargetBlockReason::not_allowlisted);
    return {true, TargetBlockReason::none, {}};
}

std::string_view to_string(const TargetBlockReason reason) noexcept {
    switch (reason) {
    case TargetBlockReason::none: return "none"; case TargetBlockReason::invalid_window: return "invalid_window";
    case TargetBlockReason::minimized: return "minimized"; case TargetBlockReason::protected_process: return "protected_process";
    case TargetBlockReason::process_mismatch: return "process_mismatch"; case TargetBlockReason::cross_session: return "cross_session";
    case TargetBlockReason::cross_user: return "cross_user"; case TargetBlockReason::inspection_incomplete: return "inspection_incomplete";
    case TargetBlockReason::anti_cheat_marker: return "anti_cheat_marker"; case TargetBlockReason::online_ambiguity: return "online_ambiguity";
    case TargetBlockReason::not_allowlisted: return "not_allowlisted";
    }
    return "unknown";
}

} // namespace npc::media
