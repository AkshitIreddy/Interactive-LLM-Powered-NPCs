#include "system_monitor.hpp"

#include <Psapi.h>
#include <Wbemidl.h>
#include <comdef.h>

#include <algorithm>
#include <chrono>
#include <iomanip>
#include <sstream>
#include <vector>

namespace game_load::win32 {
namespace {

template <typename T>
class ComReleaser {
 public:
  ~ComReleaser() { if (value_ != nullptr) value_->Release(); }
  T** out() { return &value_; }
  T* get() const { return value_; }
 private:
  T* value_ = nullptr;
};

class ComApartment final {
 public:
  ComApartment() noexcept : result_(CoInitializeEx(nullptr, COINIT_MULTITHREADED)) {}
  ~ComApartment() {
    if (SUCCEEDED(result_)) CoUninitialize();
  }
  ComApartment(const ComApartment&) = delete;
  ComApartment& operator=(const ComApartment&) = delete;
  [[nodiscard]] HRESULT result() const noexcept { return result_; }

 private:
  HRESULT result_;
};

std::string win32_error_message(HRESULT hr) {
  _com_error error(hr);
  return wide_to_utf8(error.ErrorMessage() != nullptr ? error.ErrorMessage() : L"unknown COM error");
}

}  // namespace

std::string wide_to_utf8(std::wstring_view value) {
  if (value.empty()) return {};
  const int size = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                                       static_cast<int>(value.size()), nullptr, 0,
                                       nullptr, nullptr);
  if (size <= 0) return {};
  std::string result(static_cast<std::size_t>(size), '\0');
  WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                      static_cast<int>(value.size()), result.data(), size, nullptr,
                      nullptr);
  return result;
}

SystemMemorySnapshot query_system_memory() noexcept {
  SystemMemorySnapshot snapshot;
  PERFORMANCE_INFORMATION performance{};
  performance.cb = sizeof(performance);
  MEMORYSTATUSEX memory{};
  memory.dwLength = sizeof(memory);
  if (!GetPerformanceInfo(&performance, sizeof(performance)) ||
      !GlobalMemoryStatusEx(&memory)) {
    return snapshot;
  }
  snapshot.available = true;
  snapshot.commit_limit_bytes = static_cast<std::uint64_t>(performance.CommitLimit) *
                                performance.PageSize;
  snapshot.commit_total_bytes = static_cast<std::uint64_t>(performance.CommitTotal) *
                                performance.PageSize;
  snapshot.available_physical_bytes = memory.ullAvailPhys;
  return snapshot;
}

VideoMemorySnapshot query_video_memory(IDXGIAdapter3* adapter) noexcept {
  VideoMemorySnapshot snapshot;
  if (adapter == nullptr) return snapshot;
  DXGI_QUERY_VIDEO_MEMORY_INFO info{};
  if (FAILED(adapter->QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &info))) {
    return snapshot;
  }
  snapshot.available = true;
  snapshot.budget_bytes = info.Budget;
  snapshot.current_usage_bytes = info.CurrentUsage;
  snapshot.current_reservation_bytes = info.CurrentReservation;
  return snapshot;
}

