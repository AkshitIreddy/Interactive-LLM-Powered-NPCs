#pragma once

#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

namespace npc::media {

enum class TargetBlockReason {
    none,
    invalid_window,
    minimized,
    protected_process,
    process_mismatch,
    cross_session,
    cross_user,
    inspection_incomplete,
    anti_cheat_marker,
    online_ambiguity,
    not_allowlisted,
};

struct TargetInspectionEvidence {
    bool valid_window{};
    bool minimized{};
    bool protected_process{};
    bool process_id_matches{};
    bool session_matches{};
    bool user_matches{};
    bool inspection_complete{};
    std::uint32_t process_id{};
    std::string process_name;
    std::vector<std::string> loaded_module_names;
};

struct TargetPolicyDecision {
    bool capture_allowed{};
    TargetBlockReason reason{TargetBlockReason::inspection_incomplete};
    std::string matched_marker;
};

[[nodiscard]] TargetPolicyDecision evaluate_target_policy(
    const TargetInspectionEvidence& evidence,
    const std::vector<std::string>& trusted_allowed_process_names);
[[nodiscard]] bool contains_anti_cheat_marker(std::string_view name);
[[nodiscard]] bool is_online_ambiguous_process(std::string_view process_name);
[[nodiscard]] std::string_view to_string(TargetBlockReason reason) noexcept;

} // namespace npc::media
