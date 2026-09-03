#pragma once

#ifdef _WIN32

#include "npc/media_broker/geometry.hpp"

#include <Windows.h>

#include <functional>
#include <memory>
#include <vector>

namespace npc::media::windows {

enum class ManualActorPickerGuardState {
    current,
    cancelled,
    target_lost,
    target_resized,
    dpi_changed,
    device_changed,
    capture_changed,
};

struct ManualActorPickerStartContext {
    HWND target_window{};
    TargetGeometry target_geometry;
    OverlayGeometry overlay_geometry;
    SizeI source_size_px;
    std::uint32_t source_stride_bytes{};
    std::vector<std::byte> source_bgra;
    std::vector<ManualActorCandidate> candidates;
    ManualActorPickerReceipt receipt;
    std::uint32_t timeout_ms{};
    std::function<ManualActorPickerGuardState()> query_guard;
};

class ManualActorPickerOverlay final {
public:
    ManualActorPickerOverlay();
    ~ManualActorPickerOverlay();
    ManualActorPickerOverlay(const ManualActorPickerOverlay&) = delete;
    ManualActorPickerOverlay& operator=(const ManualActorPickerOverlay&) = delete;

    [[nodiscard]] bool begin(ManualActorPickerStartContext context,
                             ManualActorPickerReceipt& receipt,
                             Failure& failure);
    [[nodiscard]] bool query(std::string_view request_id,
                             ManualActorPickerReceipt& receipt,
                             Failure& failure) const;
    [[nodiscard]] bool cancel(std::string_view request_id,
                              ManualActorPickerReceipt& receipt,
                              Failure& failure);
    void cancel() noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace npc::media::windows

#endif
