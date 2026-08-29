#include "command_line.hpp"
#include "d3d12_harness.hpp"
#include "result_manifest.hpp"
#include "system_monitor.hpp"

#include <algorithm>
#include <iostream>
#include <string>
#include <thread>

namespace {

game_load::win32::RunResult dry_run_result(const game_load::WorkloadConfig& config) {
  using namespace game_load::win32;
  RunResult result;
  result.config = config;
  result.dry_run = true;
  result.completed = true;
  result.power_settings_changed = false;
  result.stop_reason = game_load::SafetyAbort::none;
  result.stop_detail = "Dry-run validation completed; no workload was started";
  result.started_utc = utc_now_iso8601();
  result.ended_utc = result.started_utc;
  result.harness_version = GAME_LOAD_VERSION;
  result.operating_system = query_operating_system();
  result.cpu_name = query_cpu_name();
  result.logical_cpu_count = std::max(1U, std::thread::hardware_concurrency());
  result.initial_system_memory = query_system_memory();
  result.ram_allocation = game_load::decide_ram_allocation(
      config, result.initial_system_memory);
  result.warnings.emplace_back(
      "Dry-run does not initialize D3D12, query a GPU adapter, allocate memory, or apply load");
  return result;
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
  using namespace game_load;
  using namespace game_load::win32;

  const auto parsed = parse_command_line(argc, argv);
  if (!parsed.error.empty()) {
    std::cerr << "Error: " << parsed.error << "\n\n";
    print_help();
    return 64;
  }
  if (parsed.show_help) {
    print_help();
    return 0;
  }

  if (!parsed.config.execute) {
    print_dry_run_summary(parsed.config);
    if (!parsed.config.output_path.empty()) {
      auto result = dry_run_result(parsed.config);
      std::string error;
      if (!write_result_manifest(result, parsed.config.output_path, &error)) {
        std::cerr << "Could not write dry-run manifest: " << error << '\n';
        return 74;
      }
      std::cout << "Dry-run manifest: " << parsed.config.output_path << '\n';
    }
    return 0;
  }

  std::cout << "Starting an explicitly opted-in " << to_string(parsed.config.profile)
            << " workload. Press Escape or close the window to stop.\n"
            << "Power settings are metadata only and will not be changed.\n";
  D3d12Harness harness(parsed.config);
  auto result = harness.run();
  const std::string output = parsed.config.output_path.empty()
                                 ? "game-load-result.json"
                                 : parsed.config.output_path;
  std::string manifest_error;
  if (!write_result_manifest(result, output, &manifest_error)) {
    std::cerr << "Could not write result manifest: " << manifest_error << '\n';
    return 74;
  }
  std::cout << "Stopped: " << to_string(result.stop_reason) << " - "
            << result.stop_detail << '\n'
            << "Frames presented/measured: " << result.frames_presented << '/'
            << result.frames_measured << '\n'
            << "Result manifest: " << output << '\n';

  if (result.stop_reason == SafetyAbort::duration_complete) return 0;
  if (result.stop_reason == SafetyAbort::user_requested ||
      result.stop_reason == SafetyAbort::window_closed) return 130;
  return 2;
}
