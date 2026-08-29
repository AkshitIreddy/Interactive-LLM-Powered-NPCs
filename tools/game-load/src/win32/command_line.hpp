#pragma once

#include "game_load/policy.hpp"

#include <string>

namespace game_load::win32 {

struct ParsedCommandLine {
  WorkloadConfig config;
  bool show_help = false;
  bool smoke = false;
  std::string error;
};

ParsedCommandLine parse_command_line(int argc, wchar_t** argv);
void print_help();
void print_dry_run_summary(const WorkloadConfig& config);

}  // namespace game_load::win32
