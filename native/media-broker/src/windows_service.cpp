#ifdef _WIN32

#include "windows_service.hpp"

#include <AclAPI.h>
#include <TlHelp32.h>
#include <dwmapi.h>
#include <sddl.h>

#include <algorithm>
#include <array>
#include <condition_variable>
#include <filesystem>
#include <mutex>
#include <queue>
#include <thread>
#include <utility>
#include <vector>

namespace npc::media::windows {

namespace {

[[nodiscard]] std::vector<std::byte> token_user_sid(HANDLE process) {
    HANDLE token{};
    if (!OpenProcessToken(process, TOKEN_QUERY, &token)) return {};
    DWORD size{};
    GetTokenInformation(token, TokenUser, nullptr, 0, &size);
    std::vector<std::byte> bytes(size);
    if (!GetTokenInformation(token, TokenUser, bytes.data(), size, &size)) bytes.clear();
    CloseHandle(token);
    return bytes;
}

[[nodiscard]] std::optional<DWORD> token_session(HANDLE process) {
    HANDLE token{};
    if (!OpenProcessToken(process, TOKEN_QUERY, &token)) return std::nullopt;
    DWORD session{}, size{};
    const bool ok = GetTokenInformation(token, TokenSessionId, &session, sizeof(session), &size) != FALSE;
    CloseHandle(token);
    return ok ? std::optional{session} : std::nullopt;
}

[[nodiscard]] bool same_user(HANDLE left, HANDLE right) {
    const auto left_bytes = token_user_sid(left);
    const auto right_bytes = token_user_sid(right);
    if (left_bytes.empty() || right_bytes.empty()) return false;
    const auto* left_user = reinterpret_cast<const TOKEN_USER*>(left_bytes.data());
    const auto* right_user = reinterpret_cast<const TOKEN_USER*>(right_bytes.data());
    return EqualSid(left_user->User.Sid, right_user->User.Sid) != FALSE;
}

[[nodiscard]] std::string narrow_lower(const std::wstring_view value) {
    if (value.empty()) return {};
    const int required = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                                             static_cast<int>(value.size()), nullptr, 0, nullptr, nullptr);
    if (required <= 0) return {};
    std::string result(static_cast<std::size_t>(required), '\0');
    WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()),
                        result.data(), required, nullptr, nullptr);
    std::transform(result.begin(), result.end(), result.begin(),
                   [](const unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return result;
}

[[nodiscard]] std::optional<DWORD> actual_parent_process_id() {
    const DWORD current = GetCurrentProcessId();
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return std::nullopt;
    PROCESSENTRY32W entry{sizeof(entry)};
    std::optional<DWORD> result;
    if (Process32FirstW(snapshot, &entry)) {
        do {
            if (entry.th32ProcessID == current) { result = entry.th32ParentProcessID; break; }
        } while (Process32NextW(snapshot, &entry));
    }
    CloseHandle(snapshot);
    return result;
}

[[nodiscard]] bool read_exact(HANDLE pipe, std::span<std::byte> output) {
    std::size_t position{};
    while (position < output.size()) {
        DWORD read{};
        if (!ReadFile(pipe, output.data() + position,
                      static_cast<DWORD>(output.size() - position), &read, nullptr) || read == 0) return false;
        position += read;
    }
    return true;
}

[[nodiscard]] bool write_exact(HANDLE pipe, std::span<const std::byte> input) {
    std::size_t position{};
    while (position < input.size()) {
        DWORD written{};
        if (!WriteFile(pipe, input.data() + position,
                       static_cast<DWORD>(input.size() - position), &written, nullptr) || written == 0) return false;
        position += written;
    }
    return true;
}

} // namespace

