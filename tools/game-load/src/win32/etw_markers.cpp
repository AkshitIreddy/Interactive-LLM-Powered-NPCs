#include "etw_markers.hpp"

#include <string>

namespace game_load::win32 {
namespace {

// Stable provider: ResponseConsole-GameLoad-Harness
// {2D7F3C2B-337D-4F6A-89E7-97B31C4D4B5A}
constexpr GUID kProviderGuid = {0x2d7f3c2b,
                                0x337d,
                                0x4f6a,
                                {0x89, 0xe7, 0x97, 0xb3, 0x1c, 0x4d, 0x4b, 0x5a}};

}  // namespace

EtwMarkers::EtwMarkers() noexcept { EventRegister(&kProviderGuid, nullptr, nullptr, &handle_); }

EtwMarkers::~EtwMarkers() {
  if (handle_ != 0) EventUnregister(handle_);
}

void EtwMarkers::event(std::wstring_view message, UCHAR level,
                       ULONGLONG keyword) const noexcept {
  if (handle_ == 0) return;
  // EventWriteString requires a null-terminated pointer; make the ownership explicit
  // instead of assuming the string_view originated from a C string.
  const std::wstring stable(message);
  EventWriteString(handle_, level, keyword, stable.c_str());
}

EtwScope::EtwScope(const EtwMarkers& markers, std::wstring_view name,
                   std::uint64_t frame) noexcept
    : markers_(markers), name_(name), frame_(frame) {
  markers_.event(name_ + L".Begin frame=" + std::to_wstring(frame_));
}

EtwScope::~EtwScope() {
  markers_.event(name_ + L".End frame=" + std::to_wstring(frame_));
}

}  // namespace game_load::win32
