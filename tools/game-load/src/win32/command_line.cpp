#include "command_line.hpp"

#include <Windows.h>

#include <algorithm>
#include <charconv>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <limits>
#include <optional>
#include <sstream>
#include <string_view>

namespace game_load::win32 {
namespace {

std::string narrow(std::wstring_view value) {
  if (value.empty()) return {};
  const int count = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                                        static_cast<int>(value.size()), nullptr, 0,
                                        nullptr, nullptr);
  if (count <= 0) return {};
  std::string result(static_cast<std::size_t>(count), '\0');
  WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                      static_cast<int>(value.size()), result.data(), count, nullptr,
                      nullptr);
  return result;
}

template <typename T>
std::optional<T> integer_value(std::wstring_view value) {
  const auto bytes = narrow(value);
  T parsed{};
  const auto result = std::from_chars(bytes.data(), bytes.data() + bytes.size(), parsed);
  if (result.ec != std::errc{} || result.ptr != bytes.data() + bytes.size()) return std::nullopt;
  return parsed;
}

std::optional<double> double_value(std::wstring_view value) {
  const auto bytes = narrow(value);
  char* end = nullptr;
  const double parsed = std::strtod(bytes.c_str(), &end);
  if (end == bytes.c_str() || *end != '\0') return std::nullopt;
  return parsed;
}

bool requires_value(std::wstring_view option) {
  return option == L"--profile" || option == L"--duration-seconds" ||
         option == L"--width" || option == L"--height" || option == L"--fps" ||
         option == L"--gpu-duty" || option == L"--vram-mib" ||
         option == L"--vram-cap-mib" || option == L"--vram-budget-fraction" ||
         option == L"--cpu-duty" || option == L"--cpu-threads" ||
         option == L"--ram-mib" || option == L"--temperature-limit-c" ||
         option == L"--thermal-policy" || option == L"--power-profile-metadata" ||
         option == L"--label" || option == L"--output" ||
         option == L"--min-vram-headroom-mib" ||
         option == L"--min-commit-headroom-mib" ||
         option == L"--min-available-ram-mib";
}

template <typename T>
bool set_integer(T& target, std::wstring_view value, std::string& error,
                 std::wstring_view option) {
  auto parsed = integer_value<T>(value);
  if (!parsed) {
    error = narrow(option) + " requires a base-10 integer";
    return false;
  }
  target = *parsed;
  return true;
}

bool set_double(double& target, std::wstring_view value, std::string& error,
                std::wstring_view option) {
  auto parsed = double_value(value);
  if (!parsed) {
    error = narrow(option) + " requires a number";
    return false;
  }
  target = *parsed;
  return true;
}

}  // namespace

ParsedCommandLine parse_command_line(int argc, wchar_t** argv) {
  ParsedCommandLine parsed;
  LoadProfile selected = LoadProfile::typical;

  // Resolve the profile before individual overrides so argument order never changes
  // meaning. This also preserves the invariant that every profile starts dry-run.
  for (int index = 1; index < argc; ++index) {
    if (std::wstring_view(argv[index]) == L"--profile") {
      if (index + 1 >= argc) {
        parsed.error = "--profile requires a value";
        return parsed;
      }
      auto candidate = parse_profile(narrow(argv[++index]));
      if (!candidate) {
        parsed.error = "unknown profile; expected idle, typical, heavy, or constrained";
        return parsed;
      }
      selected = *candidate;
    }
  }
  parsed.config = profile_defaults(selected);

  for (int index = 1; index < argc; ++index) {
    const std::wstring_view option(argv[index]);
    if (option == L"--help" || option == L"-h" || option == L"/?") {
      parsed.show_help = true;
      continue;
    }
    if (option == L"--profile") {
      ++index;
      continue;
    }
    if (option == L"--run") {
      parsed.config.execute = true;
      continue;
    }
    if (option == L"--dry-run") {
      parsed.config.execute = false;
      continue;
    }
    if (option == L"--vsync") {
      parsed.config.vsync = true;
      continue;
    }
    if (option == L"--no-vsync") {
      parsed.config.vsync = false;
      continue;
    }
    if (option == L"--smoke") {
      parsed.smoke = true;
      parsed.config.duration_seconds = 3;
      parsed.config.width = 640;
      parsed.config.height = 360;
      parsed.config.gpu_duty_percent = std::min(parsed.config.gpu_duty_percent, 5.0);
      parsed.config.cpu_duty_percent = 0.0;
      parsed.config.requested_ram_mib = 0;
      parsed.config.requested_vram_mib = 0;
      continue;
    }
    if (!requires_value(option)) {
      parsed.error = "unknown option: " + narrow(option);
      return parsed;
    }
    if (index + 1 >= argc) {
      parsed.error = narrow(option) + " requires a value";
      return parsed;
    }
    const std::wstring_view value(argv[++index]);
    if (option == L"--duration-seconds" &&
        !set_integer(parsed.config.duration_seconds, value, parsed.error, option)) return parsed;
    if (option == L"--width" &&
        !set_integer(parsed.config.width, value, parsed.error, option)) return parsed;
    if (option == L"--height" &&
        !set_integer(parsed.config.height, value, parsed.error, option)) return parsed;
    if (option == L"--fps" &&
        !set_integer(parsed.config.target_fps, value, parsed.error, option)) return parsed;
    if (option == L"--gpu-duty" &&
        !set_double(parsed.config.gpu_duty_percent, value, parsed.error, option)) return parsed;
    if (option == L"--vram-mib" &&
        !set_integer(parsed.config.requested_vram_mib, value, parsed.error, option)) return parsed;
    if (option == L"--vram-cap-mib" &&
        !set_integer(parsed.config.absolute_vram_cap_mib, value, parsed.error, option)) return parsed;
    if (option == L"--vram-budget-fraction" &&
        !set_double(parsed.config.max_budget_fraction, value, parsed.error, option)) return parsed;
    if (option == L"--cpu-duty" &&
        !set_double(parsed.config.cpu_duty_percent, value, parsed.error, option)) return parsed;
    if (option == L"--cpu-threads" &&
        !set_integer(parsed.config.cpu_threads, value, parsed.error, option)) return parsed;
    if (option == L"--ram-mib" &&
        !set_integer(parsed.config.requested_ram_mib, value, parsed.error, option)) return parsed;
    if (option == L"--temperature-limit-c" &&
        !set_double(parsed.config.thermal_limit_c, value, parsed.error, option)) return parsed;
    if (option == L"--min-vram-headroom-mib" &&
        !set_integer(parsed.config.min_vram_headroom_mib, value, parsed.error, option)) return parsed;
    if (option == L"--min-commit-headroom-mib" &&
        !set_integer(parsed.config.min_commit_headroom_mib, value, parsed.error, option)) return parsed;
    if (option == L"--min-available-ram-mib" &&
        !set_integer(parsed.config.min_available_ram_mib, value, parsed.error, option)) return parsed;
    if (option == L"--power-profile-metadata") parsed.config.power_profile_metadata = narrow(value);
    if (option == L"--label") parsed.config.run_label = narrow(value);
    if (option == L"--output") parsed.config.output_path = narrow(value);
    if (option == L"--thermal-policy") {
      const auto policy = narrow(value);
      if (policy == "best-effort") parsed.config.thermal_policy = ThermalPolicy::best_effort;
      else if (policy == "require-sensor") parsed.config.thermal_policy = ThermalPolicy::require_sensor;
      else {
        parsed.error = "--thermal-policy expects best-effort or require-sensor";
        return parsed;
      }
    }
    if (!parsed.error.empty()) return parsed;
  }

  const auto errors = validate_config(parsed.config);
  if (!errors.empty()) parsed.error = errors.front();
  return parsed;
}

