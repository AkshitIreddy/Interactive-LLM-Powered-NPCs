#ifdef _WIN32

#include "windows_manual_actor_picker.hpp"

#include <dwmapi.h>
#include <windowsx.h>

#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <mutex>
#include <thread>
#include <utility>

namespace npc::media::windows {

namespace {

constexpr wchar_t picker_class_name[] = L"NpcMediaBrokerManualActorPicker";
constexpr UINT guard_timer_id = 1U;
constexpr UINT close_picker_message = WM_APP + 31U;

[[nodiscard]] std::uint64_t qpc_now_local() noexcept {
    LARGE_INTEGER value{};
    return QueryPerformanceCounter(&value) && value.QuadPart > 0
               ? static_cast<std::uint64_t>(value.QuadPart)
               : 0U;
}

[[nodiscard]] ManualActorPickerStatus status_for_guard(
    const ManualActorPickerGuardState state) noexcept {
    switch (state) {
    case ManualActorPickerGuardState::current: return ManualActorPickerStatus::pending;
    case ManualActorPickerGuardState::cancelled: return ManualActorPickerStatus::cancelled;
    case ManualActorPickerGuardState::target_lost: return ManualActorPickerStatus::target_lost;
    case ManualActorPickerGuardState::target_resized: return ManualActorPickerStatus::target_resized;
    case ManualActorPickerGuardState::dpi_changed: return ManualActorPickerStatus::dpi_changed;
    case ManualActorPickerGuardState::device_changed: return ManualActorPickerStatus::device_changed;
    case ManualActorPickerGuardState::capture_changed: return ManualActorPickerStatus::capture_changed;
    }
    return ManualActorPickerStatus::internal_error;
}

[[nodiscard]] bool hardware_pointer_message(ManualActorPointerKind& kind) noexcept {
    INPUT_MESSAGE_SOURCE source{};
    if (!GetCurrentInputMessageSource(&source) || source.originId != IMO_HARDWARE) {
        kind = ManualActorPointerKind::none;
        return false;
    }
    switch (source.deviceType) {
    case IMDT_MOUSE: kind = ManualActorPointerKind::mouse; break;
    case IMDT_TOUCH: kind = ManualActorPointerKind::touch; break;
    case IMDT_PEN: kind = ManualActorPointerKind::pen; break;
    default: kind = ManualActorPointerKind::none; break;
    }
    return kind != ManualActorPointerKind::none;
}

} // namespace

struct ManualActorPickerOverlay::Impl {
    mutable std::mutex mutex;
    std::condition_variable ready_condition;
    std::jthread worker;
    std::atomic<HWND> window{};
    ManualActorPickerStartContext context;
    ManualActorPickerReceipt receipt;
    std::vector<RectI> candidate_client_bounds;
    bool ready{};
    bool startup_failed{};
    bool escape_was_down{};
    std::string startup_error;

    enum class PointerButton { left, right, middle, x1, x2 };
    struct PendingGesture {
        PointerButton button{PointerButton::left};
        ManualActorPointerKind pointer_kind{ManualActorPointerKind::none};
        ManualActorPickerStatus down_result{ManualActorPickerStatus::untrusted_pointer_input};
        std::optional<std::size_t> candidate_index;
        std::optional<ManualActorPickerStatus> forced_terminal;
        bool hardware_origin{};
        bool extra_input{};
    };
    struct ActivePointer {
        std::uint32_t id{};
        ManualActorPointerKind kind{ManualActorPointerKind::none};
    };
    std::optional<PendingGesture> gesture;
    std::vector<ActivePointer> active_pointers;
    bool raw_pointer_sequence_rejected{};

