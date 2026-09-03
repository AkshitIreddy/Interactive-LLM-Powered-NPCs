#ifdef _WIN32

#include "npc/subtitle_renderer/presentation.hpp"
#include "npc/subtitle_renderer/windows_directwrite.hpp"
#include "npc/subtitle_renderer/windows_surface.hpp"

#include <Windows.h>
#include <dwrite.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iomanip>
#include <iterator>
#include <memory>
#include <optional>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;
using namespace npc::subtitle;
using namespace npc::subtitle::windows;

constexpr std::size_t maximum_line_bytes = 256U * 1024U;

[[nodiscard]] bool read_line(HANDLE input, std::string& line) {
    line.clear();
    while (line.size() <= maximum_line_bytes) {
        char value{};
        DWORD read{};
        if (!ReadFile(input, &value, 1, &read, nullptr) || read == 0) {
            return false;
        }
        if (value == '\n') {
            return true;
        }
        if (value != '\r') {
            line.push_back(value);
        }
    }
    return false;
}

void write_line(HANDLE output, const std::string& line) {
    DWORD written{};
    WriteFile(output, line.data(), static_cast<DWORD>(line.size()), &written, nullptr);
    constexpr char newline = '\n';
    WriteFile(output, &newline, 1, &written, nullptr);
}

[[nodiscard]] std::vector<std::string_view> fields(const std::string& line) {
    std::vector<std::string_view> result;
    std::size_t begin{};
    while (begin <= line.size()) {
        const auto end = line.find('\t', begin);
        result.emplace_back(line.data() + begin,
                            (end == std::string::npos ? line.size() : end) - begin);
        if (end == std::string::npos) {
            break;
        }
        begin = end + 1;
    }
    return result;
}

template <typename Integer>
[[nodiscard]] bool number(std::string_view text, Integer& value) {
    const auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    return error == std::errc{} && end == text.data() + text.size();
}

[[nodiscard]] bool floating(std::string_view text, float& value) {
    std::string owned{text};
    char* end{};
    value = std::strtof(owned.c_str(), &end);
    return end == owned.c_str() + owned.size() && std::isfinite(value);
}

[[nodiscard]] std::optional<std::vector<std::byte>> decode_hex(std::string_view text,
                                                               std::size_t maximum_bytes) {
    if (text.size() % 2 != 0 || text.size() / 2 > maximum_bytes) {
        return std::nullopt;
    }
    std::vector<std::byte> output(text.size() / 2);
    const auto nibble = [](const char value) -> int {
        if (value >= '0' && value <= '9') return value - '0';
        if (value >= 'a' && value <= 'f') return value - 'a' + 10;
        if (value >= 'A' && value <= 'F') return value - 'A' + 10;
        return -1;
    };
    for (std::size_t index = 0; index < output.size(); ++index) {
        const int high = nibble(text[index * 2]);
        const int low = nibble(text[index * 2 + 1]);
        if (high < 0 || low < 0) {
            return std::nullopt;
        }
        output[index] = static_cast<std::byte>((high << 4) | low);
    }
    return output;
}

[[nodiscard]] std::optional<std::string> decode_text(std::string_view text,
                                                     std::size_t maximum_bytes) {
    const auto bytes = decode_hex(text, maximum_bytes);
    if (!bytes) {
        return std::nullopt;
    }
    return std::string(reinterpret_cast<const char*>(bytes->data()), bytes->size());
}

[[nodiscard]] std::string encode_hex(std::string_view text) {
    constexpr char digits[] = "0123456789abcdef";
    std::string output;
    output.reserve(text.size() * 2);
    for (const unsigned char value : text) {
        output.push_back(digits[value >> 4]);
        output.push_back(digits[value & 0x0F]);
    }
    return output;
}

