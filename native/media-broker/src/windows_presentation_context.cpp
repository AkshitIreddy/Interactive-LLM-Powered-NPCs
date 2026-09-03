#ifdef _WIN32

#include "windows_presentation_context.hpp"
#include "windows_service.hpp"

#include <Windows.h>
#include <dwmapi.h>

#include <algorithm>
#include <cwchar>
#include <string_view>
#include <vector>

namespace npc::media::windows {

namespace {

[[nodiscard]] std::string utf8(const std::wstring_view value) {
    if (value.empty()) return {};
    const auto length = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                                            static_cast<int>(value.size()), nullptr, 0,
                                            nullptr, nullptr);
    if (length <= 0) return {};
    std::string output(static_cast<std::size_t>(length), '\0');
    if (WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), output.data(), length,
                            nullptr, nullptr) != length) return {};
    return output;
}

[[nodiscard]] bool same_device_name(const wchar_t* left, const wchar_t* right) noexcept {
    return left && right && _wcsicmp(left, right) == 0;
}

[[nodiscard]] std::optional<std::string> process_executable_basename(
    const std::uint32_t process_id) {
    const auto process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id);
    if (!process) return std::nullopt;
    std::wstring path(32'768, L'\0');
    DWORD length = static_cast<DWORD>(path.size());
    const auto queried = QueryFullProcessImageNameW(process, 0, path.data(), &length) != FALSE;
    CloseHandle(process);
    if (!queried || length == 0 || length > path.size()) return std::nullopt;
    path.resize(length);
    const auto separator = path.find_last_of(L"\\/");
    const auto basename = separator == std::wstring::npos
                              ? std::wstring_view{path}
                              : std::wstring_view{path}.substr(separator + 1);
    auto encoded = utf8(basename);
    return encoded.empty() ? std::nullopt : std::optional{std::move(encoded)};
}

struct DisplayPathIdentity {
    LUID adapter_id{};
    std::uint32_t target_id{};
};

[[nodiscard]] std::optional<DisplayPathIdentity> active_path_for_monitor(
    const wchar_t* monitor_device_name) {
    UINT32 path_count{};
    UINT32 mode_count{};
    if (GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &path_count, &mode_count) !=
        ERROR_SUCCESS || path_count == 0 || path_count > 128 || mode_count > 512) {
        return std::nullopt;
    }
    std::vector<DISPLAYCONFIG_PATH_INFO> paths(path_count);
    std::vector<DISPLAYCONFIG_MODE_INFO> modes(mode_count);
    if (QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &path_count, paths.data(),
                           &mode_count, modes.data(), nullptr) != ERROR_SUCCESS) {
        return std::nullopt;
    }
    paths.resize(path_count);
    for (const auto& path : paths) {
        DISPLAYCONFIG_SOURCE_DEVICE_NAME source{};
        source.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        source.header.size = sizeof(source);
        source.header.adapterId = path.sourceInfo.adapterId;
        source.header.id = path.sourceInfo.id;
        if (DisplayConfigGetDeviceInfo(&source.header) == ERROR_SUCCESS &&
            same_device_name(source.viewGdiDeviceName, monitor_device_name)) {
            return DisplayPathIdentity{path.targetInfo.adapterId, path.targetInfo.id};
        }
    }
    return std::nullopt;
}

