#ifdef _WIN32

#include "npc/media_broker/ipc.hpp"
#include "npc/media_broker/target_policy.hpp"
#include "windows_service.hpp"

#include <Windows.h>
#include <bcrypt.h>

#include <array>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <thread>
#include <type_traits>
#include <vector>

namespace {

using namespace npc::media;
using namespace npc::media::windows;

COLORREF target_color = RGB(18, 92, 112);

LRESULT CALLBACK target_window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
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

[[nodiscard]] HWND create_target_window() {
    WNDCLASSEXW definition{};
    definition.cbSize = sizeof(definition);
    definition.lpfnWndProc = target_window_proc;
    definition.hInstance = GetModuleHandleW(nullptr);
    definition.lpszClassName = L"NpcIdentityWgcSmokeTarget";
    RegisterClassExW(&definition);
    POINT origin{100, 100};
    EnumDisplayMonitors(nullptr, nullptr,
        [](const HMONITOR monitor, HDC, LPRECT, const LPARAM data) -> BOOL {
            MONITORINFO info{sizeof(info)};
            if (GetMonitorInfoW(monitor, &info) &&
                (info.dwFlags & MONITORINFOF_PRIMARY) == 0) {
                auto* selected = reinterpret_cast<POINT*>(data);
                selected->x = info.rcWork.left + 50;
                selected->y = info.rcWork.top + 50;
                return FALSE;
            }
            return TRUE;
        }, reinterpret_cast<LPARAM>(&origin));
    const HWND window = CreateWindowExW(
        WS_EX_NOACTIVATE, definition.lpszClassName, L"Identity mapping smoke target",
        WS_OVERLAPPEDWINDOW | WS_VISIBLE, origin.x, origin.y, 640, 360, nullptr, nullptr,
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

[[nodiscard]] bool pipe_write(const HANDLE pipe, const std::span<const std::byte> bytes) {
    std::size_t offset{};
    while (offset < bytes.size()) {
        DWORD written{};
        if (!WriteFile(pipe, bytes.data() + offset,
                       static_cast<DWORD>(bytes.size() - offset), &written, nullptr) ||
            written == 0U) return false;
        offset += written;
    }
    return true;
}

[[nodiscard]] bool pipe_read(const HANDLE pipe, const std::span<std::byte> bytes) {
    std::size_t offset{};
    while (offset < bytes.size()) {
        DWORD read{};
        if (!ReadFile(pipe, bytes.data() + offset,
                      static_cast<DWORD>(bytes.size() - offset), &read, nullptr) || read == 0U) {
            return false;
        }
        offset += read;
    }
    return true;
}

template <typename T>
[[nodiscard]] std::optional<T> read_little(const std::span<const std::byte> bytes,
                                           const std::size_t offset) {
    static_assert(std::is_unsigned_v<T>);
    if (offset > bytes.size() || bytes.size() - offset < sizeof(T)) return std::nullopt;
    T value{};
    for (std::size_t index = 0; index < sizeof(T); ++index) {
        value |= static_cast<T>(std::to_integer<std::uint8_t>(bytes[offset + index]))
                 << (index * 8U);
    }
    return value;
}

[[nodiscard]] std::uint64_t pack_file_time(const FILETIME value) noexcept {
    return (static_cast<std::uint64_t>(value.dwHighDateTime) << 32U) |
           static_cast<std::uint64_t>(value.dwLowDateTime);
}

[[nodiscard]] std::wstring wide_from_utf8(const std::string_view value) {
    if (value.empty() || value.size() > 1024U) return {};
    const int count = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                                          static_cast<int>(value.size()), nullptr, 0);
    if (count <= 0) return {};
    std::wstring result(static_cast<std::size_t>(count), L'\0');
    return MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                               static_cast<int>(value.size()), result.data(), count) == count
        ? result : std::wstring{};
}

[[nodiscard]] std::string sha256_hex(const std::span<const std::byte> bytes) {
    BCRYPT_ALG_HANDLE algorithm{};
    BCRYPT_HASH_HANDLE hash{};
    DWORD object_size{}, copied{};
    std::array<std::byte, 32> digest{};
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) != 0 ||
        BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                          reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size), &copied, 0) != 0) {
        if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
        return {};
    }
    std::vector<std::byte> object(object_size);
    const bool ok = BCryptCreateHash(algorithm, &hash,
                                     reinterpret_cast<PUCHAR>(object.data()), object_size,
                                     nullptr, 0, 0) == 0 &&
                    BCryptHashData(hash,
                                   const_cast<PUCHAR>(reinterpret_cast<const UCHAR*>(bytes.data())),
                                   static_cast<ULONG>(bytes.size()), 0) == 0 &&
                    BCryptFinishHash(hash, reinterpret_cast<PUCHAR>(digest.data()),
                                     static_cast<ULONG>(digest.size()), 0) == 0;
    if (hash) BCryptDestroyHash(hash);
    BCryptCloseAlgorithmProvider(algorithm, 0);
    if (!ok) return {};
    constexpr char hex[] = "0123456789abcdef";
    std::string result;
    result.reserve(64U);
    for (const auto byte : digest) {
        const auto value = std::to_integer<std::uint8_t>(byte);
        result.push_back(hex[value >> 4U]);
        result.push_back(hex[value & 0x0fU]);
    }
    return result;
}