    [[nodiscard]] static std::optional<std::pair<PointerButton, bool>> mouse_button(
        const UINT message, const WPARAM wparam) noexcept {
        switch (message) {
        case WM_LBUTTONDOWN: return std::pair{PointerButton::left, true};
        case WM_LBUTTONUP: return std::pair{PointerButton::left, false};
        case WM_RBUTTONDOWN: return std::pair{PointerButton::right, true};
        case WM_RBUTTONUP: return std::pair{PointerButton::right, false};
        case WM_MBUTTONDOWN: return std::pair{PointerButton::middle, true};
        case WM_MBUTTONUP: return std::pair{PointerButton::middle, false};
        case WM_XBUTTONDOWN:
            return std::pair{GET_XBUTTON_WPARAM(wparam) == XBUTTON1
                                 ? PointerButton::x1 : PointerButton::x2, true};
        case WM_XBUTTONUP:
            return std::pair{GET_XBUTTON_WPARAM(wparam) == XBUTTON1
                                 ? PointerButton::x1 : PointerButton::x2, false};
        default: return std::nullopt;
        }
    }

    [[nodiscard]] std::vector<std::size_t> hit_candidates(const POINT point) const {
        std::vector<std::size_t> hits;
        for (std::size_t index = 0; index < candidate_client_bounds.size(); ++index) {
            const auto& bounds = candidate_client_bounds[index];
            if (point.x >= bounds.left && point.x < bounds.right &&
                point.y >= bounds.top && point.y < bounds.bottom) hits.push_back(index);
        }
        return hits;
    }

    void finish_locked(const ManualActorPickerStatus status,
                       const std::uint64_t clicked_qpc = 0U,
                       const ManualActorPointerKind pointer_kind = ManualActorPointerKind::none,
                       const std::optional<std::size_t> selected_index = std::nullopt,
                       const bool hardware_click = false) {
        if (receipt.status != ManualActorPickerStatus::pending) return;
        receipt.status = status;
        receipt.selected_actor_id = 0U;
        receipt.selected_track_id = 0U;
        receipt.selected_track_epoch = 0U;
        receipt.clicked_qpc = 0U;
        receipt.pointer_kind = ManualActorPointerKind::none;
        receipt.single_hardware_pointer_click = false;
        const bool click_terminal = status == ManualActorPickerStatus::selected ||
            status == ManualActorPickerStatus::click_outside_detected_roi ||
            status == ManualActorPickerStatus::ambiguous_detected_roi ||
            status == ManualActorPickerStatus::untrusted_pointer_input;
        if (click_terminal) {
            receipt.clicked_qpc = clicked_qpc;
            receipt.pointer_kind = pointer_kind;
        }
        if (status == ManualActorPickerStatus::selected && selected_index &&
            *selected_index < context.candidates.size() && hardware_click) {
            const auto& selected = context.candidates[*selected_index];
            receipt.selected_actor_id = selected.actor_id;
            receipt.selected_track_id = selected.track_id;
            receipt.selected_track_epoch = selected.track_epoch;
            receipt.single_hardware_pointer_click = true;
        }
        const auto attested = qpc_now_local();
        receipt.attested_at_qpc = std::max(attested, receipt.clicked_qpc);
        gesture.reset();
    }

    void finish(const ManualActorPickerStatus status,
                const ManualActorPointerKind pointer_kind = ManualActorPointerKind::none,
                const std::optional<std::size_t> selected_index = std::nullopt,
                const bool hardware_click = false) {
        std::scoped_lock lock(mutex);
        finish_locked(status, 0U, pointer_kind, selected_index, hardware_click);
    }

    void observe_pointer_message(const UINT message, const WPARAM wparam) {
        POINTER_INFO info{};
        const auto pointer_id = GET_POINTERID_WPARAM(wparam);
        ManualActorPointerKind kind{};
        const bool hardware = hardware_pointer_message(kind) &&
                              GetPointerInfo(pointer_id, &info) != FALSE;
        std::scoped_lock lock(mutex);
        if (receipt.status != ManualActorPickerStatus::pending) return;
        if (!hardware || (info.pointerType != PT_TOUCH && info.pointerType != PT_PEN &&
                          info.pointerType != PT_MOUSE)) raw_pointer_sequence_rejected = true;
        if (message == WM_POINTERDOWN) {
            if (std::ranges::any_of(active_pointers, [pointer_id](const ActivePointer& active) {
                    return active.id == pointer_id;
                })) {
                raw_pointer_sequence_rejected = true;
            } else {
                active_pointers.push_back({pointer_id, kind});
                if (active_pointers.size() != 1U) raw_pointer_sequence_rejected = true;
            }
            if (gesture && active_pointers.size() > 1U) gesture->extra_input = true;
        } else if (message == WM_POINTERUP) {
            const auto iterator = std::find_if(active_pointers.begin(), active_pointers.end(),
                [pointer_id](const ActivePointer& active) { return active.id == pointer_id; });
            if (iterator == active_pointers.end()) raw_pointer_sequence_rejected = true;
            else {
                if (!hardware || iterator->kind != kind) raw_pointer_sequence_rejected = true;
                active_pointers.erase(iterator);
            }
        }
    }