void query_display_capabilities(const DisplayPathIdentity& path,
                                TrustedSubtitlePresentationContext& context) noexcept {
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2 advanced2{};
    advanced2.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2;
    advanced2.header.size = sizeof(advanced2);
    advanced2.header.adapterId = path.adapter_id;
    advanced2.header.id = path.target_id;
    if (DisplayConfigGetDeviceInfo(&advanced2.header) == ERROR_SUCCESS) {
        context.hdr_evidence_available = true;
        context.hdr_supported = advanced2.highDynamicRangeSupported != 0;
        context.hdr_user_enabled = advanced2.highDynamicRangeUserEnabled != 0;
        context.hdr_active = advanced2.activeColorMode == DISPLAYCONFIG_ADVANCED_COLOR_MODE_HDR;
        context.advanced_color_active = advanced2.advancedColorActive != 0;
        context.active_color_mode = static_cast<std::uint32_t>(advanced2.activeColorMode);
        context.color_encoding_available = true;
        context.color_encoding = static_cast<std::uint32_t>(advanced2.colorEncoding);
        context.bits_per_color_channel = advanced2.bitsPerColorChannel;
        context.target_color_space_available = true;
        switch (advanced2.activeColorMode) {
        case DISPLAYCONFIG_ADVANCED_COLOR_MODE_SDR:
            context.target_color_space = ColorSpace::sdr_srgb;
            break;
        case DISPLAYCONFIG_ADVANCED_COLOR_MODE_WCG:
            context.target_color_space = ColorSpace::sdr_sc_rgb;
            break;
        case DISPLAYCONFIG_ADVANCED_COLOR_MODE_HDR:
            context.target_color_space = ColorSpace::hdr_sc_rgb;
            break;
        default:
            context.target_color_space_available = false;
            context.target_color_space = ColorSpace::unknown;
            break;
        }
    }
    if (!context.color_encoding_available) {
        DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO advanced{};
        advanced.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
        advanced.header.size = sizeof(advanced);
        advanced.header.adapterId = path.adapter_id;
        advanced.header.id = path.target_id;
        if (DisplayConfigGetDeviceInfo(&advanced.header) == ERROR_SUCCESS) {
            // The legacy API proves advanced-color state but cannot distinguish
            // HDR from WCG. Keep HDR evidence unavailable instead of guessing.
            context.advanced_color_active = advanced.advancedColorEnabled != 0;
            context.color_encoding_available = true;
            context.color_encoding = static_cast<std::uint32_t>(advanced.colorEncoding);
            context.bits_per_color_channel = advanced.bitsPerColorChannel;
        }
    }
    DISPLAYCONFIG_SDR_WHITE_LEVEL white{};
    white.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
    white.header.size = sizeof(white);
    white.header.adapterId = path.adapter_id;
    white.header.id = path.target_id;
    if (DisplayConfigGetDeviceInfo(&white.header) == ERROR_SUCCESS &&
        white.SDRWhiteLevel > 0 && white.SDRWhiteLevel <= 100'000) {
        context.sdr_white_level_available = true;
        context.sdr_white_level_nits =
            static_cast<double>(white.SDRWhiteLevel) * 80.0 / 1000.0;
    }
}

} // namespace

