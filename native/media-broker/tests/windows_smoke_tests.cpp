#ifdef _WIN32

#include "npc/media_broker/geometry.hpp"
#include "npc/media_broker/ipc.hpp"
#include "windows_audio.hpp"
#include "windows_capture.hpp"
#include "windows_overlay.hpp"
#include "windows_service.hpp"

#include <Windows.h>
#include <d3d11.h>
#include <wrl/client.h>

#include <chrono>
#include <cstddef>
#include <cstdlib>
#include <iostream>
#include <filesystem>
#include <span>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;
using namespace npc::media;
using namespace npc::media::windows;

COLORREF target_color = RGB(18, 92, 112);

LRESULT CALLBACK test_window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_PAINT) {
        PAINTSTRUCT paint{};
        const HDC dc = BeginPaint(window, &paint);
        const HBRUSH brush = CreateSolidBrush(target_color);
        FillRect(dc, &paint.rcPaint, brush);
        DeleteObject(brush);
        EndPaint(window, &paint);
        return 0;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

[[nodiscard]] HWND create_test_window() {
    WNDCLASSEXW definition{};
    definition.cbSize = sizeof(definition);
    definition.lpfnWndProc = test_window_proc;
    definition.hInstance = GetModuleHandleW(nullptr);
    definition.lpszClassName = L"NpcMediaBrokerWgcSmokeTarget";
    definition.hbrBackground = reinterpret_cast<HBRUSH>(COLOR_WINDOW + 1);
    RegisterClassExW(&definition);
    const HWND window = CreateWindowExW(0, definition.lpszClassName, L"WGC smoke target",
                                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                                        100, 100, 640, 360, nullptr, nullptr,
                                        definition.hInstance, nullptr);
    UpdateWindow(window);
    return window;
}

void pump_messages() {
    MSG message{};
    while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

[[nodiscard]] bool pipe_write(HANDLE pipe, std::span<const std::byte> bytes) {
    std::size_t offset{};
    while (offset < bytes.size()) {
        DWORD written{};
        if (!WriteFile(pipe, bytes.data() + offset, static_cast<DWORD>(bytes.size() - offset), &written, nullptr)) return false;
        offset += written;
    }
    return true;
}

[[nodiscard]] bool pipe_read(HANDLE pipe, std::span<std::byte> bytes) {
    std::size_t offset{};
    while (offset < bytes.size()) {
        DWORD read{};
        if (!ReadFile(pipe, bytes.data() + offset, static_cast<DWORD>(bytes.size() - offset), &read, nullptr)) return false;
        offset += read;
    }
    return true;
}

[[nodiscard]] std::optional<ipc::Response> transact(HANDLE pipe, const ipc::Envelope& envelope) {
    const auto encoded = ipc::encode_envelope(envelope);
    if (!encoded) return std::nullopt;
    const auto framed = ipc::frame_message(*encoded);
    if (!pipe_write(pipe, framed)) return std::nullopt;
    std::array<std::byte, 4> prefix{};
    if (!pipe_read(pipe, prefix)) return std::nullopt;
    const auto size = ipc::decode_frame_size(prefix);
    if (!size) return std::nullopt;
    std::vector<std::byte> response(*size);
    return pipe_read(pipe, response) ? ipc::decode_response(response) : std::nullopt;
}

template <typename T>
[[nodiscard]] std::optional<T> read_little(const std::span<const std::byte> payload,
                                           const std::size_t offset) {
    static_assert(std::is_unsigned_v<T>);
    if (offset > payload.size() || payload.size() - offset < sizeof(T)) return std::nullopt;
    T value{};
    for (std::size_t index = 0; index < sizeof(T); ++index) {
        value |= static_cast<T>(std::to_integer<std::uint8_t>(payload[offset + index])) << (index * 8U);
    }
    return value;
}

struct CaptureDiagnosticsEvidence {
    BrokerState state{BrokerState::stopped};
    CaptureBackend capture_backend{CaptureBackend::none};
    TargetState target_state{TargetState::none};
    std::uint64_t frames_received{};
    std::uint64_t frames_presented{};
};

[[nodiscard]] std::optional<CaptureDiagnosticsEvidence> decode_capture_evidence(
    const std::span<const std::byte> payload) {
    const auto state = read_little<std::uint32_t>(payload, 0);
    const auto capture_backend = read_little<std::uint32_t>(payload, 4);
    const auto target_state = read_little<std::uint32_t>(payload, 20);
    const auto frames_received = read_little<std::uint64_t>(payload, 48);
    const auto frames_presented = read_little<std::uint64_t>(payload, 56);
    if (!state || !capture_backend || !target_state || !frames_received || !frames_presented ||
        *state > static_cast<std::uint32_t>(BrokerState::failed) ||
        *capture_backend > static_cast<std::uint32_t>(CaptureBackend::desktop_duplication) ||
        *target_state > static_cast<std::uint32_t>(TargetState::closed)) {
        return std::nullopt;
    }
    return CaptureDiagnosticsEvidence{
        static_cast<BrokerState>(*state),
        static_cast<CaptureBackend>(*capture_backend),
        static_cast<TargetState>(*target_state),
        *frames_received,
        *frames_presented,
    };
}

struct ChildService {
    PROCESS_INFORMATION process{};
    HANDLE job{};
    HANDLE pipe{INVALID_HANDLE_VALUE};
    std::string session;
    std::array<std::byte, ipc::launch_nonce_bytes> nonce{};
};

[[nodiscard]] std::optional<ChildService> launch_service(const std::wstring& executable,
                                                         const std::uint32_t suffix) {
    ChildService child;
    child.session = "service-smoke-" + std::to_string(GetCurrentProcessId()) + "-" + std::to_string(suffix);
    std::string nonce_hex;
    constexpr char hex[] = "0123456789abcdef";
    for (std::size_t index = 0; index < child.nonce.size(); ++index) {
        const auto value = static_cast<std::uint8_t>((index + suffix) & 0xffU);
        child.nonce[index] = static_cast<std::byte>(value);
        nonce_hex.push_back(hex[value >> 4U]);
        nonce_hex.push_back(hex[value & 0x0fU]);
    }
    std::wstring command = L"\"" + executable + L"\" --parent-pid=" + std::to_wstring(GetCurrentProcessId()) +
                           L" --session=" + std::wstring(child.session.begin(), child.session.end()) +
                           L" --nonce=" + std::wstring(nonce_hex.begin(), nonce_hex.end());
    STARTUPINFOW startup{sizeof(startup)};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE,
                        CREATE_SUSPENDED | CREATE_NO_WINDOW, nullptr, nullptr, &startup, &child.process)) return std::nullopt;
    child.job = CreateJobObjectW(nullptr, nullptr);
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (!child.job || !SetInformationJobObject(child.job, JobObjectExtendedLimitInformation, &limits, sizeof(limits)) ||
        !AssignProcessToJobObject(child.job, child.process.hProcess)) {
        TerminateProcess(child.process.hProcess, 1);
        return std::nullopt;
    }
    ResumeThread(child.process.hThread);
    CloseHandle(child.process.hThread);
    child.process.hThread = nullptr;
    const std::wstring pipe_name = L"\\\\.\\pipe\\npc-media-broker-" +
                                   std::wstring(child.session.begin(), child.session.end());
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(8);
    DWORD last_pipe_error{};
    while (std::chrono::steady_clock::now() < deadline) {
        child.pipe = CreateFileW(pipe_name.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                                 OPEN_EXISTING, 0, nullptr);
        if (child.pipe != INVALID_HANDLE_VALUE) return child;
        last_pipe_error = GetLastError();
        if (WaitForSingleObject(child.process.hProcess, 0) == WAIT_OBJECT_0) {
            DWORD exit_code{};
            GetExitCodeProcess(child.process.hProcess, &exit_code);
            std::cerr << "media service exited before control-pipe connection: exit=" << exit_code
                      << " pipe_error=" << last_pipe_error << '\n';
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    if (WaitForSingleObject(child.process.hProcess, 0) != WAIT_OBJECT_0) {
        std::cerr << "media service control-pipe connection timed out: pipe_error="
                  << last_pipe_error << '\n';
    }
    TerminateProcess(child.process.hProcess, 1);
    return std::nullopt;
}

void close_child(ChildService& child) {
    if (child.pipe != INVALID_HANDLE_VALUE) CloseHandle(child.pipe);
    if (child.process.hProcess) CloseHandle(child.process.hProcess);
    if (child.job) CloseHandle(child.job);
    child = {};
}

[[nodiscard]] bool service_lifecycle_smoke(const std::wstring& executable, const HWND target_window) {
    {
        STARTUPINFOW startup{sizeof(startup)};
        PROCESS_INFORMATION malformed{};
        std::wstring command = L"\"" + executable + L"\"";
        if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE,
                            CREATE_NO_WINDOW, nullptr, nullptr, &startup, &malformed)) return false;
        CloseHandle(malformed.hThread);
        const bool rejected = WaitForSingleObject(malformed.hProcess, 5000) == WAIT_OBJECT_0;
        DWORD exit_code{};
        GetExitCodeProcess(malformed.hProcess, &exit_code);
        CloseHandle(malformed.hProcess);
        if (!rejected || exit_code == 0) {
            std::cerr << "malformed service launch was not rejected: waited=" << rejected
                      << " exit=" << exit_code << '\n';
            return false;
        }
    }

    auto child_value = launch_service(executable, 1);
    if (!child_value) {
        std::cerr << "authenticated service launch failed before health exchange\n";
        return false;
    }
    auto child = std::move(*child_value);
    const auto fail_child = [&child](const std::string_view stage) {
        std::cerr << "authenticated service protocol failed at " << stage << '\n';
        close_child(child);
        return false;
    };
    const auto frequency = qpc_frequency();
    const auto now = qpc_now();
    const auto empty = ipc::encode_command(ipc::CommandKind::health, ipc::HealthCommand{});
    ipc::Envelope request{ipc::protocol_version, child.nonce, child.session, 1,
                          now + frequency * 2, 0, ipc::CommandKind::health, *empty};
    request.nonce[0] ^= std::byte{0xff};
    const auto rejected = transact(child.pipe, request);
    if (!rejected || rejected->status != ipc::StatusCode::authentication_failed) {
        return fail_child("nonce rejection");
    }
    request.nonce = child.nonce;
    request.sequence = 2;
    request.deadline_qpc = qpc_now() + frequency * 2;
    const auto health = transact(child.pipe, request);
    if (!health || health->status != ipc::StatusCode::ok) return fail_child("authenticated health");

    const auto inspected = inspect_target(target_window, GetCurrentProcessId());
    if (!inspected.valid_window || !inspected.process_id_matches ||
        !inspected.inspection_complete || inspected.process_name.empty()) {
        std::cerr << "synthetic target preflight incomplete: valid=" << inspected.valid_window
                  << " pid_match=" << inspected.process_id_matches
                  << " inspection_complete=" << inspected.inspection_complete
                  << " process_name_empty=" << inspected.process_name.empty() << '\n';
        return fail_child("synthetic target preflight");
    }
    const auto select_payload = ipc::encode_command(
        ipc::CommandKind::select_target,
        ipc::SelectTargetCommand{reinterpret_cast<std::uintptr_t>(target_window),
                                 GetCurrentProcessId(),
                                 {inspected.process_name}});
    if (!select_payload) return fail_child("SelectTarget encoding");
    request.sequence = 3;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::select_target;
    request.payload = *select_payload;
    const auto selected = transact(child.pipe, request);
    if (!selected || selected->status != ipc::StatusCode::ok ||
        selected->cancellation_generation <= request.cancellation_generation) {
        if (selected) {
            std::cerr << "SelectTarget response: status=" << ipc::to_string(selected->status)
                      << " request_generation=" << request.cancellation_generation
                      << " response_generation=" << selected->cancellation_generation;
            if (const auto reason = read_little<std::uint32_t>(selected->payload, 0)) {
                std::cerr << " block_reason=" << *reason;
            }
            std::cerr << '\n';
        }
        return fail_child("SelectTarget response");
    }
    request.cancellation_generation = selected->cancellation_generation;

    const auto diagnostics_payload = ipc::encode_command(ipc::CommandKind::diagnostics,
                                                         ipc::DiagnosticsCommand{});
    std::optional<CaptureDiagnosticsEvidence> capture_evidence;
    const auto capture_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(8);
    while (std::chrono::steady_clock::now() < capture_deadline) {
        target_color = target_color == RGB(18, 92, 112) ? RGB(116, 34, 72) : RGB(18, 92, 112);
        InvalidateRect(target_window, nullptr, FALSE);
        UpdateWindow(target_window);
        pump_messages();
        ++request.sequence;
        request.deadline_qpc = qpc_now() + frequency * 2;
        request.command = ipc::CommandKind::diagnostics;
        request.payload = *diagnostics_payload;
        const auto diagnostics = transact(child.pipe, request);
        if (!diagnostics || diagnostics->status != ipc::StatusCode::ok) {
            return fail_child("capture diagnostics exchange");
        }
        capture_evidence = decode_capture_evidence(diagnostics->payload);
        if (capture_evidence && capture_evidence->state == BrokerState::capturing_primary &&
            capture_evidence->capture_backend == CaptureBackend::windows_graphics_capture &&
            capture_evidence->target_state == TargetState::selected &&
            capture_evidence->frames_received >= 2 && capture_evidence->frames_presented >= 1) {
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    if (!capture_evidence || capture_evidence->state != BrokerState::capturing_primary ||
        capture_evidence->capture_backend != CaptureBackend::windows_graphics_capture ||
        capture_evidence->target_state != TargetState::selected ||
        capture_evidence->frames_received < 2 || capture_evidence->frames_presented < 1) {
        if (capture_evidence) {
            std::cerr << "capture evidence timeout: state="
                      << static_cast<std::uint32_t>(capture_evidence->state)
                      << " backend=" << static_cast<std::uint32_t>(capture_evidence->capture_backend)
                      << " target=" << static_cast<std::uint32_t>(capture_evidence->target_state)
                      << " received=" << capture_evidence->frames_received
                      << " presented=" << capture_evidence->frames_presented << '\n';
        }
        return fail_child("primary WGC frame evidence");
    }

    const auto clear_payload = ipc::encode_command(ipc::CommandKind::clear_target,
                                                   ipc::ClearTargetCommand{});
    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::clear_target;
    request.payload = *clear_payload;
    const auto cleared = transact(child.pipe, request);
    if (!cleared || cleared->status != ipc::StatusCode::ok ||
        cleared->cancellation_generation <= request.cancellation_generation) {
        return fail_child("ClearTarget response");
    }
    request.cancellation_generation = cleared->cancellation_generation;

    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::diagnostics;
    request.payload = *diagnostics_payload;
    const auto after_clear = transact(child.pipe, request);
    const auto cleared_evidence = after_clear ? decode_capture_evidence(after_clear->payload) : std::nullopt;
    if (!after_clear || after_clear->status != ipc::StatusCode::ok || !cleared_evidence ||
        cleared_evidence->state != BrokerState::awaiting_target ||
        cleared_evidence->capture_backend != CaptureBackend::none ||
        cleared_evidence->target_state != TargetState::none ||
        cleared_evidence->frames_received != capture_evidence->frames_received) {
        return fail_child("post-ClearTarget diagnostics");
    }

    const auto shutdown_payload = ipc::encode_command(ipc::CommandKind::shutdown, ipc::ShutdownCommand{});
    ++request.sequence;
    request.deadline_qpc = qpc_now() + frequency * 2;
    request.command = ipc::CommandKind::shutdown;
    request.payload = *shutdown_payload;
    const auto shutdown = transact(child.pipe, request);
    const bool exited = shutdown && shutdown->status == ipc::StatusCode::ok &&
                        WaitForSingleObject(child.process.hProcess, 5000) == WAIT_OBJECT_0;
    close_child(child);
    if (!exited) {
        std::cerr << "authenticated service did not exit after Shutdown\n";
        return false;
    }
    std::cout << "authenticated SelectTarget observed " << capture_evidence->frames_received
              << " WGC frames (" << capture_evidence->frames_presented
              << " presented); ClearTarget stopped capture\n";

    auto disconnect_value = launch_service(executable, 2);
    if (!disconnect_value) {
        std::cerr << "disconnect service launch failed\n";
        return false;
    }
    auto disconnect = std::move(*disconnect_value);
    CloseHandle(disconnect.pipe);
    disconnect.pipe = INVALID_HANDLE_VALUE;
    const bool disconnected_exit = WaitForSingleObject(disconnect.process.hProcess, 5000) == WAIT_OBJECT_0;
    close_child(disconnect);
    if (!disconnected_exit) std::cerr << "service did not exit after control-pipe disconnect\n";
    return disconnected_exit;
}

[[nodiscard]] TargetGeometry geometry_for(HWND window, SizeI captured_size) {
    RECT client{};
    GetClientRect(window, &client);
    POINT origin{};
    ClientToScreen(window, &origin);
    const RectI client_bounds{origin.x, origin.y,
                              origin.x + client.right, origin.y + client.bottom};
    const HMONITOR monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
    MONITORINFO info{sizeof(info)};
    GetMonitorInfoW(monitor, &info);
    const UINT dpi = GetDpiForWindow(window);
    return {
        client_bounds,
        client_bounds,
        client_bounds,
        captured_size,
        {"smoke-monitor",
         {info.rcMonitor.left, info.rcMonitor.top, info.rcMonitor.right, info.rcMonitor.bottom},
         {info.rcWork.left, info.rcWork.top, info.rcWork.right, info.rcWork.bottom},
         dpi == 0 ? 96U : dpi,
         dpi == 0 ? 96U : dpi,
         ColorSpace::sdr_srgb,
         DisplayRotation::identity,
         80.0},
        false,
    };
}

} // namespace

int main(const int argc, char** argv) {
    if (argc == 2 && std::string_view{argv[1]} == "--service-only") {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        const HWND target_window = create_test_window();
        if (!target_window) {
            std::cerr << "failed to create service protocol WGC target window\n";
            return EXIT_FAILURE;
        }
        wchar_t test_path[32768]{};
        if (GetModuleFileNameW(nullptr, test_path, ARRAYSIZE(test_path)) == 0) {
            DestroyWindow(target_window);
            return EXIT_FAILURE;
        }
        const auto service_path =
            (std::filesystem::path(test_path).parent_path() / L"npc-media-broker.exe").wstring();
        const bool passed = service_lifecycle_smoke(service_path, target_window);
        DestroyWindow(target_window);
        if (!passed) {
            std::cerr << "authenticated named-pipe service lifecycle smoke failed\n";
            return EXIT_FAILURE;
        }
        std::cout << "authenticated named-pipe service lifecycle smoke passed\n";
        return EXIT_SUCCESS;
    }
    if (argc != 1) {
        std::cerr << "unsupported smoke-test argument\n";
        return EXIT_FAILURE;
    }
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    const HWND window = create_test_window();
    if (!window) {
        std::cerr << "failed to create WGC smoke target window\n";
        return EXIT_FAILURE;
    }
    const auto inspected = inspect_target(window, GetCurrentProcessId());
    const auto local_policy = evaluate_target_policy(inspected, {inspected.process_name});
    if (!local_policy.capture_allowed) {
        std::cerr << "same-user/session HWND inspection was unexpectedly blocked: "
                  << to_string(local_policy.reason) << '\n';
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const auto mismatched = inspect_target(window, GetCurrentProcessId() + 1);
    if (evaluate_target_policy(mismatched, {mismatched.process_name}).reason != TargetBlockReason::process_mismatch) {
        std::cerr << "HWND/PID mismatch did not fail closed\n";
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    D3D_FEATURE_LEVEL selected{};
    const D3D_FEATURE_LEVEL levels[]{D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0};
    HRESULT hr = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr,
                                   D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                   levels, ARRAYSIZE(levels), D3D11_SDK_VERSION,
                                   &device, &selected, &context);
    if (FAILED(hr)) {
        std::cerr << "failed to create D3D11 smoke device\n";
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    Failure failure;
    GraphicsCapture capture;
    if (!capture.start(window, device.Get(), failure)) {
        std::cerr << failure.message << '\n';
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    std::optional<OwnedCaptureFrame> captured;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (!captured && std::chrono::steady_clock::now() < deadline) {
        pump_messages();
        captured = capture.take_latest();
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!captured || !captured->texture || !captured->size_px.valid()) {
        std::cerr << "WGC free-threaded pool produced no frame\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    const auto initial_size = captured->size_px;
    const auto initial_sequence = captured->sequence;
    SetWindowPos(window, nullptr, 100, 100, 800, 450, SWP_NOACTIVATE | SWP_NOZORDER);
    InvalidateRect(window, nullptr, TRUE);
    std::optional<OwnedCaptureFrame> resized;
    const auto resize_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (std::chrono::steady_clock::now() < resize_deadline) {
        pump_messages();
        if (auto candidate = capture.take_latest();
            candidate && candidate->size_px.valid() && candidate->size_px != initial_size) {
            resized = std::move(candidate);
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!resized || resized->sequence <= initial_sequence) {
        std::cerr << "WGC frame pool did not recover with an advancing frame sequence\n";
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    captured = std::move(resized);

    const auto target = geometry_for(window, captured->size_px);
    const auto mapped = calculate_overlay_geometry(target);
    ResidualOverlay overlay;
    if (!mapped || !overlay.start(device.Get(), *mapped, failure)) {
        std::cerr << (mapped ? failure.message : "overlay geometry failed") << '\n';
        capture.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }

    constexpr UINT residual_width = 96;
    constexpr UINT residual_height = 48;
    std::vector<std::uint32_t> premultiplied(residual_width * residual_height, 0x80463010U);
    D3D11_TEXTURE2D_DESC texture_description{};
    texture_description.Width = residual_width;
    texture_description.Height = residual_height;
    texture_description.MipLevels = 1;
    texture_description.ArraySize = 1;
    texture_description.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    texture_description.SampleDesc.Count = 1;
    texture_description.Usage = D3D11_USAGE_IMMUTABLE;
    texture_description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    const D3D11_SUBRESOURCE_DATA texture_data{premultiplied.data(), residual_width * 4, 0};
    ComPtr<ID3D11Texture2D> residual;
    hr = device->CreateTexture2D(&texture_description, &texture_data, &residual);
    const RectI patch{mapped->clipped_desktop_bounds_px.left + 20,
                      mapped->clipped_desktop_bounds_px.top + 20,
                      mapped->clipped_desktop_bounds_px.left + 20 + static_cast<int>(residual_width),
                      mapped->clipped_desktop_bounds_px.top + 20 + static_cast<int>(residual_height)};
    if (FAILED(hr) || !overlay.present(residual.Get(), patch, failure)) {
        std::cerr << (FAILED(hr) ? "residual texture creation failed" : failure.message) << '\n';
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    overlay.hide();

    EventDrivenAudio audio;
    if (!audio.start(failure) || !audio.capture_ring() || !audio.render_ring()) {
        std::cerr << failure.message << '\n';
        audio.stop();
        capture.stop();
        overlay.stop();
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
    audio.stop();
    capture.stop();
    overlay.stop();
    wchar_t test_path[32768]{};
    if (GetModuleFileNameW(nullptr, test_path, ARRAYSIZE(test_path)) == 0) {
        DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const auto service_path = (std::filesystem::path(test_path).parent_path() / L"npc-media-broker.exe").wstring();
    const bool service_passed = service_lifecycle_smoke(service_path, window);
    DestroyWindow(window);
    if (!service_passed) {
        std::cerr << "authenticated named-pipe service lifecycle smoke failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "WGC frame, premultiplied DirectComposition overlay, and event-driven WASAPI smoke passed\n";
    return EXIT_SUCCESS;
}

#endif