enum class WorkerOperation : std::uint32_t { verify = 1, require_absent = 2, shutdown = 3 };

#pragma pack(push, 1)
struct WorkerRequest {
    WorkerOperation operation{};
    std::uint32_t mapping_name_size{};
    std::uint64_t byte_length{};
    std::array<char, 64> expected_sha256{};
};
struct WorkerResponse {
    std::uint32_t success{};
};
#pragma pack(pop)

[[nodiscard]] int run_mapping_worker(const HANDLE input, const HANDLE output) {
    for (;;) {
        WorkerRequest request{};
        if (!pipe_read(input, {reinterpret_cast<std::byte*>(&request), sizeof(request)}) ||
            request.mapping_name_size == 0U || request.mapping_name_size > 128U) return 20;
        std::string mapping_name(request.mapping_name_size, '\0');
        if (!pipe_read(input, {reinterpret_cast<std::byte*>(mapping_name.data()),
                               mapping_name.size()})) return 21;
        if (request.operation == WorkerOperation::shutdown) return 0;
        const auto wide_name = wide_from_utf8(mapping_name);
        HANDLE mapping = wide_name.empty()
            ? nullptr : OpenFileMappingW(FILE_MAP_READ, FALSE, wide_name.c_str());
        bool success{};
        if (request.operation == WorkerOperation::require_absent) {
            success = mapping == nullptr;
        } else if (request.operation == WorkerOperation::verify && mapping &&
                   request.byte_length > 0U && request.byte_length <= 64U * 1024U * 1024U) {
            const void* view = MapViewOfFile(mapping, FILE_MAP_READ, 0U, 0U,
                                             static_cast<SIZE_T>(request.byte_length));
            if (view) {
                const auto digest = sha256_hex({static_cast<const std::byte*>(view),
                                                static_cast<std::size_t>(request.byte_length)});
                success = digest == std::string_view{request.expected_sha256.data(),
                                                     request.expected_sha256.size()};
                UnmapViewOfFile(view);
            }
        }
        if (mapping) CloseHandle(mapping);
        const WorkerResponse response{success ? 1U : 0U};
        if (!pipe_write(output, {reinterpret_cast<const std::byte*>(&response), sizeof(response)})) {
            return 22;
        }
    }
}

struct MappingWorker {
    PROCESS_INFORMATION process{};
    HANDLE job{};
    HANDLE command{};
    HANDLE response{};
    std::uint64_t creation_time{};
    std::string executable_name;
};

