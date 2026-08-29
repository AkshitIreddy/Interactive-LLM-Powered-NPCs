#pragma once

#include "harness_types.hpp"

#include <string>

namespace game_load::win32 {

std::string result_manifest_json(const RunResult& result);
bool write_result_manifest(const RunResult& result, const std::string& path,
                           std::string* error = nullptr);

}  // namespace game_load::win32
