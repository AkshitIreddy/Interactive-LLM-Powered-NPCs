#pragma once

#include <Windows.h>
#include <evntprov.h>

#include <cstdint>
#include <string>
#include <string_view>

namespace game_load::win32 {

class EtwMarkers final {
 public:
  EtwMarkers() noexcept;
  ~EtwMarkers();
  EtwMarkers(const EtwMarkers&) = delete;
  EtwMarkers& operator=(const EtwMarkers&) = delete;

  void event(std::wstring_view message, UCHAR level = 4,
             ULONGLONG keyword = 0x1) const noexcept;
  [[nodiscard]] bool available() const noexcept { return handle_ != 0; }

 private:
  REGHANDLE handle_ = 0;
};

class EtwScope final {
 public:
  EtwScope(const EtwMarkers& markers, std::wstring_view name,
           std::uint64_t frame) noexcept;
  ~EtwScope();
  EtwScope(const EtwScope&) = delete;
  EtwScope& operator=(const EtwScope&) = delete;

 private:
  const EtwMarkers& markers_;
  std::wstring name_;
  std::uint64_t frame_;
};

}  // namespace game_load::win32