TargetInspectionEvidence inspect_target(const HWND window, const std::uint32_t expected_process_id) {
    TargetInspectionEvidence evidence;
    evidence.valid_window = window && IsWindow(window) != FALSE;
    DWORD cloaked{};
    if (evidence.valid_window &&
        (IsWindowVisible(window) == FALSE ||
         (SUCCEEDED(DwmGetWindowAttribute(window, DWMWA_CLOAKED, &cloaked, sizeof(cloaked))) && cloaked != 0))) {
        evidence.valid_window = false;
    }
    evidence.minimized = evidence.valid_window && IsIconic(window) != FALSE;
    if (!evidence.valid_window) return evidence;

    DWORD process_id{};
    GetWindowThreadProcessId(window, &process_id);
    evidence.process_id = process_id;
    evidence.process_id_matches = expected_process_id == 0 || expected_process_id == process_id;
    // Policy inspection requires process metadata only. Never request VM write,
    // VM operation, thread creation, or even VM read access to a selected game.
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id);
    if (!process) {
        evidence.protected_process = GetLastError() == ERROR_ACCESS_DENIED;
        return evidence;
    }

    DWORD own_session{}, target_session{};
    evidence.session_matches = ProcessIdToSessionId(GetCurrentProcessId(), &own_session) &&
                               ProcessIdToSessionId(process_id, &target_session) && own_session == target_session;
    evidence.user_matches = same_user(GetCurrentProcess(), process);

    PROCESS_PROTECTION_LEVEL_INFORMATION protection{};
    if (GetProcessInformation(process, ProcessProtectionLevelInfo, &protection, sizeof(protection))) {
        evidence.protected_process = protection.ProtectionLevel != PROTECTION_LEVEL_NONE;
    }

    std::wstring path(32768, L'\0');
    DWORD path_size = static_cast<DWORD>(path.size());
    if (QueryFullProcessImageNameW(process, 0, path.data(), &path_size)) {
        path.resize(path_size);
        evidence.process_name = narrow_lower(std::filesystem::path(path).filename().wstring());
    }

    HANDLE modules = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, process_id);
    if (modules != INVALID_HANDLE_VALUE) {
        MODULEENTRY32W module{sizeof(module)};
        if (Module32FirstW(modules, &module)) {
            do {
                evidence.loaded_module_names.push_back(narrow_lower(module.szModule));
            } while (evidence.loaded_module_names.size() < 2048 && Module32NextW(modules, &module));
            evidence.inspection_complete = !evidence.process_name.empty();
        }
        CloseHandle(modules);
    }
    CloseHandle(process);
    return evidence;
}

bool process_is_current_user_and_session(HANDLE process) noexcept {
    if (!process) return false;
    const auto own_session = token_session(GetCurrentProcess());
    const auto other_session = token_session(process);
    return own_session && other_session && *own_session == *other_session && same_user(GetCurrentProcess(), process);
}

bool verify_parent_and_job(const ServiceLaunchConfig& config, HANDLE& parent_handle) noexcept {
    BOOL in_job{};
    if (!IsProcessInJob(GetCurrentProcess(), nullptr, &in_job) || !in_job) return false;
    const auto actual_parent = actual_parent_process_id();
    if (!actual_parent || *actual_parent != config.parent_process_id) return false;
    parent_handle = OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, config.parent_process_id);
    return parent_handle && process_is_current_user_and_session(parent_handle);
}

std::uint64_t qpc_now() noexcept { LARGE_INTEGER value{}; QueryPerformanceCounter(&value); return static_cast<std::uint64_t>(value.QuadPart); }
std::uint64_t qpc_frequency() noexcept { LARGE_INTEGER value{}; QueryPerformanceFrequency(&value); return static_cast<std::uint64_t>(value.QuadPart); }

struct CurrentUserPipeServer::Impl {
    Impl(std::string value, const std::uint32_t expected_pid)
        : pipe_name(std::move(value)), expected_client_process_id(expected_pid) {}
    std::string pipe_name;
    std::uint32_t expected_client_process_id{};
    HANDLE pipe{INVALID_HANDLE_VALUE};
    std::jthread worker;
    mutable std::mutex mutex;
    std::condition_variable_any condition;
    std::queue<ipc::Envelope> requests;
    std::queue<ipc::Response> responses;
    std::atomic_bool connected{};
    std::atomic_bool disconnected{};
    std::atomic_bool trusted{};
    std::atomic<std::uint64_t> last_response_sequence{};

    void run(std::stop_token token) {
        const BOOL connected_result = ConnectNamedPipe(pipe, nullptr);
        if (!connected_result && GetLastError() != ERROR_PIPE_CONNECTED) { disconnected = true; return; }
        ULONG client_pid{};
        if (!GetNamedPipeClientProcessId(pipe, &client_pid) || client_pid != expected_client_process_id) {
            disconnected = true;
            return;
        }
        HANDLE client = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, client_pid);
        trusted = client && process_is_current_user_and_session(client);
        if (client) CloseHandle(client);
        if (!trusted) { disconnected = true; DisconnectNamedPipe(pipe); return; }
        connected = true;

