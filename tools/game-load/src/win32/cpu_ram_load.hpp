#pragma once

#include "game_load/policy.hpp"

#include <Windows.h>

#include <atomic>
#include <cstdint>
#include <stop_token>
#include <thread>
#include <vector>

namespace game_load::win32 {

class CpuRamLoad final {
 public:
  CpuRamLoad() = default;
  ~CpuRamLoad();
  CpuRamLoad(const CpuRamLoad&) = delete;
  CpuRamLoad& operator=(const CpuRamLoad&) = delete;

  bool start(double cpu_duty_percent, std::uint32_t worker_count,
             std::uint64_t approved_ram_bytes);
  void stop() noexcept;

  [[nodiscard]] std::uint64_t committed_ram_bytes() const noexcept {
    return committed_ram_bytes_;
  }
  [[nodiscard]] std::uint32_t worker_count() const noexcept {
    return static_cast<std::uint32_t>(workers_.size());
  }

 private:
  static void worker(std::stop_token stop, double duty_percent,
                     std::uint32_t seed) noexcept;

  void* ram_ = nullptr;
  std::uint64_t committed_ram_bytes_ = 0;
  std::vector<std::jthread> workers_;
};

}  // namespace game_load::win32