void close_worker(MappingWorker& worker) {
    if (worker.command) CloseHandle(worker.command);
    if (worker.response) CloseHandle(worker.response);
    if (worker.job) CloseHandle(worker.job);
    if (worker.process.hProcess) CloseHandle(worker.process.hProcess);
    worker = {};
}

[[nodiscard]] std::optional<MappingWorker> launch_worker(const std::wstring& executable) {
    SECURITY_ATTRIBUTES inheritable{sizeof(inheritable), nullptr, TRUE};
    HANDLE child_input{}, parent_command{}, parent_response{}, child_output{};
    if (!CreatePipe(&child_input, &parent_command, &inheritable, 0U) ||
        !CreatePipe(&parent_response, &child_output, &inheritable, 0U) ||
        !SetHandleInformation(parent_command, HANDLE_FLAG_INHERIT, 0U) ||
        !SetHandleInformation(parent_response, HANDLE_FLAG_INHERIT, 0U)) {
        return std::nullopt;
    }
    MappingWorker worker{};
    worker.command = parent_command;
    worker.response = parent_response;
    std::wstring command = L"\"" + executable + L"\" --identity-mapping-worker " +
        std::to_wstring(reinterpret_cast<std::uintptr_t>(child_input)) + L" " +
        std::to_wstring(reinterpret_cast<std::uintptr_t>(child_output));
    STARTUPINFOW startup{sizeof(startup)};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, TRUE,
                        CREATE_SUSPENDED | CREATE_NO_WINDOW, nullptr, nullptr, &startup,
                        &worker.process)) {
        return std::nullopt;
    }
    CloseHandle(child_input);
    CloseHandle(child_output);
    worker.job = CreateJobObjectW(nullptr, nullptr);
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (!worker.job ||
        !SetInformationJobObject(worker.job, JobObjectExtendedLimitInformation, &limits,
                                 sizeof(limits)) ||
        !AssignProcessToJobObject(worker.job, worker.process.hProcess)) {
        TerminateProcess(worker.process.hProcess, 1U);
        close_worker(worker);
        return std::nullopt;
    }
    ResumeThread(worker.process.hThread);
    CloseHandle(worker.process.hThread);
    worker.process.hThread = nullptr;
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(worker.process.hProcess, &created, &exited, &kernel, &user)) {
        close_worker(worker);
        return std::nullopt;
    }
    worker.creation_time = pack_file_time(created);
    worker.executable_name = std::filesystem::path(executable).filename().string();
    return worker;
}

[[nodiscard]] bool worker_request(MappingWorker& worker, const WorkerOperation operation,
                                  const IdentityFrameLease& lease) {
    WorkerRequest request{operation, static_cast<std::uint32_t>(lease.shared_memory_name.size()),
                          lease.byte_length, {}};
    if (lease.content_sha256.size() == request.expected_sha256.size()) {
        std::copy(lease.content_sha256.begin(), lease.content_sha256.end(),
                  request.expected_sha256.begin());
    }
    if (!pipe_write(worker.command,
                    {reinterpret_cast<const std::byte*>(&request), sizeof(request)}) ||
        !pipe_write(worker.command,
                    {reinterpret_cast<const std::byte*>(lease.shared_memory_name.data()),
                     lease.shared_memory_name.size()})) return false;
    WorkerResponse response{};
    return pipe_read(worker.response, {reinterpret_cast<std::byte*>(&response), sizeof(response)}) &&
           response.success == 1U;
}

struct ChildService {
    PROCESS_INFORMATION process{};
    HANDLE job{};
    HANDLE pipe{INVALID_HANDLE_VALUE};
    std::string session;
    std::array<std::byte, ipc::launch_nonce_bytes> nonce{};
};

void close_service(ChildService& child) {
    if (child.pipe != INVALID_HANDLE_VALUE) CloseHandle(child.pipe);
    if (child.job) CloseHandle(child.job);
    if (child.process.hProcess) CloseHandle(child.process.hProcess);
    child = {};
}

