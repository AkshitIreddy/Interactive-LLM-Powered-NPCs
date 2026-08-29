#include "game_load/policy.hpp"

#include <algorithm>
#include <cmath>
#include <limits>
#include <numeric>

namespace game_load {
namespace {

std::uint64_t saturating_sub(std::uint64_t lhs, std::uint64_t rhs) {
  return lhs > rhs ? lhs - rhs : 0;
}

std::uint64_t fraction_of(std::uint64_t value, double fraction) {
  if (fraction <= 0.0) return 0;
  if (fraction >= 1.0) return value;
  return static_cast<std::uint64_t>(static_cast<long double>(value) * fraction);
}

}  // namespace

WorkloadConfig profile_defaults(LoadProfile profile) {
  WorkloadConfig config;
  config.profile = profile;
  switch (profile) {
    case LoadProfile::idle:
      config.duration_seconds = 15;
      config.width = 1280;
      config.height = 720;
      config.target_fps = 60;
      config.gpu_duty_percent = 3.0;
      config.requested_vram_mib = 0;
      config.absolute_vram_cap_mib = 256;
      config.max_budget_fraction = 0.10;
      config.min_vram_headroom_mib = 1024;
      config.cpu_duty_percent = 0.0;
      config.cpu_threads = 1;
      config.requested_ram_mib = 0;
      break;
    case LoadProfile::typical:
      config.duration_seconds = 30;
      config.width = 1920;
      config.height = 1080;
      config.target_fps = 60;
      config.gpu_duty_percent = 35.0;
      config.requested_vram_mib = 1024;
      config.absolute_vram_cap_mib = 3072;
      config.max_budget_fraction = 0.40;
      config.min_vram_headroom_mib = 1024;
      config.cpu_duty_percent = 25.0;
      config.cpu_threads = 0;
      config.requested_ram_mib = 512;
      break;
    case LoadProfile::heavy:
      config.duration_seconds = 45;
      config.width = 2560;
      config.height = 1440;
      config.target_fps = 60;
      config.gpu_duty_percent = 65.0;
      config.requested_vram_mib = 3072;
      config.absolute_vram_cap_mib = 6144;
      config.max_budget_fraction = 0.55;
      config.min_vram_headroom_mib = 1536;
      config.cpu_duty_percent = 60.0;
      config.cpu_threads = 0;
      config.requested_ram_mib = 1536;
      config.min_commit_headroom_mib = 3072;
      config.min_available_ram_mib = 2048;
      config.thermal_policy = ThermalPolicy::require_sensor;
      break;
    case LoadProfile::constrained:
      config.duration_seconds = 30;
      config.width = 1280;
      config.height = 720;
      config.target_fps = 60;
      config.gpu_duty_percent = 20.0;
      config.requested_vram_mib = 384;
      config.absolute_vram_cap_mib = 768;
      config.max_budget_fraction = 0.20;
      config.min_vram_headroom_mib = 1536;
      config.cpu_duty_percent = 15.0;
      config.cpu_threads = 2;
      config.requested_ram_mib = 256;
      config.min_commit_headroom_mib = 3072;
      config.min_available_ram_mib = 2048;
      config.thermal_limit_c = 85.0;
      break;
  }
  return config;
}

std::optional<LoadProfile> parse_profile(std::string_view value) {
  if (value == "idle") return LoadProfile::idle;
  if (value == "typical") return LoadProfile::typical;
  if (value == "heavy") return LoadProfile::heavy;
  if (value == "constrained") return LoadProfile::constrained;
  return std::nullopt;
}

std::string_view to_string(LoadProfile profile) noexcept {
  switch (profile) {
    case LoadProfile::idle: return "idle";
    case LoadProfile::typical: return "typical";
    case LoadProfile::heavy: return "heavy";
    case LoadProfile::constrained: return "constrained";
  }
  return "unknown";
}

std::string_view to_string(ThermalPolicy policy) noexcept {
  switch (policy) {
    case ThermalPolicy::best_effort: return "best-effort";
    case ThermalPolicy::require_sensor: return "require-sensor";
  }
  return "unknown";
}

std::string_view to_string(SafetyAbort abort) noexcept {
  switch (abort) {
    case SafetyAbort::none: return "none";
    case SafetyAbort::user_requested: return "user-requested";
    case SafetyAbort::duration_complete: return "duration-complete";
    case SafetyAbort::thermal_limit: return "thermal-limit";
    case SafetyAbort::thermal_sensor_required: return "thermal-sensor-required";
    case SafetyAbort::commit_pressure: return "commit-pressure";
    case SafetyAbort::physical_memory_pressure: return "physical-memory-pressure";
    case SafetyAbort::video_memory_pressure: return "video-memory-pressure";
    case SafetyAbort::device_removed: return "device-removed";
    case SafetyAbort::gpu_timeout: return "gpu-timeout";
    case SafetyAbort::window_closed: return "window-closed";
    case SafetyAbort::initialization_failure: return "initialization-failure";
    case SafetyAbort::internal_error: return "internal-error";
  }
  return "unknown";
}

double target_compute_ms(const WorkloadConfig& config) noexcept {
  if (config.target_fps == 0) return 0.0;
  const double frame_ms = 1000.0 / static_cast<double>(config.target_fps);
  return frame_ms * std::clamp(config.gpu_duty_percent, 0.0, 95.0) / 100.0;
}

AllocationDecision decide_vram_allocation(const WorkloadConfig& config,
                                          const VideoMemorySnapshot& snapshot) {
  AllocationDecision result;
  result.requested_bytes = mib(config.requested_vram_mib);
  if (result.requested_bytes == 0) {
    result.reasons.emplace_back("profile requests no synthetic VRAM allocation");
    return result;
  }
  if (!snapshot.available || snapshot.budget_bytes == 0) {
    result.reasons.emplace_back("DXGI video-memory budget is unavailable; allocation denied");
    return result;
  }

  result.budget_headroom_bytes = saturating_sub(snapshot.budget_bytes,
                                                snapshot.current_usage_bytes);
  const auto preserved_headroom = mib(config.min_vram_headroom_mib);
  const auto after_reserve = saturating_sub(result.budget_headroom_bytes,
                                            preserved_headroom);
  const auto fraction_cap = fraction_of(snapshot.budget_bytes,
                                        config.max_budget_fraction);
  const auto absolute_cap = mib(config.absolute_vram_cap_mib);
  result.approved_bytes = std::min({result.requested_bytes, after_reserve,
                                    fraction_cap, absolute_cap});

  if (result.approved_bytes < result.requested_bytes) {
    result.reasons.emplace_back("request reduced to preserve live DXGI budget headroom");
  }
  if (result.approved_bytes == 0) {
    result.reasons.emplace_back("insufficient safe video-memory headroom");
  }
  return result;
}

AllocationDecision decide_ram_allocation(const WorkloadConfig& config,
                                         const SystemMemorySnapshot& snapshot) {
  AllocationDecision result;
  result.requested_bytes = mib(config.requested_ram_mib);
  if (result.requested_bytes == 0) {
    result.reasons.emplace_back("profile requests no synthetic RAM commit");
    return result;
  }
  if (!snapshot.available || snapshot.commit_limit_bytes == 0) {
    result.reasons.emplace_back("system commit limits are unavailable; allocation denied");
    return result;
  }

  const auto commit_headroom = saturating_sub(snapshot.commit_limit_bytes,
                                              snapshot.commit_total_bytes);
  const auto commit_safe = saturating_sub(commit_headroom,
                                          mib(config.min_commit_headroom_mib));
  const auto physical_safe = saturating_sub(snapshot.available_physical_bytes,
                                            mib(config.min_available_ram_mib));
  result.budget_headroom_bytes = std::min(commit_headroom,
                                         snapshot.available_physical_bytes);
  result.approved_bytes = std::min({result.requested_bytes, commit_safe,
                                    physical_safe});
  if (result.approved_bytes < result.requested_bytes) {
    result.reasons.emplace_back("request reduced to preserve commit and physical-memory headroom");
  }
  if (result.approved_bytes == 0) {
    result.reasons.emplace_back("insufficient safe system-memory headroom");
  }
  return result;
}

SafetyDecision evaluate_safety(const WorkloadConfig& config,
                               const SafetySnapshot& snapshot) {
  if (snapshot.user_requested_stop) {
    return {SafetyAbort::user_requested, "Escape or a stop request was received"};
  }
  if (snapshot.window_closed) {
    return {SafetyAbort::window_closed, "The harness window was closed"};
  }
  if (snapshot.device_removed) {
    return {SafetyAbort::device_removed, "D3D12 reported a removed or reset device"};
  }
  if (snapshot.gpu_timed_out) {
    return {SafetyAbort::gpu_timeout, "A GPU fence exceeded the configured timeout"};
  }
  if (snapshot.temperature_c.has_value() &&
      *snapshot.temperature_c >= config.thermal_limit_c) {
    return {SafetyAbort::thermal_limit, "The highest reported thermal zone reached the limit"};
  }
  if (!snapshot.temperature_c.has_value() &&
      config.thermal_policy == ThermalPolicy::require_sensor) {
    return {SafetyAbort::thermal_sensor_required,
            "The selected profile requires a readable ACPI thermal sensor"};
  }

  if (snapshot.memory.available) {
    const auto commit_headroom = saturating_sub(snapshot.memory.commit_limit_bytes,
                                                snapshot.memory.commit_total_bytes);
    if (commit_headroom < mib(config.min_commit_headroom_mib)) {
      return {SafetyAbort::commit_pressure,
              "System commit headroom fell below the configured reserve"};
    }
    if (snapshot.memory.available_physical_bytes <
        mib(config.min_available_ram_mib)) {
      return {SafetyAbort::physical_memory_pressure,
              "Available physical memory fell below the configured reserve"};
    }
  }

  if (snapshot.video_memory.available) {
    const auto headroom = saturating_sub(snapshot.video_memory.budget_bytes,
                                         snapshot.video_memory.current_usage_bytes);
    if (headroom < mib(config.min_vram_headroom_mib)) {
      return {SafetyAbort::video_memory_pressure,
              "DXGI video-memory headroom fell below the configured reserve"};
    }
  }
  return {};
}

std::vector<std::string> validate_config(const WorkloadConfig& config) {
  std::vector<std::string> errors;
  if (config.duration_seconds == 0 || config.duration_seconds > 3600) {
    errors.emplace_back("duration must be between 1 and 3600 seconds");
  }
  if (config.width < 640 || config.width > 7680 || config.height < 360 ||
      config.height > 4320) {
    errors.emplace_back("resolution must be between 640x360 and 7680x4320");
  }
  if (config.target_fps < 15 || config.target_fps > 240) {
    errors.emplace_back("target FPS must be between 15 and 240");
  }
  if (config.gpu_duty_percent < 0.0 || config.gpu_duty_percent > 95.0) {
    errors.emplace_back("GPU duty must be between 0 and 95 percent");
  }
  if (config.cpu_duty_percent < 0.0 || config.cpu_duty_percent > 90.0) {
    errors.emplace_back("CPU duty must be between 0 and 90 percent");
  }
  if (config.absolute_vram_cap_mib > 16'384) {
    errors.emplace_back("absolute VRAM cap may not exceed 16384 MiB");
  }
  if (config.requested_ram_mib > 16'384) {
    errors.emplace_back("requested RAM commit may not exceed 16384 MiB");
  }
  if (config.max_budget_fraction < 0.0 || config.max_budget_fraction > 0.75) {
    errors.emplace_back("VRAM budget fraction must be between 0 and 0.75");
  }
  if (config.thermal_limit_c < 60.0 || config.thermal_limit_c > 100.0) {
    errors.emplace_back("thermal limit must be between 60 and 100 C");
  }
  if (config.safety_poll_ms < 100 || config.safety_poll_ms > 5000) {
    errors.emplace_back("safety poll interval must be between 100 and 5000 ms");
  }
  if (config.gpu_fence_timeout_ms < 250 || config.gpu_fence_timeout_ms > 10'000) {
    errors.emplace_back("GPU fence timeout must be between 250 and 10000 ms");
  }
  if (config.power_profile_metadata.size() > 256) {
    errors.emplace_back("power profile metadata may not exceed 256 characters");
  }
  return errors;
}

std::uint32_t resolve_cpu_threads(const WorkloadConfig& config,
                                  std::uint32_t hardware_threads) noexcept {
  const auto available = std::max(1U, hardware_threads);
  if (config.cpu_threads != 0) return std::clamp(config.cpu_threads, 1U, available);
  // Leave two logical processors for the game, runtime, compositor, and OS.
  return available > 4 ? available - 2 : std::max(1U, available / 2);
}

ComputeCalibrator::ComputeCalibrator(double target_ms,
                                     std::uint32_t minimum_dispatches,
                                     std::uint32_t maximum_dispatches)
    : target_ms_(std::max(0.0, target_ms)),
      minimum_(std::max(1U, minimum_dispatches)),
      maximum_(std::max(minimum_, maximum_dispatches)),
      current_(minimum_) {}

std::uint32_t ComputeCalibrator::next(const CalibrationSample& sample) {
  if (target_ms_ <= 0.0) {
    current_ = minimum_;
    return current_;
  }
  if (!std::isfinite(sample.measured_compute_ms) || sample.measured_compute_ms <= 0.0) {
    current_ = std::min(maximum_, std::max(minimum_, current_ * 2U));
    return current_;
  }
  filtered_ms_ = filtered_ms_ == 0.0
                     ? sample.measured_compute_ms
                     : filtered_ms_ * 0.75 + sample.measured_compute_ms * 0.25;
  const double raw_ratio = target_ms_ / filtered_ms_;
  // A cold one-group dispatch can be hundreds of times below target. Allow a
  // conservative 4x acquisition step only while measured work is below 10% of the
  // target; once signal is meaningful, use the tighter 25% controller.
  const double upper_ratio = filtered_ms_ < target_ms_ * 0.10 ? 4.0 : 1.25;
  const double bounded_ratio = std::clamp(raw_ratio, 0.75, upper_ratio);
  auto proposed = static_cast<std::uint64_t>(std::llround(
      static_cast<double>(std::max(1U, sample.dispatches)) * bounded_ratio));
  // Rounding 1 * 1.25 back to 1 otherwise traps the controller permanently at its
  // minimum. Guarantee monotonic progress when the error is clearly outside the
  // dead band, while retaining the multiplicative bound at useful work sizes.
  if (raw_ratio > 1.05 && proposed <= sample.dispatches &&
      sample.dispatches < maximum_) {
    proposed = static_cast<std::uint64_t>(sample.dispatches) + 1;
  } else if (raw_ratio < 0.95 && proposed >= sample.dispatches &&
             sample.dispatches > minimum_) {
    proposed = static_cast<std::uint64_t>(sample.dispatches) - 1;
  }
  current_ = static_cast<std::uint32_t>(
      std::clamp<std::uint64_t>(proposed, minimum_, maximum_));
  return current_;
}

SampleSeries::SampleSeries(std::size_t capacity)
    : capacity_(std::max<std::size_t>(1, capacity)) {
  values_.reserve(std::min<std::size_t>(capacity_, 4096));
}

void SampleSeries::add(double value) {
  if (!std::isfinite(value) || values_.size() >= capacity_) return;
  values_.push_back(value);
  sum_ += value;
}

double SampleSeries::minimum() const noexcept {
  return values_.empty() ? 0.0 : *std::min_element(values_.begin(), values_.end());
}

double SampleSeries::maximum() const noexcept {
  return values_.empty() ? 0.0 : *std::max_element(values_.begin(), values_.end());
}

double SampleSeries::mean() const noexcept {
  return values_.empty() ? 0.0 : sum_ / static_cast<double>(values_.size());
}

double SampleSeries::percentile(double probability) const {
  if (values_.empty()) return 0.0;
  auto sorted = values_;
  std::sort(sorted.begin(), sorted.end());
  const double p = std::clamp(probability, 0.0, 1.0);
  const double index = p * static_cast<double>(sorted.size() - 1);
  const auto lower = static_cast<std::size_t>(std::floor(index));
  const auto upper = static_cast<std::size_t>(std::ceil(index));
  const double fraction = index - static_cast<double>(lower);
  return sorted[lower] * (1.0 - fraction) + sorted[upper] * fraction;
}

}  // namespace game_load
