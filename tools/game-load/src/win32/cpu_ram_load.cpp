#include "cpu_ram_load.hpp"

#include <algorithm>
#include <chrono>
#include <cstddef>
#include <string>

namespace game_load::win32 {

CpuRamLoad::~CpuRamLoad() { stop(); }

bool CpuRamLoad::start(double cpu_duty_percent, std::uint32_t worker_count,
                       std::uint64_t approved_ram_bytes) {
  stop();

  if (approved_ram_bytes > 0) {
    ram_ = VirtualAlloc(nullptr, static_cast<SIZE_T>(approved_ram_bytes),
                        MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
    if (ram_ == nullptr) return false;

    // Touch every page so the manifest reports actual committed working data rather
    // than an untouched reservation that the OS can satisfy lazily.
    SYSTEM_INFO info{};
    GetSystemInfo(&info);
    const std::size_t page = std::max<std::size_t>(4096, info.dwPageSize);
    auto* bytes = static_cast<volatile std::byte*>(ram_);
    for (std::uint64_t offset = 0; offset < approved_ram_bytes; offset += page) {
      bytes[offset] = static_cast<std::byte>((offset / page) & 0xffU);
    }
    committed_ram_bytes_ = approved_ram_bytes;
  }

  if (cpu_duty_percent <= 0.0 || worker_count == 0) return true;
  workers_.reserve(worker_count);
  for (std::uint32_t index = 0; index < worker_count; ++index) {
    workers_.emplace_back(&CpuRamLoad::worker, cpu_duty_percent,
                          0x9e3779b9U ^ (index * 0x85ebca6bU));
  }
  return true;
}

void CpuRamLoad::stop() noexcept {
  for (auto& thread : workers_) thread.request_stop();
  workers_.clear();  // jthread joins during destruction.
  if (ram_ != nullptr) {
    VirtualFree(ram_, 0, MEM_RELEASE);
    ram_ = nullptr;
  }
  committed_ram_bytes_ = 0;
}

void CpuRamLoad::worker(std::stop_token stop, double duty_percent,
                        std::uint32_t seed) noexcept {
  SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
  SetThreadDescription(GetCurrentThread(), L"GameLoadHarness.CpuPressure");

  using clock = std::chrono::steady_clock;
  constexpr auto period = std::chrono::milliseconds(100);
  const auto busy = std::chrono::duration_cast<clock::duration>(
      period * (std::clamp(duty_percent, 0.0, 90.0) / 100.0));
  std::uint64_t state = (static_cast<std::uint64_t>(seed) << 32U) | 1U;

  while (!stop.stop_requested()) {
    const auto cycle_start = clock::now();
    const auto busy_until = cycle_start + busy;
    std::uint32_t stop_probe = 0;
    while (clock::now() < busy_until && !stop.stop_requested()) {
      // Deterministic integer mixing avoids compiler-elided empty spinning and keeps
      // this load independent from RAM pressure. Probe cancellation frequently.
      state ^= state >> 12U;
      state ^= state << 25U;
      state ^= state >> 27U;
      state *= 0x2545f4914f6cdd1dULL;
      if ((++stop_probe & 0x3fffU) == 0 && stop.stop_requested()) break;
    }
    const auto wake_at = cycle_start + period;
    while (!stop.stop_requested()) {
      const auto remaining = wake_at - clock::now();
      if (remaining <= clock::duration::zero()) break;
      const auto chunk = std::min(remaining,
          std::chrono::duration_cast<clock::duration>(std::chrono::milliseconds(10)));
      std::this_thread::sleep_for(chunk);
    }
  }

  // Keep the computation externally observable without creating shared contention.
  if (state == 0xdeadbeefULL) OutputDebugStringW(L"unreachable pressure state\n");
}

}  // namespace game_load::win32
