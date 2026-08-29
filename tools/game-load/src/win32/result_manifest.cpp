#include "result_manifest.hpp"

#include <Windows.h>

#include <filesystem>
#include <fstream>
#include <iomanip>
#include <sstream>
#include <stdexcept>
#include <string_view>

namespace game_load::win32 {
namespace {

std::string json_string(std::string_view value) {
  std::ostringstream output;
  output << '"';
  for (const unsigned char character : value) {
    switch (character) {
      case '"': output << "\\\""; break;
      case '\\': output << "\\\\"; break;
      case '\b': output << "\\b"; break;
      case '\f': output << "\\f"; break;
      case '\n': output << "\\n"; break;
      case '\r': output << "\\r"; break;
      case '\t': output << "\\t"; break;
      default:
        if (character < 0x20) {
          output << "\\u" << std::hex << std::setw(4) << std::setfill('0')
                 << static_cast<int>(character) << std::dec;
        } else {
          output << character;
        }
    }
  }
  output << '"';
  return output.str();
}

const char* boolean(bool value) { return value ? "true" : "false"; }

std::filesystem::path utf8_path(std::string_view value) {
  if (value.empty()) return {};
  const int count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                        static_cast<int>(value.size()), nullptr, 0);
  if (count <= 0) throw std::runtime_error("manifest path is not valid UTF-8");
  std::wstring wide(static_cast<std::size_t>(count), L'\0');
  MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                      static_cast<int>(value.size()), wide.data(), count);
  return std::filesystem::path(wide);
}

void summary(std::ostringstream& out, std::string_view indent,
             const SampleSummary& value) {
  out << "{\n" << indent << "  \"count\": " << value.count << ",\n"
      << indent << "  \"min\": " << value.minimum << ",\n"
      << indent << "  \"mean\": " << value.mean << ",\n"
      << indent << "  \"p50\": " << value.p50 << ",\n"
      << indent << "  \"p95\": " << value.p95 << ",\n"
      << indent << "  \"p99\": " << value.p99 << ",\n"
      << indent << "  \"max\": " << value.maximum << '\n'
      << indent << '}';
}

void string_array(std::ostringstream& out, const std::vector<std::string>& values,
                  std::string_view indent) {
  out << '[';
  if (!values.empty()) out << '\n';
  for (std::size_t index = 0; index < values.size(); ++index) {
    out << indent << "  " << json_string(values[index]);
    if (index + 1 != values.size()) out << ',';
    out << '\n';
  }
  if (!values.empty()) out << indent;
  out << ']';
}

void allocation(std::ostringstream& out, const AllocationDecision& value,
                std::string_view indent) {
  out << "{\n" << indent << "  \"requested_bytes\": " << value.requested_bytes
      << ",\n" << indent << "  \"approved_bytes\": " << value.approved_bytes
      << ",\n" << indent << "  \"budget_headroom_bytes\": "
      << value.budget_headroom_bytes << ",\n" << indent << "  \"reasons\": ";
  string_array(out, value.reasons, std::string(indent) + "  ");
  out << '\n' << indent << '}';
}

}  // namespace