[[nodiscard]] std::optional<ChildService> launch_service(const std::wstring& executable) {
    ChildService child{};
    child.session = "identity-smoke-" + std::to_string(GetCurrentProcessId());
    constexpr char hex[] = "0123456789abcdef";
    std::string nonce_hex;
    for (std::size_t index = 0; index < child.nonce.size(); ++index) {
        const auto value = static_cast<std::uint8_t>(index + 1U);
        child.nonce[index] = static_cast<std::byte>(value);
        nonce_hex.push_back(hex[value >> 4U]);
        nonce_hex.push_back(hex[value & 0x0fU]);
    }
    std::wstring command = L"\"" + executable + L"\" --parent-pid=" +
        std::to_wstring(GetCurrentProcessId()) + L" --session=" +
        std::wstring(child.session.begin(), child.session.end()) + L" --nonce=" +
        std::wstring(nonce_hex.begin(), nonce_hex.end());
    STARTUPINFOW startup{sizeof(startup)};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE,
                        CREATE_SUSPENDED | CREATE_NO_WINDOW, nullptr, nullptr, &startup,
                        &child.process)) return std::nullopt;
    child.job = CreateJobObjectW(nullptr, nullptr);
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (!child.job ||
        !SetInformationJobObject(child.job, JobObjectExtendedLimitInformation, &limits,
                                 sizeof(limits)) ||
        !AssignProcessToJobObject(child.job, child.process.hProcess)) {
        TerminateProcess(child.process.hProcess, 1U);
        close_service(child);
        return std::nullopt;
    }
    ResumeThread(child.process.hThread);
    CloseHandle(child.process.hThread);
    child.process.hThread = nullptr;
    const std::wstring pipe_name = L"\\\\.\\pipe\\npc-media-broker-" +
        std::wstring(child.session.begin(), child.session.end());
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(8);
    while (std::chrono::steady_clock::now() < deadline) {
        child.pipe = CreateFileW(pipe_name.c_str(), GENERIC_READ | GENERIC_WRITE, 0U, nullptr,
                                 OPEN_EXISTING, 0U, nullptr);
        if (child.pipe != INVALID_HANDLE_VALUE) return child;
        if (WaitForSingleObject(child.process.hProcess, 0U) == WAIT_OBJECT_0) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }
    close_service(child);
    return std::nullopt;
}