std::optional<TrustedSubtitlePresentationContext>
query_trusted_subtitle_presentation_context(const Diagnostics& diagnostics,
                                            std::string& error) {
    if (diagnostics.selected_window == 0 || diagnostics.selected_process_id == 0 ||
        diagnostics.device_generation == 0 || diagnostics.geometry_epoch == 0 ||
        diagnostics.latest_frame_sequence == 0 || diagnostics.latest_frame_qpc == 0 ||
        !diagnostics.latest_content_size_px.valid()) {
        error = "A current advancing selected-target capture is required";
        return std::nullopt;
    }
    const auto window = reinterpret_cast<HWND>(diagnostics.selected_window);
    if (!IsWindow(window)) {
        error = "Selected target window no longer exists";
        return std::nullopt;
    }
    DWORD process_id{};
    GetWindowThreadProcessId(window, &process_id);
    if (process_id != diagnostics.selected_process_id) {
        error = "Selected target window process identity changed";
        return std::nullopt;
    }
    const auto executable = process_executable_basename(process_id);
    if (!executable || diagnostics.selected_executable_name.empty() ||
        _stricmp(executable->c_str(), diagnostics.selected_executable_name.c_str()) != 0) {
        error = "Selected target process executable identity changed";
        return std::nullopt;
    }

    RECT window_rect{};
    if (FAILED(DwmGetWindowAttribute(window, DWMWA_EXTENDED_FRAME_BOUNDS,
                                     &window_rect, sizeof(window_rect))) &&
        !GetWindowRect(window, &window_rect)) {
        error = "Query selected target window bounds failed";
        return std::nullopt;
    }
    RECT client{};
    if (!GetClientRect(window, &client)) {
        error = "Query selected target client viewport failed";
        return std::nullopt;
    }
    POINT client_origin{client.left, client.top};
    if (!ClientToScreen(window, &client_origin)) {
        error = "Map selected target client viewport to desktop failed";
        return std::nullopt;
    }
    const RectI client_bounds{
        client_origin.x,
        client_origin.y,
        client_origin.x + client.right - client.left,
        client_origin.y + client.bottom - client.top,
    };
    if (!client_bounds.valid()) {
        error = "Selected target client viewport is empty";
        return std::nullopt;
    }

    const auto monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
    MONITORINFOEXW monitor_info{};
    monitor_info.cbSize = sizeof(monitor_info);
    if (!monitor || !GetMonitorInfoW(monitor, &monitor_info)) {
        error = "Query selected target monitor failed";
        return std::nullopt;
    }

    TrustedSubtitlePresentationContext context;
    context.selected_process_id = diagnostics.selected_process_id;
    context.selected_window = static_cast<std::uint64_t>(diagnostics.selected_window);
    context.selected_executable_name = *executable;
    context.capture_device_generation = diagnostics.device_generation;
    context.geometry_epoch = diagnostics.geometry_epoch;
    context.source_frame_sequence = diagnostics.latest_frame_sequence;
    context.source_frame_qpc = diagnostics.latest_frame_qpc;
    context.window_bounds_px = {window_rect.left, window_rect.top,
                                window_rect.right, window_rect.bottom};
    context.client_bounds_px = client_bounds;
    context.captured_content_px = diagnostics.latest_content_size_px;
    context.monitor_id = utf8(monitor_info.szDevice);
    context.monitor_bounds_px = {monitor_info.rcMonitor.left, monitor_info.rcMonitor.top,
                                 monitor_info.rcMonitor.right, monitor_info.rcMonitor.bottom};
    context.monitor_work_area_px = {monitor_info.rcWork.left, monitor_info.rcWork.top,
                                    monitor_info.rcWork.right, monitor_info.rcWork.bottom};
    const auto dpi = GetDpiForWindow(window);
    if (dpi != 0) {
        context.dpi_available = true;
        context.dpi_x = dpi;
        context.dpi_y = dpi;
    }
    if (const auto path = active_path_for_monitor(monitor_info.szDevice)) {
        query_display_capabilities(*path, context);
    }
    context.capture_backend = diagnostics.capture_backend;
    context.capture_scope = diagnostics.capture_backend == CaptureBackend::windows_graphics_capture
                                ? 1U
                                : diagnostics.capture_backend == CaptureBackend::desktop_duplication
                                      ? 2U
                                      : 0U;
    context.overlay_capture_excluded = diagnostics.overlay_capture_excluded;
    context.overlay_visuals_allowed = diagnostics.overlay_visuals_allowed;
    if (context.capture_scope == 0) {
        error = "Selected target has no authoritative capture scope";
        return std::nullopt;
    }
    DWORD verified_process_id{};
    GetWindowThreadProcessId(window, &verified_process_id);
    if (verified_process_id != context.selected_process_id) {
        error = "Selected target changed while presentation context was attested";
        return std::nullopt;
    }
    context.attested_at_qpc = qpc_now();
    context.qpc_frequency = qpc_frequency();
    auto attestation = 1469598103934665603ULL;
    const auto mix = [&](const std::uint64_t value) {
        attestation ^= value;
        attestation *= 1099511628211ULL;
    };
    mix(context.selected_process_id);
    mix(context.selected_window);
    mix(context.capture_device_generation);
    mix(context.geometry_epoch);
    mix(context.source_frame_sequence);
    mix(context.source_frame_qpc);
    mix(context.attested_at_qpc);
    for (const unsigned char byte : context.selected_executable_name) mix(byte);
    context.attestation_id = attestation == 0 ? 1 : attestation;
    return context;
}

} // namespace npc::media::windows

#endif