std::string result_manifest_json(const RunResult& result) {
  const auto& config = result.config;
  std::ostringstream out;
  out << std::fixed << std::setprecision(4);
  out << "{\n"
      << "  \"schema\": \"response-console.game-load-result.v1\",\n"
      << "  \"harness_version\": " << json_string(result.harness_version) << ",\n"
      << "  \"started_utc\": " << json_string(result.started_utc) << ",\n"
      << "  \"ended_utc\": " << json_string(result.ended_utc) << ",\n"
      << "  \"dry_run\": " << boolean(result.dry_run) << ",\n"
      << "  \"completed\": " << boolean(result.completed) << ",\n"
      << "  \"stop_reason\": " << json_string(to_string(result.stop_reason)) << ",\n"
      << "  \"stop_detail\": " << json_string(result.stop_detail) << ",\n"
      << "  \"power\": {\n"
      << "    \"profile_metadata\": " << json_string(config.power_profile_metadata) << ",\n"
      << "    \"metadata_source\": \"user-supplied\",\n"
      << "    \"settings_changed_by_harness\": " << boolean(result.power_settings_changed) << ",\n"
      << "    \"note\": \"The harness never reads or changes G-Helper or Windows power settings.\"\n"
      << "  },\n"
      << "  \"configuration\": {\n"
      << "    \"profile\": " << json_string(to_string(config.profile)) << ",\n"
      << "    \"run_label\": " << json_string(config.run_label) << ",\n"
      << "    \"duration_seconds\": " << config.duration_seconds << ",\n"
      << "    \"width\": " << config.width << ",\n"
      << "    \"height\": " << config.height << ",\n"
      << "    \"target_fps\": " << config.target_fps << ",\n"
      << "    \"vsync\": " << boolean(config.vsync) << ",\n"
      << "    \"gpu_duty_percent\": " << config.gpu_duty_percent << ",\n"
      << "    \"gpu_compute_target_ms\": " << target_compute_ms(config) << ",\n"
      << "    \"requested_vram_mib\": " << config.requested_vram_mib << ",\n"
      << "    \"absolute_vram_cap_mib\": " << config.absolute_vram_cap_mib << ",\n"
      << "    \"max_budget_fraction\": " << config.max_budget_fraction << ",\n"
      << "    \"min_vram_headroom_mib\": " << config.min_vram_headroom_mib << ",\n"
      << "    \"cpu_duty_percent\": " << config.cpu_duty_percent << ",\n"
      << "    \"cpu_threads_requested\": " << config.cpu_threads << ",\n"
      << "    \"requested_ram_mib\": " << config.requested_ram_mib << ",\n"
      << "    \"min_commit_headroom_mib\": " << config.min_commit_headroom_mib << ",\n"
      << "    \"min_available_ram_mib\": " << config.min_available_ram_mib << ",\n"
      << "    \"thermal_limit_c\": " << config.thermal_limit_c << ",\n"
      << "    \"thermal_policy\": " << json_string(to_string(config.thermal_policy)) << "\n"
      << "  },\n"
      << "  \"system\": {\n"
      << "    \"operating_system\": " << json_string(result.operating_system) << ",\n"
      << "    \"cpu_name\": " << json_string(result.cpu_name) << ",\n"
      << "    \"logical_cpu_count\": " << result.logical_cpu_count << ",\n"
      << "    \"adapter\": {\n"
      << "      \"name\": " << json_string(result.adapter.name) << ",\n"
      << "      \"vendor_id\": " << result.adapter.vendor_id << ",\n"
      << "      \"device_id\": " << result.adapter.device_id << ",\n"
      << "      \"dedicated_video_memory_bytes\": " << result.adapter.dedicated_video_memory_bytes << ",\n"
      << "      \"shared_system_memory_bytes\": " << result.adapter.shared_system_memory_bytes << ",\n"
      << "      \"timestamp_frequency_hz\": " << result.adapter.timestamp_frequency_hz << ",\n"
      << "      \"driver_version\": " << json_string(result.adapter.driver_version) << "\n"
      << "    },\n"
      << "    \"initial_memory\": {\n"
      << "      \"commit_limit_bytes\": " << result.initial_system_memory.commit_limit_bytes << ",\n"
      << "      \"commit_total_bytes\": " << result.initial_system_memory.commit_total_bytes << ",\n"
      << "      \"available_physical_bytes\": " << result.initial_system_memory.available_physical_bytes << ",\n"
      << "      \"vram_budget_bytes\": " << result.initial_video_memory.budget_bytes << ",\n"
      << "      \"vram_usage_bytes\": " << result.initial_video_memory.current_usage_bytes << "\n"
      << "    }\n"
      << "  },\n"
      << "  \"allocations\": {\n"
      << "    \"vram\": ";
  allocation(out, result.vram_allocation, "    ");
  out << ",\n    \"ram\": ";
  allocation(out, result.ram_allocation, "    ");
  out << ",\n"
      << "    \"committed_vram_bytes\": " << result.committed_vram_bytes << ",\n"
      << "    \"committed_ram_bytes\": " << result.committed_ram_bytes << "\n"
      << "  },\n"
      << "  \"measurements\": {\n"
      << "    \"frames_presented\": " << result.frames_presented << ",\n"
      << "    \"frames_measured\": " << result.frames_measured << ",\n"
      << "    \"final_compute_dispatches\": " << result.final_compute_dispatches << ",\n"
      << "    \"qpc_frame_ms\": ";
  summary(out, "    ", result.qpc_frame_ms);
  out << ",\n    \"gpu_frame_ms\": ";
  summary(out, "    ", result.gpu_frame_ms);
  out << ",\n    \"gpu_compute_ms\": ";
  summary(out, "    ", result.gpu_compute_ms);
  out << ",\n    \"temperature_c\": ";
  summary(out, "    ", result.temperature_c);
  out << ",\n    \"vram_usage_mib\": ";
  summary(out, "    ", result.vram_usage_mib);
  out << ",\n    \"available_ram_mib\": ";
  summary(out, "    ", result.available_ram_mib);
  out << "\n  },\n"
      << "  \"safety_events\": ";
  string_array(out, result.safety_events, "  ");
  out << ",\n  \"warnings\": ";
  string_array(out, result.warnings, "  ");
  out << "\n}\n";
  return out.str();
}

bool write_result_manifest(const RunResult& result, const std::string& path,
                           std::string* error) {
  try {
    const auto target = utf8_path(path);
    if (target.has_parent_path()) std::filesystem::create_directories(target.parent_path());
    const auto temporary = target.wstring() + L".tmp";
    {
      std::ofstream stream(std::filesystem::path(temporary),
                           std::ios::binary | std::ios::trunc);
      if (!stream) {
        if (error) *error = "could not open temporary manifest file";
        return false;
      }
      stream << result_manifest_json(result);
      stream.flush();
      if (!stream) {
        if (error) *error = "could not flush temporary manifest file";
        return false;
      }
    }
    if (!MoveFileExW(temporary.c_str(), target.c_str(),
                     MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
      DeleteFileW(temporary.c_str());
      if (error) *error = "atomic manifest replacement failed with Win32 error " +
                          std::to_string(GetLastError());
      return false;
    }
    return true;
  } catch (const std::exception& exception) {
    if (error) *error = exception.what();
    return false;
  }
}

}  // namespace game_load::win32
