#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace game_load {

enum class LoadProfile {
  idle,
  typical,
  heavy,
  constrained,
};

enum class ThermalPolicy {
  best_effort,
  require_sensor,
};

enum class SafetyAbort {
  none,
  user_requested,
  duration_complete,
  thermal_limit,
  thermal_sensor_required,
  commit_pressure,
  physical_memory_pressure,
  video_memory_pressure,
  device_removed,
  gpu_timeout,
  window_closed,
  initialization_failure,
  internal_error,
};

struct WorkloadConfig {
  LoadProfile profile = LoadProfile::typical;
  bool execute = false;
  std::uint32_t duration_seconds = 15;
  std::uint32_t width = 1920;
  std::uint32_t height = 1080;
  std::uint32_t target_fps = 60;
  bool vsync = true;

  double gpu_duty_percent = 35.0;
  std::uint64_t requested_vram_mib = 1024;
  std::uint64_t absolute_vram_cap_mib = 4096;
  double max_budget_fraction = 0.50;
  std::uint64_t min_vram_headroom_mib = 768;

  double cpu_duty_percent = 25.0;
  std::uint32_t cpu_threads = 0;  // 0 resolves from hardware concurrency.
  std::uint64_t requested_ram_mib = 512;
  std::uint64_t min_commit_headroom_mib = 2048;
  std::uint64_t min_available_ram_mib = 1536;

  double thermal_limit_c = 90.0;
  ThermalPolicy thermal_policy = ThermalPolicy::best_effort;
  std::uint32_t safety_poll_ms = 500;
  std::uint32_t gpu_fence_timeout_ms = 2000;

  std::string power_profile_metadata = "unspecified";
  std::string run_label;
  std::string output_path;
};

struct VideoMemorySnapshot {
  bool available = false;
  std::uint64_t budget_bytes = 0;
  std::uint64_t current_usage_bytes = 0;
  std::uint64_t current_reservation_bytes = 0;
};

struct SystemMemorySnapshot {
  bool available = false;
  std::uint64_t commit_limit_bytes = 0;
  std::uint64_t commit_total_bytes = 0;
  std::uint64_t available_physical_bytes = 0;
};

struct SafetySnapshot {
  std::optional<double> temperature_c;
  SystemMemorySnapshot memory;
  VideoMemorySnapshot video_memory;
  bool device_removed = false;
  bool gpu_timed_out = false;
  bool user_requested_stop = false;
  bool window_closed = false;
};

struct AllocationDecision {
  std::uint64_t requested_bytes = 0;
  std::uint64_t approved_bytes = 0;
  std::uint64_t budget_headroom_bytes = 0;
  std::vector<std::string> reasons;
};

struct SafetyDecision {
  SafetyAbort abort = SafetyAbort::none;
  std::string detail;
};

struct CalibrationSample {
  double measured_compute_ms = 0.0;
  std::uint32_t dispatches = 1;
};

class ComputeCalibrator {
 public:
  ComputeCalibrator(double target_ms, std::uint32_t minimum_dispatches = 1,
                    std::uint32_t maximum_dispatches = 1'000'000);

  [[nodiscard]] std::uint32_t next(const CalibrationSample& sample);
  [[nodiscard]] std::uint32_t current() const noexcept { return current_; }
  [[nodiscard]] double target_ms() const noexcept { return target_ms_; }

 private:
  double target_ms_;
  std::uint32_t minimum_;
  std::uint32_t maximum_;
  std::uint32_t current_;
  double filtered_ms_ = 0.0;
};

class SampleSeries {
 public:
  explicit SampleSeries(std::size_t capacity = 65'536);
  void add(double value);

  [[nodiscard]] std::size_t size() const noexcept { return values_.size(); }
  [[nodiscard]] double minimum() const noexcept;
  [[nodiscard]] double maximum() const noexcept;
  [[nodiscard]] double mean() const noexcept;
  [[nodiscard]] double percentile(double probability) const;
  [[nodiscard]] const std::vector<double>& values() const noexcept { return values_; }

 private:
  std::size_t capacity_;
  std::vector<double> values_;
  double sum_ = 0.0;
};

[[nodiscard]] WorkloadConfig profile_defaults(LoadProfile profile);
[[nodiscard]] std::optional<LoadProfile> parse_profile(std::string_view value);
[[nodiscard]] std::string_view to_string(LoadProfile profile) noexcept;
[[nodiscard]] std::string_view to_string(ThermalPolicy policy) noexcept;
[[nodiscard]] std::string_view to_string(SafetyAbort abort) noexcept;
[[nodiscard]] double target_compute_ms(const WorkloadConfig& config) noexcept;
[[nodiscard]] AllocationDecision decide_vram_allocation(
    const WorkloadConfig& config, const VideoMemorySnapshot& snapshot);
[[nodiscard]] AllocationDecision decide_ram_allocation(
    const WorkloadConfig& config, const SystemMemorySnapshot& snapshot);
[[nodiscard]] SafetyDecision evaluate_safety(const WorkloadConfig& config,
                                             const SafetySnapshot& snapshot);
[[nodiscard]] std::vector<std::string> validate_config(const WorkloadConfig& config);
[[nodiscard]] std::uint32_t resolve_cpu_threads(const WorkloadConfig& config,
                                                std::uint32_t hardware_threads) noexcept;

constexpr std::uint64_t mib(std::uint64_t value) noexcept {
  return value * 1024ULL * 1024ULL;
}

}  // namespace game_load