std::optional<double> query_acpi_temperature_c(std::string* warning) noexcept {
  // Declare the apartment before every COM smart owner so reverse destruction
  // releases WMI objects before CoUninitialize. Reversing this order can cause an
  // access violation on firmware that returns a WMI query error.
  ComApartment apartment;
  const HRESULT init = apartment.result();
  if (FAILED(init) && init != RPC_E_CHANGED_MODE) {
    if (warning) *warning = "COM initialization failed: " + win32_error_message(init);
    return std::nullopt;
  }

  ComReleaser<IWbemLocator> locator;
  HRESULT hr = CoCreateInstance(CLSID_WbemLocator, nullptr, CLSCTX_INPROC_SERVER,
                                IID_IWbemLocator,
                                reinterpret_cast<void**>(locator.out()));
  if (FAILED(hr)) {
    if (warning) *warning = "WMI locator unavailable: " + win32_error_message(hr);
    return std::nullopt;
  }

  ComReleaser<IWbemServices> services;
  BSTR root = SysAllocString(L"ROOT\\WMI");
  hr = locator.get()->ConnectServer(root, nullptr, nullptr, nullptr, 0, nullptr,
                                    nullptr, services.out());
  SysFreeString(root);
  if (FAILED(hr)) {
    if (warning) *warning = "ACPI thermal WMI namespace unavailable: " + win32_error_message(hr);
    return std::nullopt;
  }

  hr = CoSetProxyBlanket(services.get(), RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE,
                         nullptr, RPC_C_AUTHN_LEVEL_CALL,
                         RPC_C_IMP_LEVEL_IMPERSONATE, nullptr, EOAC_NONE);
  if (FAILED(hr)) {
    if (warning) *warning = "WMI proxy security failed: " + win32_error_message(hr);
    return std::nullopt;
  }

  ComReleaser<IEnumWbemClassObject> enumerator;
  BSTR language = SysAllocString(L"WQL");
  BSTR query = SysAllocString(
      L"SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature");
  hr = services.get()->ExecQuery(language, query,
                                 WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                                 nullptr, enumerator.out());
  SysFreeString(query);
  SysFreeString(language);
  if (FAILED(hr)) {
    if (warning) *warning = "ACPI temperature query failed: " + win32_error_message(hr);
    return std::nullopt;
  }

  std::optional<double> maximum;
  while (true) {
    IWbemClassObject* raw = nullptr;
    ULONG returned = 0;
    hr = enumerator.get()->Next(500, 1, &raw, &returned);
    if (FAILED(hr) || returned == 0 || raw == nullptr) break;
    VARIANT value;
    VariantInit(&value);
    if (SUCCEEDED(raw->Get(L"CurrentTemperature", 0, &value, nullptr, nullptr))) {
      std::uint64_t tenths_kelvin = 0;
      if (value.vt == VT_I4 || value.vt == VT_INT) tenths_kelvin = value.lVal;
      else if (value.vt == VT_UI4 || value.vt == VT_UINT) tenths_kelvin = value.ulVal;
      if (tenths_kelvin > 0) {
        const double celsius = static_cast<double>(tenths_kelvin) / 10.0 - 273.15;
        if (celsius > -50.0 && celsius < 200.0) {
          maximum = maximum ? std::max(*maximum, celsius) : celsius;
        }
      }
    }
    VariantClear(&value);
    raw->Release();
  }

  if (!maximum && warning) {
    *warning = "No readable MSAcpi_ThermalZoneTemperature sensor was reported";
  }
  return maximum;
}

std::string query_operating_system() {
  using RtlGetVersionFn = LONG(WINAPI*)(PRTL_OSVERSIONINFOW);
  const HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
  auto rtl_get_version = reinterpret_cast<RtlGetVersionFn>(
      GetProcAddress(ntdll, "RtlGetVersion"));
  RTL_OSVERSIONINFOW version{};
  version.dwOSVersionInfoSize = sizeof(version);
  if (rtl_get_version == nullptr || rtl_get_version(&version) != 0) return "Windows (version unavailable)";
  std::ostringstream stream;
  stream << "Windows " << version.dwMajorVersion << '.' << version.dwMinorVersion
         << " build " << version.dwBuildNumber;
  return stream.str();
}

std::string query_cpu_name() {
  HKEY key = nullptr;
  if (RegOpenKeyExW(HKEY_LOCAL_MACHINE,
                    L"HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0", 0,
                    KEY_QUERY_VALUE, &key) != ERROR_SUCCESS) {
    return "unknown";
  }
  wchar_t value[256]{};
  DWORD type = 0;
  DWORD bytes = sizeof(value);
  const auto status = RegQueryValueExW(key, L"ProcessorNameString", nullptr, &type,
                                      reinterpret_cast<BYTE*>(value), &bytes);
  RegCloseKey(key);
  if (status != ERROR_SUCCESS || (type != REG_SZ && type != REG_EXPAND_SZ)) return "unknown";
  return wide_to_utf8(value);
}

std::string utc_now_iso8601() {
  const auto now = std::chrono::system_clock::now();
  const auto seconds = std::chrono::system_clock::to_time_t(now);
  std::tm utc{};
  gmtime_s(&utc, &seconds);
  const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(
                          now.time_since_epoch()) %
                      1000;
  std::ostringstream stream;
  stream << std::put_time(&utc, "%Y-%m-%dT%H:%M:%S") << '.' << std::setw(3)
         << std::setfill('0') << millis.count() << 'Z';
  return stream.str();
}

}  // namespace game_load::win32
