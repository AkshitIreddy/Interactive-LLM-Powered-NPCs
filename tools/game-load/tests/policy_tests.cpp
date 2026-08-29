#include "game_load/policy.hpp"

#include <cmath>
#include <cstdlib>
#include <iostream>
#include <string>

namespace {

int failures = 0;

void check(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "FAIL: " << message << '\n';
    ++failures;
  }
}

void check_near(double actual, double expected, double epsilon,
                const std::string& message) {
  check(std::abs(actual - expected) <= epsilon, message);
}

void profile_tests() {
  using namespace game_load;
  const auto idle = profile_defaults(LoadProfile::idle);
  const auto typical = profile_defaults(LoadProfile::typical);
  const auto heavy = profile_defaults(LoadProfile::heavy);
  const auto constrained = profile_defaults(LoadProfile::constrained);

  check(!idle.execute, "every profile defaults to dry-run");
  check(!typical.execute && !heavy.execute && !constrained.execute,
        "no profile opts into live pressure");
  check(heavy.gpu_duty_percent > typical.gpu_duty_percent,
        "heavy GPU duty exceeds typical");
  check(constrained.min_vram_headroom_mib >= typical.min_vram_headroom_mib,
        "constrained profile preserves at least as much VRAM headroom");
  check(heavy.thermal_policy == ThermalPolicy::require_sensor,
        "heavy runs require a thermal sensor");
  check(parse_profile("typical") == LoadProfile::typical,
        "profile parser recognizes typical");
  check(!parse_profile("turbo").has_value(), "profile parser rejects unknown names");
  check_near(target_compute_ms(typical), 1000.0 / 60.0 * 0.35, 0.001,
             "target compute time derives from frame budget and duty");
}

void allocation_tests() {
  using namespace game_load;
  auto config = profile_defaults(LoadProfile::typical);
  VideoMemorySnapshot video{true, mib(10'000), mib(4'000), 0};
  auto vram = decide_vram_allocation(config, video);
  check(vram.approved_bytes == mib(1024), "healthy VRAM budget approves request");

  video.current_usage_bytes = mib(9'500);
  vram = decide_vram_allocation(config, video);
  check(vram.approved_bytes == 0, "VRAM reserve denies unsafe allocation");

  video.available = false;
  vram = decide_vram_allocation(config, video);
  check(vram.approved_bytes == 0, "missing DXGI budget fails closed");

  SystemMemorySnapshot memory{true, mib(32'000), mib(20'000), mib(8'000)};
  auto ram = decide_ram_allocation(config, memory);
  check(ram.approved_bytes == mib(512), "healthy commit budget approves RAM request");

  memory.available_physical_bytes = mib(1'500);
  ram = decide_ram_allocation(config, memory);
  check(ram.approved_bytes == 0, "physical memory reserve denies RAM allocation");
}

void safety_tests() {
  using namespace game_load;
  auto config = profile_defaults(LoadProfile::typical);
  SafetySnapshot snapshot;
  snapshot.memory = {true, mib(32'000), mib(20'000), mib(8'000)};
  snapshot.video_memory = {true, mib(10'000), mib(4'000), 0};
  snapshot.temperature_c = 70.0;
  check(evaluate_safety(config, snapshot).abort == SafetyAbort::none,
        "healthy snapshot continues");

  snapshot.temperature_c = 91.0;
  check(evaluate_safety(config, snapshot).abort == SafetyAbort::thermal_limit,
        "thermal limit aborts");
  snapshot.temperature_c = 70.0;
  snapshot.device_removed = true;
  check(evaluate_safety(config, snapshot).abort == SafetyAbort::device_removed,
        "device removal aborts");
  snapshot.device_removed = false;
  snapshot.memory.commit_total_bytes = mib(31'000);
  check(evaluate_safety(config, snapshot).abort == SafetyAbort::commit_pressure,
        "low commit headroom aborts");

  config = profile_defaults(LoadProfile::heavy);
  snapshot = {};
  check(evaluate_safety(config, snapshot).abort == SafetyAbort::thermal_sensor_required,
        "required missing thermal sensor aborts");
}

void calibration_tests() {
  using namespace game_load;
  ComputeCalibrator calibrator(5.0, 1, 1000);
  const auto up = calibrator.next({1.0, 100});
  check(up == 125, "calibrator bounds upward change to 25 percent");
  const auto down = calibrator.next({20.0, up});
  check(down >= 93 && down <= 157,
        "filtered calibrator remains bounded after a slow sample");

  ComputeCalibrator zero(0.0, 1, 1000);
  check(zero.next({10.0, 100}) == 1, "zero target chooses minimum work");

  ComputeCalibrator tiny_start(0.5, 1, 1000);
  check(tiny_start.next({0.003, 1}) == 4,
        "calibrator uses a bounded cold-start acquisition step");
}

void series_tests() {
  using namespace game_load;
  SampleSeries series(5);
  for (double value : {1.0, 2.0, 3.0, 4.0, 5.0, 99.0}) series.add(value);
  check(series.size() == 5, "series enforces capacity");
  check_near(series.minimum(), 1.0, 0.001, "series minimum");
  check_near(series.maximum(), 5.0, 0.001, "series maximum");
  check_near(series.mean(), 3.0, 0.001, "series mean");
  check_near(series.percentile(0.95), 4.8, 0.001, "series interpolated p95");
}

void validation_tests() {
  using namespace game_load;
  auto config = profile_defaults(LoadProfile::typical);
  check(validate_config(config).empty(), "built-in typical profile is valid");
  config.gpu_duty_percent = 100.0;
  config.duration_seconds = 0;
  check(validate_config(config).size() == 2, "invalid unsafe values are rejected");
  config = profile_defaults(LoadProfile::heavy);
  check(resolve_cpu_threads(config, 32) == 30,
        "automatic CPU workers leave two logical processors free");
  config.cpu_threads = 64;
  check(resolve_cpu_threads(config, 16) == 16,
        "explicit worker count clamps to available hardware");
}

}  // namespace

int main() {
  profile_tests();
  allocation_tests();
  safety_tests();
  calibration_tests();
  series_tests();
  validation_tests();
  if (failures != 0) {
    std::cerr << failures << " test(s) failed\n";
    return EXIT_FAILURE;
  }
  std::cout << "All game-load policy tests passed\n";
  return EXIT_SUCCESS;
}