void print_help() {
  std::cout << R"HELP(Response Console synthetic game-load harness

USAGE
  game-load [--profile NAME] [--run] [options]

SAFETY MODEL
  The default is a dry-run. Real load begins only with --run. This program never
  reads, changes, or restores G-Helper settings or a Windows power plan. Record
  the current setting as inert metadata with --power-profile-metadata.

PROFILES
  idle          Visible scene with nearly no added pressure
  typical       1080p, moderate GPU/CPU/RAM/VRAM pressure
  heavy         1440p, higher pressure; requires a readable thermal sensor
  constrained   Lower load with larger safety reserves

CORE OPTIONS
  --run | --dry-run
  --profile idle|typical|heavy|constrained
  --duration-seconds N        1..3600
  --width N --height N        640x360..7680x4320
  --fps N                     15..240
  --vsync | --no-vsync
  --gpu-duty PERCENT          0..95 percent of a frame budget
  --vram-mib N                requested synthetic committed VRAM
  --vram-cap-mib N            absolute cap, still bounded by DXGI budget
  --vram-budget-fraction F    0..0.75 of current DXGI budget
  --cpu-duty PERCENT          0..90 per worker
  --cpu-threads N             0 chooses a safe automatic count
  --ram-mib N                 requested touched RAM commit
  --temperature-limit-c C     60..100
  --thermal-policy best-effort|require-sensor
  --power-profile-metadata S  user-supplied label only; never acted upon
  --label S                   run label for result correlation
  --output PATH               JSON manifest path
  --smoke                     three-second minimal configuration

Press Escape or close the window to stop. Live runs also stop on thermal,
system-commit, physical-memory, DXGI-budget, GPU-timeout, or device-reset limits.
)HELP";
}

void print_dry_run_summary(const WorkloadConfig& config) {
  std::cout << "DRY RUN - no workload was started\n"
            << "  profile: " << to_string(config.profile) << '\n'
            << "  duration: " << config.duration_seconds << " s\n"
            << "  surface: " << config.width << 'x' << config.height << " @ "
            << config.target_fps << " FPS\n"
            << "  GPU duty target: " << std::fixed << std::setprecision(1)
            << config.gpu_duty_percent << "% (" << target_compute_ms(config)
            << " ms/frame)\n"
            << "  CPU duty: " << config.cpu_duty_percent << "%\n"
            << "  requested VRAM/RAM: " << config.requested_vram_mib << "/"
            << config.requested_ram_mib << " MiB\n"
            << "  thermal policy: " << to_string(config.thermal_policy) << " at "
            << config.thermal_limit_c << " C\n"
            << "  power profile metadata: " << config.power_profile_metadata << '\n'
            << "Power settings changed: no (this program has no such code path)\n"
            << "Add --run only when you are ready for an opt-in live load.\n";
}

}  // namespace game_load::win32
