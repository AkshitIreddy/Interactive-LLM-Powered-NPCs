#pragma once

#include "game_load/policy.hpp"

#include <chrono>
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace game_load::win32 {

struct AdapterInfo {
  std::string name;
  std::uint32_t vendor_id = 0;
  std::uint32_t device_id = 0;
  std::uint64_t dedicated_video_memory_bytes = 0;
  std::uint64_t dedicated_system_memory_bytes = 0;
  std::uint64_t shared_system_memory_bytes = 0;
  std::uint64_t timestamp_frequency_hz = 0;
  std::string driver_version;
};

struct SampleSummary {
  std::size_t count = 0;
  double minimum = 0.0;
  double mean = 0.0;
  double p50 = 0.0;
  double p95 = 0.0;
  double p99 = 0.0;
  double maximum = 0.0;
};

struct RunResult {
  WorkloadConfig config;
  bool dry_run = true;
  bool completed = false;
  bool power_settings_changed = false;
  SafetyAbort stop_reason = SafetyAbort::none;
  std::string stop_detail;
  std::string started_utc;
  std::string ended_utc;
  std::string harness_version;
  std::string operating_system;
  std::string cpu_name;
  std::uint32_t logical_cpu_count = 0;
  AdapterInfo adapter;
  VideoMemorySnapshot initial_video_memory;
  SystemMemorySnapshot initial_system_memory;
  AllocationDecision vram_allocation;
  AllocationDecision ram_allocation;
  std::uint64_t committed_vram_bytes = 0;
  std::uint64_t committed_ram_bytes = 0;
  std::uint64_t frames_presented = 0;
  std::uint64_t frames_measured = 0;
  std::uint32_t final_compute_dispatches = 0;
  SampleSummary qpc_frame_ms;
  SampleSummary gpu_frame_ms;
  SampleSummary gpu_compute_ms;
  SampleSummary temperature_c;
  SampleSummary vram_usage_mib;
  SampleSummary available_ram_mib;
  std::vector<std::string> warnings;
  std::vector<std::string> safety_events;
};

inline SampleSummary summarize(const SampleSeries& series) {
  return {series.size(), series.minimum(), series.mean(), series.percentile(0.50),
          series.percentile(0.95), series.percentile(0.99), series.maximum()};
}

}  // namespace game_load::win32