[[nodiscard]] std::optional<ipc::Response> transact(const HANDLE pipe,
                                                     const ipc::Envelope& envelope) {
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

[[nodiscard]] bool identity_mapping_smoke(const std::wstring& service_executable,
                                          const std::wstring& worker_executable,
                                          const HWND target_window) {
    auto child_value = launch_service(service_executable);
    auto worker_value = launch_worker(worker_executable);
    if (!child_value || !worker_value) {
        std::cerr << "identity smoke stage=launch service=" << child_value.has_value()
                  << " worker=" << worker_value.has_value() << '\n';
        return false;
    }
    auto child = std::move(*child_value);
    auto worker = std::move(*worker_value);
    const auto cleanup = [&] { close_worker(worker); close_service(child); };
    const auto fail = [&](const std::string_view stage) {
        std::cerr << "identity smoke stage=" << stage << '\n';
        cleanup();
        return false;
    };
    const auto frequency = qpc_frequency();
    const auto command = [&](ipc::Envelope& envelope, const ipc::CommandKind kind,
                             const ipc::Command& value) -> std::optional<ipc::Response> {
        const auto payload = ipc::encode_command(kind, value);
        if (!payload) return std::nullopt;
        ++envelope.sequence;
        envelope.deadline_qpc = qpc_now() + frequency * 2U;
        envelope.command = kind;
        envelope.payload = *payload;
        return transact(child.pipe, envelope);
    };
    const auto health = ipc::encode_command(ipc::CommandKind::health, ipc::HealthCommand{});
    ipc::Envelope envelope{ipc::protocol_version, child.nonce, child.session, 0U,
                           qpc_now() + frequency * 2U, 0U, ipc::CommandKind::health, {}};
    if (!health || !command(envelope, ipc::CommandKind::health, ipc::HealthCommand{})) {
        return fail("health");
    }
    const auto inspected = inspect_target(target_window, GetCurrentProcessId());
    if (!inspected.valid_window || !inspected.process_id_matches ||
        !inspected.inspection_complete || inspected.process_name.empty()) {
        std::cerr << "identity inspect valid=" << inspected.valid_window
                  << " pid=" << inspected.process_id_matches
                  << " complete=" << inspected.inspection_complete
                  << " exe=" << inspected.process_name << '\n';
        return fail("inspect_target");
    }
    const auto selected = command(
        envelope, ipc::CommandKind::select_target,
        ipc::SelectTargetCommand{reinterpret_cast<std::uintptr_t>(target_window),
                                 GetCurrentProcessId(), {inspected.process_name}});
    if (!selected || selected->status != ipc::StatusCode::ok) {
        std::cerr << "identity select status="
                  << (selected ? ipc::to_string(selected->status) : "no_response") << '\n';
        return fail("select_target");
    }
    envelope.cancellation_generation = selected->cancellation_generation;

    const ipc::AllocateIdentityFrameCommand valid{
        worker.process.dwProcessId, worker.creation_time, worker.executable_name,
        RectI{16, 16, 80, 80}};
    const auto wrong_worker = command(
        envelope, ipc::CommandKind::allocate_identity_frame,
        ipc::AllocateIdentityFrameCommand{worker.process.dwProcessId, worker.creation_time + 1U,
                                          worker.executable_name, valid.crop_px});
    const auto wrong_crop = command(
        envelope, ipc::CommandKind::allocate_identity_frame,
        ipc::AllocateIdentityFrameCommand{worker.process.dwProcessId, worker.creation_time,
                                          worker.executable_name, RectI{0, 0, 4096, 4096}});
    if (!wrong_worker || wrong_worker->status != ipc::StatusCode::capability_unavailable ||
        !wrong_crop || wrong_crop->status != ipc::StatusCode::capability_unavailable) {
        std::cerr << "identity rejection statuses worker="
                  << (wrong_worker ? ipc::to_string(wrong_worker->status) : "no_response")
                  << " crop=" << (wrong_crop ? ipc::to_string(wrong_crop->status) : "no_response")
                  << '\n';
        return fail("negative_contracts");
    }

    std::optional<IdentityFrameLease> lease;
    ipc::StatusCode last_allocation_status{ipc::StatusCode::internal_error};
    std::optional<std::uint32_t> last_allocation_failure;
    std::optional<ipc::Response> last_diagnostics;
    std::optional<ipc::Response> last_capture_evidence;
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(8);
    while (!lease && std::chrono::steady_clock::now() < deadline) {
        target_color = target_color == RGB(18, 92, 112) ? RGB(116, 34, 72) : RGB(18, 92, 112);
        InvalidateRect(target_window, nullptr, FALSE);
        UpdateWindow(target_window);
        pump_messages();
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        last_diagnostics = command(envelope, ipc::CommandKind::diagnostics,
                                   ipc::DiagnosticsCommand{});
        last_capture_evidence = command(envelope, ipc::CommandKind::capture_evidence,
                                        ipc::CaptureEvidenceCommand{});
        const auto response = command(envelope, ipc::CommandKind::allocate_identity_frame, valid);
        if (response) {
            last_allocation_status = response->status;
            if (response->payload.size() >= sizeof(std::uint32_t)) {
                std::uint32_t value{};
                std::memcpy(&value, response->payload.data(), sizeof(value));
                last_allocation_failure = value;
            }
        }
        if (response && response->status == ipc::StatusCode::ok) {
            lease = ipc::decode_identity_frame_lease(response->payload);
        }
    }
    if (!lease || lease->worker_process_id != worker.process.dwProcessId ||
        lease->worker_process_creation_time != worker.creation_time ||
        lease->worker_executable_name != worker.executable_name ||
        lease->capture_session_id != child.session || lease->crop_px != valid.crop_px ||
        lease->selected_process_id != GetCurrentProcessId() ||
        lease->selected_window_handle != reinterpret_cast<std::uintptr_t>(target_window) ||
        lease->source_device_generation == 0U || lease->source_geometry_epoch == 0U ||
        lease->source_frame_sequence == 0U || lease->source_frame_qpc == 0U ||
        !lease->advancing_frame_verified || !lease->overlay_capture_excluded ||
        lease->protected_online_detected || lease->anti_cheat_detected ||
        !worker_request(worker, WorkerOperation::verify, *lease)) {
        std::cerr << "identity allocation status=" << ipc::to_string(last_allocation_status);
        if (last_allocation_failure) std::cerr << " failure=" << *last_allocation_failure;
        if (last_diagnostics) {
            std::cerr << " state=" << read_little<std::uint32_t>(last_diagnostics->payload, 0U).value_or(999U)
                      << " backend=" << read_little<std::uint32_t>(last_diagnostics->payload, 4U).value_or(999U)
                      << " target=" << read_little<std::uint32_t>(last_diagnostics->payload, 20U).value_or(999U)
                      << " device=" << read_little<std::uint64_t>(last_diagnostics->payload, 24U).value_or(0U)
                      << " cancel=" << read_little<std::uint64_t>(last_diagnostics->payload, 40U).value_or(0U)
                      << " received=" << read_little<std::uint64_t>(last_diagnostics->payload, 48U).value_or(0U)
                      << " presented=" << read_little<std::uint64_t>(last_diagnostics->payload, 56U).value_or(0U);
        }
        if (last_capture_evidence) {
            std::cerr << " geometry=" << read_little<std::uint64_t>(last_capture_evidence->payload, 24U).value_or(0U)
                      << " sequence=" << read_little<std::uint64_t>(last_capture_evidence->payload, 32U).value_or(0U)
                      << " frame_qpc=" << read_little<std::uint64_t>(last_capture_evidence->payload, 40U).value_or(0U)
                      << " size=" << read_little<std::uint32_t>(last_capture_evidence->payload, 88U).value_or(0U)
                      << 'x' << read_little<std::uint32_t>(last_capture_evidence->payload, 92U).value_or(0U)
                      << " excluded=" << read_little<std::uint32_t>(last_capture_evidence->payload, 96U).value_or(0U)
                      << " visuals=" << read_little<std::uint32_t>(last_capture_evidence->payload, 100U).value_or(0U);
        }
        if (lease) {
            std::cerr << " lease pid=" << lease->worker_process_id
                      << " creation=" << lease->worker_process_creation_time
                      << " frame=" << lease->source_frame_sequence
                      << " qpc=" << lease->source_frame_qpc
                      << " geometry=" << lease->source_geometry_epoch
                      << " advancing=" << lease->advancing_frame_verified
                      << " excluded=" << lease->overlay_capture_excluded;
        }
        std::cerr << '\n';
        return fail("allocate_or_worker_verify");
    }
    const auto released = command(
        envelope, ipc::CommandKind::release_identity_frame,
        ipc::ReleaseIdentityFrameCommand{worker.process.dwProcessId, lease->lease_id,
                                         lease->lease_nonce});
    if (!released || released->status != ipc::StatusCode::ok ||
        !worker_request(worker, WorkerOperation::require_absent, *lease)) {
        std::cerr << "identity release status="
                  << (released ? ipc::to_string(released->status) : "no_response") << '\n';
        return fail("release_or_reopen");
    }

    const auto expiring_response = command(envelope, ipc::CommandKind::allocate_identity_frame, valid);
    auto expiring = expiring_response && expiring_response->status == ipc::StatusCode::ok
        ? ipc::decode_identity_frame_lease(expiring_response->payload) : std::nullopt;
    if (!expiring) {
        std::cerr << "identity expiry allocation status="
                  << (expiring_response ? ipc::to_string(expiring_response->status) : "no_response")
                  << '\n';
        return fail("expiry_allocate");
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(600));
    std::optional<ipc::Response> replacement_response;
    std::optional<IdentityFrameLease> replacement;
    const auto replacement_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    while (!replacement && std::chrono::steady_clock::now() < replacement_deadline) {
        target_color = target_color == RGB(18, 92, 112) ? RGB(116, 34, 72) : RGB(18, 92, 112);
        InvalidateRect(target_window, nullptr, FALSE);
        UpdateWindow(target_window);
        pump_messages();
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        replacement_response = command(envelope, ipc::CommandKind::allocate_identity_frame, valid);
        if (replacement_response && replacement_response->status == ipc::StatusCode::ok) {
            replacement = ipc::decode_identity_frame_lease(replacement_response->payload);
        }
    }
    if (!replacement || !worker_request(worker, WorkerOperation::require_absent, *expiring)) {
        std::cerr << "identity replacement status="
                  << (replacement_response ? ipc::to_string(replacement_response->status)
                                           : "no_response") << '\n';
        return fail("expiry_cleanup");
    }
    const auto replacement_release = command(
        envelope, ipc::CommandKind::release_identity_frame,
        ipc::ReleaseIdentityFrameCommand{worker.process.dwProcessId, replacement->lease_id,
                                         replacement->lease_nonce});
    if (!replacement_release || replacement_release->status != ipc::StatusCode::ok) {
        return fail("replacement_release");
    }

    const auto cancelled_response = command(envelope, ipc::CommandKind::allocate_identity_frame, valid);
    auto cancelled = cancelled_response && cancelled_response->status == ipc::StatusCode::ok
        ? ipc::decode_identity_frame_lease(cancelled_response->payload) : std::nullopt;
    const auto cancelled_generation = envelope.cancellation_generation + 1U;
    const auto cancellation = command(envelope, ipc::CommandKind::cancel,
                                      ipc::CancelCommand{cancelled_generation});
    const bool passed = cancelled && cancellation && cancellation->status == ipc::StatusCode::ok &&
        cancellation->cancellation_generation == cancelled_generation &&
        worker_request(worker, WorkerOperation::require_absent, *cancelled);
    if (!passed) {
        std::cerr << "identity cancel lease=" << cancelled.has_value() << " status="
                  << (cancellation ? ipc::to_string(cancellation->status) : "no_response")
                  << " expected_generation=" << cancelled_generation
                  << " actual_generation="
                  << (cancellation ? cancellation->cancellation_generation : 0U) << '\n';
    }
    command(envelope, ipc::CommandKind::shutdown, ipc::ShutdownCommand{});
    cleanup();
    return passed;
}

} // namespace

