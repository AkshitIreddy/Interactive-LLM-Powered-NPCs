#pragma once

#include "game_load/policy.hpp"

#include <Windows.h>
#include <dxgi1_6.h>
#include <wrl/client.h>

#include <optional>
#include <string>

namespace game_load::win32 {

SystemMemorySnapshot query_system_memory() noexcept;
VideoMemorySnapshot query_video_memory(IDXGIAdapter3* adapter) noexcept;
std::optional<double> query_acpi_temperature_c(std::string* warning = nullptr) noexcept;
std::string query_operating_system();
std::string query_cpu_name();
std::string wide_to_utf8(std::wstring_view value);
std::string utc_now_iso8601();

}  // namespace game_load::win32