[[nodiscard]] std::optional<std::string> environment(const wchar_t* name) {
    const DWORD size = GetEnvironmentVariableW(name, nullptr, 0);
    if (size <= 1 || size > 4096) {
        return std::nullopt;
    }
    std::wstring wide(size, L'\0');
    if (GetEnvironmentVariableW(name, wide.data(), size) + 1 != size) {
        return std::nullopt;
    }
    wide.resize(size - 1);
    const int utf8_size = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, wide.data(),
                                              static_cast<int>(wide.size()), nullptr, 0, nullptr,
                                              nullptr);
    if (utf8_size <= 0) {
        return std::nullopt;
    }
    std::string utf8(static_cast<std::size_t>(utf8_size), '\0');
    if (WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, wide.data(),
                            static_cast<int>(wide.size()), utf8.data(), utf8_size, nullptr,
                            nullptr) != utf8_size) {
        return std::nullopt;
    }
    return utf8;
}

[[nodiscard]] std::uint64_t qpc_now() {
    LARGE_INTEGER value{};
    QueryPerformanceCounter(&value);
    return static_cast<std::uint64_t>(value.QuadPart);
}

void pump_messages() {
    MSG message{};
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

[[nodiscard]] RectF console_viewport() {
    return {static_cast<float>(GetSystemMetrics(SM_XVIRTUALSCREEN)),
            static_cast<float>(GetSystemMetrics(SM_YVIRTUALSCREEN)),
            static_cast<float>(GetSystemMetrics(SM_CXVIRTUALSCREEN)),
            static_cast<float>(GetSystemMetrics(SM_CYVIRTUALSCREEN))};
}

[[nodiscard]] bool target_process_matches(std::uint32_t pid, std::string_view executable) {
    if (pid == 0 || executable.empty()) {
        return false;
    }
    const HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!process) {
        return false;
    }
    std::wstring path(32'768, L'\0');
    DWORD size = static_cast<DWORD>(path.size());
    const bool queried = QueryFullProcessImageNameW(process, 0, path.data(), &size) != FALSE;
    CloseHandle(process);
    if (!queried || size == 0) {
        return false;
    }
    path.resize(size);
    const auto slash = path.find_last_of(L"\\/");
    const std::wstring leaf = slash == std::wstring::npos ? path : path.substr(slash + 1);
    std::wstring expected;
    const int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, executable.data(),
                                           static_cast<int>(executable.size()), nullptr, 0);
    if (length <= 0) return false;
    expected.resize(static_cast<std::size_t>(length));
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, executable.data(),
                        static_cast<int>(executable.size()), expected.data(), length);
    return CompareStringOrdinal(leaf.data(), static_cast<int>(leaf.size()), expected.data(),
                                static_cast<int>(expected.size()), TRUE) == CSTR_EQUAL;
}

[[nodiscard]] std::optional<std::uint64_t> process_creation_time(const std::uint32_t pid) {
    const HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!process) {
        return std::nullopt;
    }
    FILETIME creation{}, exit{}, kernel{}, user{};
    const bool queried = GetProcessTimes(process, &creation, &exit, &kernel, &user) != FALSE;
    CloseHandle(process);
    if (!queried) {
        return std::nullopt;
    }
    ULARGE_INTEGER value{};
    value.LowPart = creation.dwLowDateTime;
    value.HighPart = creation.dwHighDateTime;
    return value.QuadPart;
}

[[nodiscard]] bool window_belongs_to_process(const std::uint64_t raw_window,
                                              const std::uint32_t pid) noexcept {
    const HWND window = reinterpret_cast<HWND>(static_cast<std::uintptr_t>(raw_window));
    DWORD owner{};
    return window != nullptr && IsWindow(window) != FALSE &&
           GetWindowThreadProcessId(window, &owner) != 0 && owner == pid;
}

[[nodiscard]] bool window_geometry_matches(const std::uint64_t raw_window,
                                           const RectF& expected,
                                           const std::uint32_t dpi_x,
                                           const std::uint32_t dpi_y) noexcept {
    const HWND window = reinterpret_cast<HWND>(static_cast<std::uintptr_t>(raw_window));
    RECT client{};
    POINT origin{};
    if (!window || !GetClientRect(window, &client) || !ClientToScreen(window, &origin)) {
        return false;
    }
    const auto width = client.right - client.left;
    const auto height = client.bottom - client.top;
    const auto dpi = GetDpiForWindow(window);
    return width > 0 && height > 0 && expected.x == static_cast<float>(origin.x) &&
           expected.y == static_cast<float>(origin.y) &&
           expected.width == static_cast<float>(width) &&
           expected.height == static_cast<float>(height) && dpi == dpi_x && dpi == dpi_y;
}