        while (!token.stop_requested()) {
            std::array<std::byte, 4> prefix{};
            if (!read_exact(pipe, prefix)) break;
            const auto size = ipc::decode_frame_size(prefix);
            if (!size) break;
            std::vector<std::byte> frame(*size);
            if (!read_exact(pipe, frame)) break;
            auto envelope = ipc::decode_envelope(frame);
            if (!envelope) break;
            {
                std::scoped_lock lock(mutex);
                if (requests.size() >= ipc::maximum_pending_requests) break;
                requests.push(std::move(*envelope));
            }
            condition.notify_all();

            std::optional<ipc::Response> response;
            {
                std::unique_lock lock(mutex);
                condition.wait(lock, token, [&] { return !responses.empty(); });
                if (token.stop_requested() || responses.empty()) break;
                response = std::move(responses.front());
                responses.pop();
            }
            const auto encoded = ipc::encode_response(*response);
            if (!encoded) break;
            const auto framed = ipc::frame_message(*encoded);
            if (!write_exact(pipe, framed)) break;
            last_response_sequence.store(response->response_to_sequence, std::memory_order_release);
        }
        connected = false;
        disconnected = true;
        DisconnectNamedPipe(pipe);
        condition.notify_all();
    }
};

CurrentUserPipeServer::CurrentUserPipeServer(std::string pipe_name,
                                             const std::uint32_t expected_client_process_id)
    : impl_(std::make_unique<Impl>(std::move(pipe_name), expected_client_process_id)) {}
CurrentUserPipeServer::~CurrentUserPipeServer() { stop(); }

bool CurrentUserPipeServer::start() {
    if (impl_->pipe != INVALID_HANDLE_VALUE) return true;
    const auto current_user = token_user_sid(GetCurrentProcess());
    if (current_user.empty()) return false;
    const auto* user = reinterpret_cast<const TOKEN_USER*>(current_user.data());
    LPWSTR sid_text{};
    if (!ConvertSidToStringSidW(user->User.Sid, &sid_text)) return false;
    const std::wstring sddl = L"D:P(A;;GA;;;" + std::wstring(sid_text) + L")";
    LocalFree(sid_text);
    PSECURITY_DESCRIPTOR descriptor{};
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(), SDDL_REVISION_1,
                                                               &descriptor, nullptr)) return false;
    SECURITY_ATTRIBUTES attributes{sizeof(attributes), descriptor, FALSE};
    const std::wstring pipe_name(impl_->pipe_name.begin(), impl_->pipe_name.end());
    impl_->pipe = CreateNamedPipeW(pipe_name.c_str(),
                                  PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                                  PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                                  1, ipc::maximum_frame_bytes + 4, ipc::maximum_frame_bytes + 4,
                                  0, &attributes);
    LocalFree(descriptor);
    if (impl_->pipe == INVALID_HANDLE_VALUE) return false;
    impl_->worker = std::jthread([state = impl_.get()](std::stop_token token) { state->run(token); });
    return true;
}

void CurrentUserPipeServer::stop() noexcept {
    if (!impl_ || impl_->pipe == INVALID_HANDLE_VALUE) return;
    if (impl_->worker.joinable()) impl_->worker.request_stop();
    if (impl_->worker.joinable()) CancelSynchronousIo(impl_->worker.native_handle());
    CancelIoEx(impl_->pipe, nullptr);
    DisconnectNamedPipe(impl_->pipe);
    impl_->condition.notify_all();
    if (impl_->worker.joinable()) impl_->worker.join();
    CloseHandle(impl_->pipe);
    impl_->pipe = INVALID_HANDLE_VALUE;
}

std::optional<ipc::Envelope> CurrentUserPipeServer::take_request() {
    std::scoped_lock lock(impl_->mutex);
    if (impl_->requests.empty()) return std::nullopt;
    auto result = std::move(impl_->requests.front()); impl_->requests.pop(); return result;
}

bool CurrentUserPipeServer::submit_response(ipc::Response response) {
    std::scoped_lock lock(impl_->mutex);
    if (impl_->responses.size() >= ipc::maximum_pending_requests || impl_->disconnected) return false;
    impl_->responses.push(std::move(response)); impl_->condition.notify_all(); return true;
}

bool CurrentUserPipeServer::connected() const noexcept { return impl_->connected; }
bool CurrentUserPipeServer::disconnected() const noexcept { return impl_->disconnected; }
bool CurrentUserPipeServer::client_trusted() const noexcept { return impl_->trusted; }
bool CurrentUserPipeServer::response_sent(const std::uint64_t sequence) const noexcept {
    return impl_->last_response_sequence.load(std::memory_order_acquire) >= sequence;
}

} // namespace npc::media::windows

#endif