int main(int argc, char** argv) {
    if (argc == 4 && std::string_view{argv[1]} == "--identity-mapping-worker") {
        const auto input = std::strtoull(argv[2], nullptr, 10);
        const auto output = std::strtoull(argv[3], nullptr, 10);
        return input != 0U && output != 0U
            ? run_mapping_worker(reinterpret_cast<HANDLE>(input),
                                 reinterpret_cast<HANDLE>(output))
            : EXIT_FAILURE;
    }
    if (argc != 1) return EXIT_FAILURE;
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    const HWND window = create_target_window();
    wchar_t test_path[32768]{};
    if (!window || GetModuleFileNameW(nullptr, test_path, ARRAYSIZE(test_path)) == 0U) {
        if (window) DestroyWindow(window);
        return EXIT_FAILURE;
    }
    const auto broker_path =
        (std::filesystem::path(test_path).parent_path() / L"npc-media-broker.exe").wstring();
    const bool passed = identity_mapping_smoke(broker_path, test_path, window);
    DestroyWindow(window);
    if (!passed) {
        std::cerr << "authenticated identity mapping smoke failed\n";
        return EXIT_FAILURE;
    }
    std::cout << "identity command 16/17 mapping, digest, release, expiry, and cancellation passed\n";
    return EXIT_SUCCESS;
}

#else
int main() { return 0; }
#endif