[[nodiscard]] std::string error_line(std::uint64_t sequence, std::string_view code) {
    return "ERR\t" + std::to_string(sequence) + '\t' + std::string(code);
}

[[nodiscard]] std::optional<std::vector<RectF>> exclusions(std::string_view field) {
    std::vector<RectF> output;
    while (!field.empty()) {
        const auto separator = field.find(';');
        const auto entry = field.substr(0, separator);
        std::array<std::string_view, 4> coordinate{};
        std::size_t begin{};
        bool valid = true;
        for (std::size_t index{}; index < coordinate.size(); ++index) {
            const auto comma = entry.find(',', begin);
            const auto end = index + 1 == coordinate.size() ? entry.size() : comma;
            if (end == std::string_view::npos || (index + 1 == coordinate.size() && comma != std::string_view::npos)) {
                valid = false;
                break;
            }
            coordinate[index] = entry.substr(begin, end - begin);
            begin = end + 1;
        }
        std::int32_t x{}, y{};
        std::uint32_t width{}, height{};
        if (!valid || !number(coordinate[0], x) || !number(coordinate[1], y) ||
            !number(coordinate[2], width) || !number(coordinate[3], height) || width == 0 ||
            height == 0) {
            return std::nullopt;
        }
        output.push_back(RectF{static_cast<float>(x), static_cast<float>(y),
                               static_cast<float>(width), static_cast<float>(height)});
        if (output.size() > 32) {
            return std::nullopt;
        }
        if (separator == std::string_view::npos) {
            break;
        }
        field.remove_prefix(separator + 1);
    }
    return output;
}

[[nodiscard]] bool overlaps(const RectF& left, const RectF& right) noexcept {
    return left.x < right.right() && left.right() > right.x && left.y < right.bottom() &&
           left.bottom() > right.y;
}