    void cancel_from_overlay() {
        bool close{};
        {
            std::scoped_lock lock(mutex);
            if (receipt.status != ManualActorPickerStatus::pending) return;
            if (gesture) gesture->forced_terminal = ManualActorPickerStatus::cancelled;
            else {
                finish_locked(ManualActorPickerStatus::cancelled);
                close = true;
            }
        }
        if (close) {
            PostMessageW(window.load(std::memory_order_acquire), close_picker_message, 0U, 0U);
        }
    }

    void handle_mouse_button(const UINT message, const WPARAM wparam, const LPARAM lparam) {
        const auto button_event = mouse_button(message, wparam);
        if (!button_event) return;
        const auto [button, down] = *button_event;
        ManualActorPointerKind kind{};
        const bool hardware = hardware_pointer_message(kind);
        const POINT point{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
        const auto hits = hit_candidates(point);
        bool close{};
        {
            // The final guard and every receipt field are committed while this
            // one lock excludes authenticated cancellation. No intermediate
            // clicked fields can survive a losing cancellation race.
            std::scoped_lock lock(mutex);
            if (receipt.status != ManualActorPickerStatus::pending) return;
            if (down) {
                if (gesture) {
                    gesture->extra_input = true;
                    return;
                }
                PendingGesture pending;
                pending.button = button;
                pending.pointer_kind = kind;
                pending.hardware_origin = hardware;
                pending.extra_input = raw_pointer_sequence_rejected;
                if (!hardware || button != PointerButton::left) {
                    pending.down_result = ManualActorPickerStatus::untrusted_pointer_input;
                } else if (hits.empty()) {
                    pending.down_result = ManualActorPickerStatus::click_outside_detected_roi;
                } else if (hits.size() != 1U) {
                    pending.down_result = ManualActorPickerStatus::ambiguous_detected_roi;
                } else {
                    pending.down_result = ManualActorPickerStatus::selected;
                    pending.candidate_index = hits.front();
                }
                gesture = pending;
                SetCapture(window.load(std::memory_order_acquire));
                return;
            }
            if (!gesture) {
                finish_locked(ManualActorPickerStatus::untrusted_pointer_input,
                              qpc_now_local(), kind);
                close = true;
            } else if (gesture->button != button) {
                gesture->extra_input = true;
            } else {
                auto terminal = gesture->forced_terminal.value_or(gesture->down_result);
                std::optional<std::size_t> selected = gesture->candidate_index;
                const bool exact_hardware_up = hardware && gesture->hardware_origin &&
                                               kind == gesture->pointer_kind;
                if (!exact_hardware_up || gesture->extra_input || raw_pointer_sequence_rejected) {
                    terminal = ManualActorPickerStatus::untrusted_pointer_input;
                    selected.reset();
                } else if (terminal == ManualActorPickerStatus::selected) {
                    if (hits.empty()) {
                        terminal = ManualActorPickerStatus::click_outside_detected_roi;
                        selected.reset();
                    } else if (hits.size() != 1U || !selected || hits.front() != *selected) {
                        terminal = hits.size() > 1U
                            ? ManualActorPickerStatus::ambiguous_detected_roi
                            : ManualActorPickerStatus::click_outside_detected_roi;
                        selected.reset();
                    }
                }
                if (context.query_guard) {
                    const auto guarded = status_for_guard(context.query_guard());
                    if (guarded != ManualActorPickerStatus::pending) {
                        terminal = guarded;
                        selected.reset();
                    }
                } else {
                    terminal = ManualActorPickerStatus::internal_error;
                    selected.reset();
                }
                const bool click_terminal = terminal == ManualActorPickerStatus::selected ||
                    terminal == ManualActorPickerStatus::click_outside_detected_roi ||
                    terminal == ManualActorPickerStatus::ambiguous_detected_roi ||
                    terminal == ManualActorPickerStatus::untrusted_pointer_input;
                finish_locked(terminal, click_terminal ? qpc_now_local() : 0U,
                              click_terminal ? kind : ManualActorPointerKind::none,
                              selected, terminal == ManualActorPickerStatus::selected &&
                                            exact_hardware_up);
                close = true;
            }
        }
        if (close) {
            ReleaseCapture();
            PostMessageW(window.load(std::memory_order_acquire), close_picker_message, 0U, 0U);
        }
    }

    void check_guard() {
        ManualActorPickerStatus terminal = ManualActorPickerStatus::pending;
        const bool escape_down = (GetAsyncKeyState(VK_ESCAPE) & 0x8000) != 0;
        bool close{};
        {
            std::scoped_lock lock(mutex);
            if (receipt.status != ManualActorPickerStatus::pending) return;
            const auto now = qpc_now_local();
            const auto timeout_ticks = receipt.qpc_frequency * context.timeout_ms / 1000U;
            if (now == 0U || now < receipt.began_qpc) {
                terminal = ManualActorPickerStatus::internal_error;
            } else if (now - receipt.began_qpc >= timeout_ticks) {
                terminal = ManualActorPickerStatus::timed_out;
            } else if (context.query_guard) {
                terminal = status_for_guard(context.query_guard());
            }
            if (terminal == ManualActorPickerStatus::pending && escape_down &&
                !escape_was_down) terminal = ManualActorPickerStatus::cancelled;
            escape_was_down = escape_down;
            if (terminal != ManualActorPickerStatus::pending) {
                if (gesture) gesture->forced_terminal = terminal;
                else {
                    finish_locked(terminal);
                    close = true;
                }
            }
        }
        if (close) {
            PostMessageW(window.load(std::memory_order_acquire), close_picker_message, 0U, 0U);
        }
    }

    static LRESULT CALLBACK window_proc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
        auto* self = reinterpret_cast<Impl*>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
        if (message == WM_NCCREATE) {
            const auto* create = reinterpret_cast<const CREATESTRUCTW*>(lparam);
            self = static_cast<Impl*>(create->lpCreateParams);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
        }
        switch (message) {
        case WM_MOUSEACTIVATE: return MA_NOACTIVATE;
        case WM_NCHITTEST: return HTCLIENT;
        case WM_ERASEBKGND: return 1;
        case WM_LBUTTONDOWN:
        case WM_LBUTTONUP:
        case WM_RBUTTONDOWN:
        case WM_RBUTTONUP:
        case WM_MBUTTONDOWN:
        case WM_MBUTTONUP:
        case WM_XBUTTONDOWN:
        case WM_XBUTTONUP:
            if (self) self->handle_mouse_button(message, wparam, lparam);
            return 0;
        case WM_POINTERDOWN:
        case WM_POINTERUP:
            if (self) self->observe_pointer_message(message, wparam);
            // DefWindowProc preserves the OS compatibility-mouse sequence; the
            // overlay remains alive and captured until that matching button-up
            // is swallowed and attested.
            return DefWindowProcW(hwnd, message, wparam, lparam);
        case WM_KEYDOWN:
            if (self && wparam == VK_ESCAPE) {
                // If a button is held, defer closure until its matching up so
                // that the release cannot fall through into the game.
                self->cancel_from_overlay();
            }
            return 0;
        case WM_TIMER:
            if (self && wparam == guard_timer_id) self->check_guard();
            return 0;
        case close_picker_message:
            DestroyWindow(hwnd);
            return 0;
        case WM_DESTROY:
            KillTimer(hwnd, guard_timer_id);
            PostQuitMessage(0);
            return 0;
        default: return DefWindowProcW(hwnd, message, wparam, lparam);
        }
    }