[[nodiscard]] bool contained_by(const RectF& inner, const RectF& outer) noexcept {
    return inner.finite_positive() && inner.x >= outer.x && inner.y >= outer.y &&
           inner.right() <= outer.right() && inner.bottom() <= outer.bottom();
}

} // namespace

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    const HANDLE input = GetStdHandle(STD_INPUT_HANDLE);
    const HANDLE output = GetStdHandle(STD_OUTPUT_HANDLE);
    const auto expected_nonce_text = environment(L"NPC_SUBTITLE_LAUNCH_NONCE");
    if (!input || input == INVALID_HANDLE_VALUE || !output || output == INVALID_HANDLE_VALUE ||
        !expected_nonce_text) {
        return 10;
    }
    const auto expected_nonce = decode_hex(*expected_nonce_text, presentation_nonce_bytes);
    if (!expected_nonce || expected_nonce->size() != presentation_nonce_bytes) {
        return 11;
    }

    const HRESULT com = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(com)) {
        return 12;
    }
    ComPtr<IDWriteFactory> write_factory;
    if (FAILED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED, __uuidof(IDWriteFactory),
                                   &write_factory))) {
        CoUninitialize();
        return 13;
    }
    DirectWriteD2DBackend text_backend(write_factory.Get());
    WindowsSubtitleSurface surface;
    std::unique_ptr<SubtitlePresentationService> service;
    PresentationSessionBinding binding;
    std::string line;
    while (read_line(input, line)) {
        const auto part = fields(line);
        if (part.empty()) {
            continue;
        }
        if (part[0] == "HELLO") {
            if (service || part.size() != 61 || part[1] != "1") {
                write_line(output, error_line(0, "invalid_hello"));
                break;
            }
            std::uint64_t sequence{}, deadline{}, generation{}, creation{};
            std::uint32_t pid{};
            const auto nonce = decode_hex(part[5], presentation_nonce_bytes);
            const auto session = decode_text(part[6], 128);
            const auto executable = decode_text(part[9], 260);
            const auto authority_sources = decode_text(part[12], 16U * 1024U);
            const auto style_id = decode_text(part[13], 128);
            std::uint32_t outline_enabled{}, shadow_enabled{}, backplate_enabled{};
            if (!number(part[2], sequence) || !number(part[3], deadline) ||
                !number(part[4], generation) || !number(part[7], pid) ||
                !number(part[8], creation) || sequence != 1 ||
                deadline <= qpc_now() || !nonce || !session || !executable ||
                *nonce != *expected_nonce || !target_process_matches(pid, *executable) ||
                !number(part[10], binding.renderer_authority.revision) ||
                part[11].size() != 64 || !authority_sources || !style_id ||
                !floating(part[14], binding.renderer_authority.safe_area_dp) ||
                !floating(part[15], binding.renderer_authority.text_scale) ||
                !floating(part[16], binding.renderer_authority.body_size_dp) ||
                !floating(part[17], binding.renderer_authority.speaker_size_dp) ||
                !floating(part[18], binding.renderer_authority.line_height) ||
                !number(part[19], binding.renderer_authority.max_body_lines) ||
                !floating(part[20], binding.renderer_authority.max_width_fraction) ||
                !floating(part[21], binding.renderer_authority.min_width_dp) ||
                !floating(part[22], binding.renderer_authority.max_width_dp) ||
                !floating(part[23], binding.renderer_authority.padding_x_dp) ||
                !floating(part[24], binding.renderer_authority.padding_y_dp) ||
                !floating(part[25], binding.renderer_authority.speaker_gap_dp) ||
                !floating(part[26], binding.renderer_authority.fallback_bottom_dp) ||
                !floating(part[27], binding.renderer_authority.corner_radius_dp) ||
                !floating(part[28], binding.renderer_authority.body.r) ||
                !floating(part[29], binding.renderer_authority.body.g) ||
                !floating(part[30], binding.renderer_authority.body.b) ||
                !floating(part[31], binding.renderer_authority.body.a) ||
                !floating(part[32], binding.renderer_authority.speaker.r) ||
                !floating(part[33], binding.renderer_authority.speaker.g) ||
                !floating(part[34], binding.renderer_authority.speaker.b) ||
                !floating(part[35], binding.renderer_authority.speaker.a) ||
                !number(part[36], outline_enabled) || outline_enabled > 1 ||
                !floating(part[37], binding.renderer_authority.outline.width_px) ||
                !floating(part[38], binding.renderer_authority.outline.color.r) ||
                !floating(part[39], binding.renderer_authority.outline.color.g) ||
                !floating(part[40], binding.renderer_authority.outline.color.b) ||
                !floating(part[41], binding.renderer_authority.outline.color.a) ||
                !number(part[42], shadow_enabled) || shadow_enabled > 1 ||
                !floating(part[43], binding.renderer_authority.shadow.offset_x_px) ||
                !floating(part[44], binding.renderer_authority.shadow.offset_y_px) ||
                !floating(part[45], binding.renderer_authority.shadow.blur_radius_px) ||
                !floating(part[46], binding.renderer_authority.shadow.color.r) ||
                !floating(part[47], binding.renderer_authority.shadow.color.g) ||
                !floating(part[48], binding.renderer_authority.shadow.color.b) ||
                !floating(part[49], binding.renderer_authority.shadow.color.a) ||
                !number(part[50], backplate_enabled) || backplate_enabled > 1 ||
                !floating(part[51], binding.renderer_authority.backplate.border_width_px) ||
                !floating(part[52], binding.renderer_authority.backplate.fill.r) ||
                !floating(part[53], binding.renderer_authority.backplate.fill.g) ||
                !floating(part[54], binding.renderer_authority.backplate.fill.b) ||
                !floating(part[55], binding.renderer_authority.backplate.fill.a) ||
                !floating(part[56], binding.renderer_authority.backplate.border.r) ||
                !floating(part[57], binding.renderer_authority.backplate.border.g) ||
                !floating(part[58], binding.renderer_authority.backplate.border.b) ||
                !floating(part[59], binding.renderer_authority.backplate.border.a) ||
                !floating(part[60], binding.renderer_authority.opacity)) {
                write_line(output, error_line(sequence, "authentication_failed"));
                break;
            }
            const auto observed_creation = process_creation_time(pid);
            if (!observed_creation || *observed_creation != creation) {
                write_line(output, error_line(sequence, "authentication_failed"));
                break;
            }
            std::copy(nonce->begin(), nonce->end(), binding.launch_nonce.begin());
            binding.session_id = *session;
            binding.client_process_id = pid;
            binding.client_process_creation_time = creation;
            binding.client_executable_name = *executable;
            binding.renderer_authority.sha256 = std::string(part[11]);
            binding.renderer_authority.sources_json = *authority_sources;
            binding.renderer_authority.style_id = *style_id;
            binding.renderer_authority.outline.enabled = outline_enabled != 0;
            binding.renderer_authority.shadow.enabled = shadow_enabled != 0;
            binding.renderer_authority.backplate.enabled = backplate_enabled != 0;
            service = std::make_unique<SubtitlePresentationService>(binding, surface);
            if (generation != 0) {
                service->cancel(generation);
            }
            write_line(output, "HELLO_OK\t1");
            continue;
        }
        if (!service || part[0] != "PRESENT" || part.size() != 30 || part[1] != "1") {
            write_line(output, error_line(0, "invalid_command"));
            break;
        }
        std::uint64_t sequence{}, deadline{}, generation{}, sentence_id{}, presentation_id{};
        std::uint64_t target_hwnd{}, geometry_epoch{}, capture_sequence{}, graphics_generation{};
        std::uint64_t renderer_authority_revision{};
        std::uint32_t target_pid{}, viewport_width{}, viewport_height{}, dpi_x{}, dpi_y{};
        std::int32_t viewport_x{}, viewport_y{};
        float white_nits{};
        const auto turn = decode_text(part[5], 128);
        const auto target_exe = decode_text(part[11], 260);
        const auto hud_exclusions = exclusions(part[24]);
        const auto body = decode_text(part[25], 16U * 1024U);
        const auto speaker = decode_text(part[26], 1024);
        const auto locale = decode_text(part[27], 64);
        const bool console = part[8] == "console";
        const bool color_valid = part[19] == "unknown" || part[19] == "sdr" ||
                                 part[19] == "sdrscrgb" || part[19] == "hdr10" ||
                                 part[19] == "hdrscrgb";
        const bool direction_valid = part[18] == "ltr" || part[18] == "rtl";
        if (!number(part[2], sequence) || !number(part[3], deadline) ||
            !number(part[4], generation) || !number(part[6], sentence_id) ||
            !number(part[7], presentation_id) || !number(part[9], target_pid) ||
            !number(part[10], target_hwnd) || !number(part[12], viewport_x) ||
            !number(part[13], viewport_y) || !number(part[14], viewport_width) ||
            !number(part[15], viewport_height) || !number(part[16], dpi_x) ||
            !number(part[17], dpi_y) || !floating(part[20], white_nits) ||
            !number(part[21], geometry_epoch) || !number(part[22], capture_sequence) ||
            !number(part[23], graphics_generation) || !turn || !target_exe || !hud_exclusions || !body ||
            !speaker || !locale || deadline <= qpc_now() || sentence_id == 0 ||
            presentation_id == 0 || (!console && part[8] != "native") || !color_valid ||
            !direction_valid || !number(part[28], renderer_authority_revision) ||
            part[29].size() != 64) {
            write_line(output, error_line(sequence, "payload_invalid"));
            continue;
        }
        RectF viewport{static_cast<float>(viewport_x), static_cast<float>(viewport_y),
                       static_cast<float>(viewport_width), static_cast<float>(viewport_height)};
        if (console) {
            if (target_pid != 0 || target_hwnd != 0 || geometry_epoch != 0 ||
                capture_sequence != 0 || graphics_generation != 0 || viewport_width != 0 ||
                viewport_height != 0 || dpi_x != 0 || dpi_y != 0 ||
                !hud_exclusions->empty()) {
                write_line(output, error_line(sequence, "console_provenance_mismatch"));
                continue;
            }
            viewport = console_viewport();
            dpi_x = GetDpiForSystem();
            dpi_y = dpi_x;
            white_nits = 80.0F;
        } else if (!target_process_matches(target_pid, *target_exe) ||
                   !window_belongs_to_process(target_hwnd, target_pid) ||
                   !viewport.finite_positive() || dpi_x < 48 || dpi_y < 48 ||
                   !window_geometry_matches(target_hwnd, viewport, dpi_x, dpi_y) ||
                   !std::all_of(hud_exclusions->begin(), hud_exclusions->end(),
                                [&](const RectF& exclusion) {
                                    return contained_by(exclusion, viewport);
                                }) ||
                   geometry_epoch == 0 || capture_sequence == 0 || graphics_generation == 0) {
            write_line(output, error_line(sequence, "trusted_target_mismatch"));
            continue;
        }

        const auto& authority = binding.renderer_authority;
        const float scale = static_cast<float>(dpi_y) / 96.0F;
        const float usable_width = std::max(1.0F, viewport.width - 2.0F * authority.safe_area_dp * scale);
        const float preferred_width = viewport.width * authority.max_width_fraction;
        const float width = std::clamp(preferred_width,
                                       std::min(authority.min_width_dp * scale, usable_width),
                                       std::min(authority.max_width_dp * scale, usable_width));
        const float speaker_height = speaker->empty()
                                         ? 0.0F
                                         : authority.speaker_size_dp * authority.line_height * scale;
        const float body_height = authority.body_size_dp * authority.line_height *
                                  static_cast<float>(authority.max_body_lines) * scale;
        const float speaker_gap = speaker->empty() ? 0.0F : authority.speaker_gap_dp * scale;
        const float height = body_height + speaker_height + speaker_gap +
                             2.0F * authority.padding_y_dp * scale;
        const float safe_margin = authority.safe_area_dp * scale;
        float bottom_offset = authority.fallback_bottom_dp * scale;
        for (std::size_t attempt{}; attempt <= hud_exclusions->size(); ++attempt) {
            const RectF candidate{
                viewport.x + (viewport.width - width) * 0.5F,
                viewport.bottom() - safe_margin - bottom_offset - height,
                width,
                height,
            };
            const auto collision = std::find_if(hud_exclusions->begin(), hud_exclusions->end(),
                                                [&](const RectF& exclusion) {
                                                    return overlaps(candidate, exclusion);
                                                });
            if (collision == hud_exclusions->end()) {
                break;
            }
            bottom_offset = std::max(
                bottom_offset,
                viewport.bottom() - collision->y + authority.speaker_gap_dp * scale);
        }
        const RectF offscreen{viewport.x - viewport.width * 3.0F,
                              viewport.y - viewport.height * 3.0F, width, height};
        RenderRequest render;
        render.presentation_id = presentation_id;
        render.capture_sequence = capture_sequence;
        render.graphics_generation = graphics_generation;
        render.viewport_px = viewport;
        render.output_clip_px = viewport;
        render.fallback.safe_margin_px = safe_margin;
        render.fallback.bottom_offset_px = bottom_offset;
        render.layout.bounds_px = offscreen;
        render.layout.body_bounds_px = {
            offscreen.x + authority.padding_x_dp * scale,
            offscreen.y + authority.padding_y_dp * scale + speaker_height + speaker_gap,
            width - 2.0F * authority.padding_x_dp * scale,
            body_height};
        if (!speaker->empty()) {
            render.layout.speaker_bounds_px = RectF{
                offscreen.x + authority.padding_x_dp * scale,
                offscreen.y + authority.padding_y_dp * scale,
                width - 2.0F * authority.padding_x_dp * scale, speaker_height};
        }
        TextRasterRequest text;
        text.body_utf8 = *body;
        text.speaker_utf8 = speaker->empty() ? std::nullopt : std::optional{*speaker};
        text.locale = locale->empty() ? "und" : *locale;
        text.body_bounds_px = render.layout.body_bounds_px;
        text.speaker_bounds_px = render.layout.speaker_bounds_px;
        text.body_size_px = authority.body_size_dp * scale;
        text.speaker_size_px = authority.speaker_size_dp * scale;
        text.right_to_left = part[18] == "rtl";
        std::string shaping_error;
        if (!text_backend.resolve_text(text, render.glyph_runs, shaping_error)) {
            write_line(output, error_line(sequence, "shaping_failed"));
            continue;
        }

        SubtitlePresentationRequest request;
        request.auth.launch_nonce = binding.launch_nonce;
        request.auth.session_id = binding.session_id;
        request.auth.client_process_id = binding.client_process_id;
        request.auth.client_process_creation_time = binding.client_process_creation_time;
        request.auth.client_executable_name = binding.client_executable_name;
        request.auth.request_sequence = sequence;
        request.auth.deadline_qpc = deadline;
        request.turn_id = *turn;
        request.sentence_id = sentence_id;
        request.provenance = console ? PresentationProvenance::console_bottom_center_unavailable
                                     : PresentationProvenance::trusted_native_capture;
        request.cancellation_generation = generation;
        request.target_geometry_epoch = geometry_epoch;
        request.renderer_authority_revision = renderer_authority_revision;
        request.renderer_authority_sha256 = std::string(part[29]);
        request.dpi_x = dpi_x;
        request.dpi_y = dpi_y;
        request.direction = text.right_to_left ? ParagraphDirection::right_to_left
                                               : ParagraphDirection::left_to_right;
        request.bidi_shaping_applied = true;
        request.grapheme_clusters_preserved = true;
        request.target_color_space = part[19] == "hdr10" ? TargetColorSpace::hdr10_pq
                                     : part[19] == "hdrscrgb" ? TargetColorSpace::hdr_sc_rgb
                                     : part[19] == "sdrscrgb" ? TargetColorSpace::sdr_sc_rgb
                                     : part[19] == "unknown" ? TargetColorSpace::unknown
                                                               : TargetColorSpace::sdr_srgb;
        request.sdr_white_level_nits = white_nits;
        render.style.body = authority.body;
        render.style.speaker = authority.speaker;
        render.style.outline = authority.outline;
        render.style.outline.width_px *= scale;
        render.style.shadow = authority.shadow;
        render.style.shadow.offset_x_px *= scale;
        render.style.shadow.offset_y_px *= scale;
        render.style.shadow.blur_radius_px *= scale;
        render.style.backplate = authority.backplate;
        render.style.backplate.corner_radius_px = authority.corner_radius_dp * scale;
        render.style.backplate.border_width_px *= scale;
        render.style.global_alpha = authority.opacity;
        request.render = std::move(render);
        const auto result = service->present(request, qpc_now());
        if (!result || !result.receipt) {
            write_line(output, error_line(sequence, "presentation_failed"));
            continue;
        }
        const auto& receipt = *result.receipt;
        std::ostringstream response;
        response << "OK\t" << sequence << '\t' << encode_hex(receipt.receipt_id) << '\t'
                 << receipt.sentence_id << '\t' << receipt.presentation_id << '\t'
                 << (console ? "console" : "native") << '\t' << receipt.target_geometry_epoch
                 << '\t' << receipt.capture_sequence << '\t' << receipt.graphics_generation
                 << '\t' << std::hex << std::setw(16) << std::setfill('0') << receipt.layer_hash
                 << std::dec << '\t' << receipt.presented_qpc << '\t' << receipt.desktop_x_px
                 << '\t' << receipt.desktop_y_px << '\t' << receipt.width_px << '\t'
                 << receipt.height_px << '\t' << receipt.dpi_x << '\t' << receipt.dpi_y << '\t'
                 << (receipt.direction == ParagraphDirection::right_to_left ? "rtl" : "ltr")
                 << "\t1\t1\t" << (receipt.used_bottom_center_fallback ? 1 : 0) << '\t'
                 << static_cast<unsigned>(receipt.color_treatment) << "\t1\t"
                 << receipt.renderer_authority_revision << '\t'
                 << receipt.renderer_authority_sha256 << '\t'
                 << encode_hex(receipt.renderer_authority_sources_json) << '\t'
                 << encode_hex(receipt.renderer_style_id) << '\t'
                 << receipt.renderer_safe_area_dp << '\t'
                 << receipt.renderer_text_scale << '\t'
                 << (receipt.renderer_backplate_enabled ? 1 : 0) << '\t'
                 << receipt.renderer_opacity;
        write_line(output, response.str());
        pump_messages();
    }
    if (service) {
        service->shutdown();
    }
    surface.shutdown();
    CoUninitialize();
    return 0;
}

#endif