    static bool register_window_class() {
        static std::once_flag once;
        static bool registered{};
        std::call_once(once, [] {
            WNDCLASSEXW definition{};
            definition.cbSize = sizeof(definition);
            definition.lpfnWndProc = window_proc;
            definition.hInstance = GetModuleHandleW(nullptr);
            definition.lpszClassName = picker_class_name;
            definition.hCursor = LoadCursorW(nullptr, IDC_CROSS);
            registered = RegisterClassExW(&definition) != 0 ||
                         GetLastError() == ERROR_CLASS_ALREADY_EXISTS;
        });
        return registered;
    }

    [[nodiscard]] bool render_frozen_frame(HWND hwnd) {
        const auto& overlay = context.overlay_geometry;
        const auto& bounds = overlay.clipped_desktop_bounds_px;
        if (!bounds.valid() || !overlay.source_crop_px.valid() ||
            !context.source_size_px.valid() || context.source_stride_bytes == 0U) return false;
        HDC screen = GetDC(nullptr);
        HDC memory = CreateCompatibleDC(screen);
        BITMAPINFO info{};
        info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        info.bmiHeader.biWidth = bounds.width();
        info.bmiHeader.biHeight = -bounds.height();
        info.bmiHeader.biPlanes = 1U;
        info.bmiHeader.biBitCount = 32U;
        info.bmiHeader.biCompression = BI_RGB;
        void* pixels{};
        HBITMAP bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &pixels, nullptr, 0U);
        if (!screen || !memory || !bitmap || !pixels) {
            if (bitmap) DeleteObject(bitmap);
            if (memory) DeleteDC(memory);
            if (screen) ReleaseDC(nullptr, screen);
            return false;
        }
        const auto old_bitmap = SelectObject(memory, bitmap);
        BITMAPINFO source_info{};
        source_info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        source_info.bmiHeader.biWidth = context.source_size_px.width;
        source_info.bmiHeader.biHeight = -context.source_size_px.height;
        source_info.bmiHeader.biPlanes = 1U;
        source_info.bmiHeader.biBitCount = 32U;
        source_info.bmiHeader.biCompression = BI_RGB;
        const auto crop = overlay.source_crop_px;
        SetStretchBltMode(memory, HALFTONE);
        const auto copied = StretchDIBits(
            memory, 0, 0, bounds.width(), bounds.height(), crop.left, crop.top,
            crop.width(), crop.height(), context.source_bgra.data(), &source_info,
            DIB_RGB_COLORS, SRCCOPY);
        candidate_client_bounds.clear();
        const auto roi_pen = CreatePen(PS_SOLID, 4, RGB(57, 220, 255));
        const auto old_pen = SelectObject(memory, roi_pen);
        const auto old_brush = SelectObject(memory, GetStockObject(HOLLOW_BRUSH));
        for (const auto& candidate : context.candidates) {
            const auto desktop = map_normalized_source_rect(candidate.normalized_bounds, overlay);
            if (!desktop) continue;
            const RectI local{desktop->left - bounds.left, desktop->top - bounds.top,
                              desktop->right - bounds.left, desktop->bottom - bounds.top};
            candidate_client_bounds.push_back(local);
            Rectangle(memory, local.left, local.top, local.right, local.bottom);
        }
        SelectObject(memory, old_brush);
        SelectObject(memory, old_pen);
        DeleteObject(roi_pen);
        RECT banner{0, 0, bounds.width(), std::min(44, bounds.height())};
        FillRect(memory, &banner, static_cast<HBRUSH>(GetStockObject(BLACK_BRUSH)));
        SetBkMode(memory, TRANSPARENT);
        SetTextColor(memory, RGB(255, 255, 255));
        const wchar_t instruction[] = L"Click one highlighted actor - Esc cancels";
        DrawTextW(memory, instruction, -1, &banner, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        auto* bgra = static_cast<std::byte*>(pixels);
        const auto pixel_count = static_cast<std::size_t>(bounds.width()) * bounds.height();
        for (std::size_t index = 0; index < pixel_count; ++index) bgra[index * 4U + 3U] = std::byte{0xff};
        POINT source_point{};
        SIZE size{bounds.width(), bounds.height()};
        POINT destination{bounds.left, bounds.top};
        BLENDFUNCTION blend{AC_SRC_OVER, 0U, 255U, AC_SRC_ALPHA};
        const bool updated = copied != GDI_ERROR &&
            UpdateLayeredWindow(hwnd, screen, &destination, &size, memory, &source_point,
                                0U, &blend, ULW_ALPHA) != FALSE;
        SelectObject(memory, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(nullptr, screen);
        return updated && candidate_client_bounds.size() == context.candidates.size();
    }

    void run(std::stop_token token) {
        if (!register_window_class()) {
            std::scoped_lock lock(mutex);
            startup_failed = true;
            startup_error = "Manual actor picker window class registration failed";
            ready_condition.notify_all();
            return;
        }
        const auto bounds = context.overlay_geometry.clipped_desktop_bounds_px;
        const auto hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
            picker_class_name, L"", WS_POPUP, bounds.left, bounds.top, bounds.width(),
            bounds.height(), context.target_window, nullptr, GetModuleHandleW(nullptr), this);
        window.store(hwnd, std::memory_order_release);
        constexpr DWORD exclude_from_capture = 0x00000011;
        DWORD affinity{};
        const auto extended_style = hwnd ? static_cast<DWORD>(GetWindowLongPtrW(hwnd, GWL_EXSTYLE)) : 0U;
        const bool protected_overlay = hwnd &&
            SetWindowDisplayAffinity(hwnd, exclude_from_capture) &&
            GetWindowDisplayAffinity(hwnd, &affinity) && affinity == exclude_from_capture &&
            (extended_style & WS_EX_NOACTIVATE) != 0U &&
            (extended_style & WS_EX_TRANSPARENT) == 0U && render_frozen_frame(hwnd) &&
            SetTimer(hwnd, guard_timer_id, 16U, nullptr) != 0U;
        if (!protected_overlay) {
            if (hwnd) DestroyWindow(hwnd);
            window.store(nullptr, std::memory_order_release);
            std::scoped_lock lock(mutex);
            startup_failed = true;
            startup_error = "Capture-excluded nonactivating picker overlay could not be established";
            ready_condition.notify_all();
            return;
        }
        bool shown{};
        {
            // This is the authoritative synchronous recheck. The periodic
            // timer below is only supplemental; no overlay becomes visible on
            // a target/PID/session/capture/device/geometry/DPI mismatch.
            // Holding the receipt mutex through SetWindowPos makes show and
            // authenticated cancellation a single ordered transaction.
            std::scoped_lock lock(mutex);
            if (receipt.status == ManualActorPickerStatus::pending && context.query_guard) {
                const auto pre_show_guard = context.query_guard();
                if (pre_show_guard == ManualActorPickerGuardState::current) {
                    RECT shown_bounds{};
                    shown = SetWindowPos(
                        hwnd, HWND_TOPMOST, bounds.left, bounds.top, bounds.width(), bounds.height(),
                        SWP_NOACTIVATE | SWP_SHOWWINDOW) != FALSE &&
                        GetWindowRect(hwnd, &shown_bounds) && shown_bounds.left == bounds.left &&
                        shown_bounds.top == bounds.top && shown_bounds.right == bounds.right &&
                        shown_bounds.bottom == bounds.bottom && IsWindowVisible(hwnd) != FALSE;
                }
            }
        }
        if (!shown) {
            DestroyWindow(hwnd);
            window.store(nullptr, std::memory_order_release);
            std::scoped_lock lock(mutex);
            startup_failed = true;
            startup_error = receipt.status == ManualActorPickerStatus::pending
                ? "Manual actor picker binding changed or exact target bounds could not be shown"
                : "Manual actor picker was cancelled immediately before show";
            ready_condition.notify_all();
            return;
        }
        {
            std::scoped_lock lock(mutex);
            escape_was_down = (GetAsyncKeyState(VK_ESCAPE) & 0x8000) != 0;
            ready = true;
            ready_condition.notify_all();
        }
        MSG message{};
        while (!token.stop_requested() && GetMessageW(&message, nullptr, 0U, 0U) > 0) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if (IsWindow(hwnd)) DestroyWindow(hwnd);
        window.store(nullptr, std::memory_order_release);
        std::scoped_lock lock(mutex);
        if (receipt.status == ManualActorPickerStatus::pending) {
            receipt.status = token.stop_requested() ? ManualActorPickerStatus::cancelled
                                                    : ManualActorPickerStatus::internal_error;
            receipt.attested_at_qpc = qpc_now_local();
        }
        std::fill(context.source_bgra.begin(), context.source_bgra.end(), std::byte{});
        context.source_bgra.clear();
    }
};

ManualActorPickerOverlay::ManualActorPickerOverlay() : impl_(std::make_unique<Impl>()) {}
ManualActorPickerOverlay::~ManualActorPickerOverlay() { cancel(); }

bool ManualActorPickerOverlay::begin(ManualActorPickerStartContext context,
                                     ManualActorPickerReceipt& receipt,
                                     Failure& failure) {
    cancel();
    if (impl_->worker.joinable()) impl_->worker.join();
    {
        std::scoped_lock lock(impl_->mutex);
        impl_->context = std::move(context);
        impl_->receipt = impl_->context.receipt;
        impl_->ready = false;
        impl_->startup_failed = false;
        impl_->startup_error.clear();
        impl_->candidate_client_bounds.clear();
        impl_->gesture.reset();
        impl_->active_pointers.clear();
        impl_->raw_pointer_sequence_rejected = false;
    }
    impl_->worker = std::jthread([state = impl_.get()](const std::stop_token token) {
        state->run(token);
    });
    std::unique_lock lock(impl_->mutex);
    if (!impl_->ready_condition.wait_for(lock, std::chrono::seconds(2), [&] {
            return impl_->ready || impl_->startup_failed;
        }) || impl_->startup_failed) {
        failure = {FailureDomain::overlay, FailureCode::unsupported_path, false,
                   impl_->startup_error.empty() ? "Manual actor picker startup timed out"
                                                : impl_->startup_error};
        lock.unlock();
        cancel();
        if (impl_->worker.joinable()) impl_->worker.join();
        return false;
    }
    receipt = impl_->receipt;
    return true;
}

bool ManualActorPickerOverlay::query(const std::string_view request_id,
                                     ManualActorPickerReceipt& receipt,
                                     Failure& failure) const {
    std::scoped_lock lock(impl_->mutex);
    if (request_id.empty() || request_id != impl_->receipt.request_id) {
        failure = {FailureDomain::overlay, FailureCode::access_denied, false,
                   "Manual actor picker request identity is unknown"};
        return false;
    }
    receipt = impl_->receipt;
    return true;
}

bool ManualActorPickerOverlay::cancel(const std::string_view request_id,
                                      ManualActorPickerReceipt& receipt,
                                      Failure& failure) {
    {
        std::scoped_lock lock(impl_->mutex);
        if (request_id.empty() || request_id != impl_->receipt.request_id) {
            failure = {FailureDomain::overlay, FailureCode::access_denied, false,
                       "Manual actor picker request identity is unknown"};
            return false;
        }
    }
    impl_->finish(ManualActorPickerStatus::cancelled);
    if (const auto hwnd = impl_->window.load(std::memory_order_acquire)) {
        PostMessageW(hwnd, close_picker_message, 0U, 0U);
    }
    return query(request_id, receipt, failure);
}

void ManualActorPickerOverlay::cancel() noexcept {
    if (!impl_) return;
    impl_->finish(ManualActorPickerStatus::cancelled);
    if (const auto hwnd = impl_->window.load(std::memory_order_acquire)) {
        PostMessageW(hwnd, close_picker_message, 0U, 0U);
    }
    if (impl_->worker.joinable()) {
        impl_->worker.request_stop();
        PostThreadMessageW(GetThreadId(impl_->worker.native_handle()), WM_QUIT, 0U, 0U);
        impl_->worker.join();
    }
    std::scoped_lock lock(impl_->mutex);
    std::fill(impl_->context.source_bgra.begin(), impl_->context.source_bgra.end(), std::byte{});
    impl_->context.source_bgra.clear();
}

} // namespace npc::media::windows

#endif
