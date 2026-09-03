use crate::{
    device_fingerprint_sha256, unavailable, AdapterLuid, AdapterSelector, CapturedTimestamps,
    GraphicsAdapterIdentity, Observation, ResourceTelemetrySnapshotV1, TelemetryRequest,
    TelemetrySource, UnavailableReason, RESOURCE_TELEMETRY_SCHEMA_V1,
};
use std::{ffi::c_void, mem::size_of, ptr};
use windows::{
    core::{s, w, Error, Interface, PCSTR},
    Win32::{
        Foundation::{CloseHandle, FreeLibrary, HANDLE, HMODULE},
        Graphics::Dxgi::{
            CreateDXGIFactory1, IDXGIAdapter1, IDXGIAdapter3, IDXGIFactory1, DXGI_ADAPTER_DESC1,
            DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_NOT_FOUND, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
            DXGI_QUERY_VIDEO_MEMORY_INFO,
        },
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    },
};

const NVIDIA_VENDOR_ID: u32 = 0x10de;

pub(crate) fn collect(
    request: TelemetryRequest,
    timestamps: CapturedTimestamps,
) -> ResourceTelemetrySnapshotV1 {
    let (physical_ram_bytes, available_ram_bytes) = sample_ram(timestamps);
    let selected = select_adapter(request.adapter);
    let adapter = match &selected {
        Ok(selected) => available(
            selected.identity.clone(),
            timestamps,
            TelemetrySource::DxgiAdapterDescription,
        ),
        Err(reason) => unavailable(
            timestamps,
            TelemetrySource::DxgiAdapterDescription,
            reason.clone(),
        ),
    };
    let dedicated_vram_bytes = match &selected {
        Ok(selected) if selected.dedicated_vram_bytes > 0 => available(
            selected.dedicated_vram_bytes,
            timestamps,
            TelemetrySource::DxgiAdapterDescription,
        ),
        Ok(_) => unavailable(
            timestamps,
            TelemetrySource::DxgiAdapterDescription,
            UnavailableReason::InconsistentMeasurement,
        ),
        Err(reason) => unavailable(
            timestamps,
            TelemetrySource::DxgiAdapterDescription,
            reason.clone(),
        ),
    };
    let device_fingerprint_sha256 = match &selected {
        Ok(selected) if selected.dedicated_vram_bytes > 0 => available(
            device_fingerprint_sha256(&selected.identity, selected.dedicated_vram_bytes),
            timestamps,
            TelemetrySource::DxgiAdapterDescription,
        ),
        Ok(_) => unavailable(
            timestamps,
            TelemetrySource::DxgiAdapterDescription,
            UnavailableReason::InconsistentMeasurement,
        ),
        Err(reason) => unavailable(
            timestamps,
            TelemetrySource::DxgiAdapterDescription,
            reason.clone(),
        ),
    };
    let (os_local_vram_budget_bytes, current_process_local_vram_bytes) = match &selected {
        Ok(selected) => sample_dxgi_budget(selected, timestamps),
        Err(reason) => {
            let metric = || {
                unavailable(
                    timestamps,
                    TelemetrySource::DxgiProcessVideoMemoryInfo,
                    reason.clone(),
                )
            };
            (metric(), metric())
        }
    };
    let selected_game_working_set_bytes = match request.selected_game_pid {
        Some(pid) => sample_process_working_set(pid, timestamps),
        None => unavailable(
            timestamps,
            TelemetrySource::Win32ProcessMemoryInfo,
            UnavailableReason::GameProcessNotSelected,
        ),
    };
    let (total_device_pressure_vram_bytes, selected_game_vram_bytes) = match &selected {
        Ok(selected) => sample_nvml(selected, request.selected_game_pid, timestamps),
        Err(reason) => {
            let total = unavailable(
                timestamps,
                TelemetrySource::NvmlDeviceMemoryInfo,
                reason.clone(),
            );
            let game = unavailable(
                timestamps,
                TelemetrySource::NvmlRunningProcesses,
                request
                    .selected_game_pid
                    .map_or(UnavailableReason::GameProcessNotSelected, |_| {
                        reason.clone()
                    }),
            );
            (total, game)
        }
    };

    ResourceTelemetrySnapshotV1 {
        schema: RESOURCE_TELEMETRY_SCHEMA_V1.to_owned(),
        captured_unix_millis: timestamps.unix_millis,
        captured_monotonic_millis: timestamps.monotonic_millis,
        selected_game_pid: request.selected_game_pid,
        physical_ram_bytes,
        available_ram_bytes,
        adapter,
        device_fingerprint_sha256,
        dedicated_vram_bytes,
        os_local_vram_budget_bytes,
        current_process_local_vram_bytes,
        total_device_pressure_vram_bytes,
        selected_game_working_set_bytes,
        selected_game_vram_bytes,
    }
}

fn available<T>(
    value: T,
    timestamps: CapturedTimestamps,
    source: TelemetrySource,
) -> Observation<T> {
    Observation::Available {
        value,
        provenance: timestamps.provenance(source),
    }
}

fn sample_ram(timestamps: CapturedTimestamps) -> (Observation<u64>, Observation<u64>) {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is a correctly sized, writable MEMORYSTATUSEX whose
    // required dwLength field is initialized for the duration of the call.
    match unsafe { GlobalMemoryStatusEx(&mut status) } {
        Ok(()) if status.ullTotalPhys > 0 && status.ullAvailPhys <= status.ullTotalPhys => (
            available(
                status.ullTotalPhys,
                timestamps,
                TelemetrySource::Win32GlobalMemoryStatusEx,
            ),
            available(
                status.ullAvailPhys,
                timestamps,
                TelemetrySource::Win32GlobalMemoryStatusEx,
            ),
        ),
        Ok(()) => {
            let metric = || {
                unavailable(
                    timestamps,
                    TelemetrySource::Win32GlobalMemoryStatusEx,
                    UnavailableReason::InconsistentMeasurement,
                )
            };
            (metric(), metric())
        }
        Err(error) => {
            let reason = win32_reason(&error);
            let metric = || {
                unavailable(
                    timestamps,
                    TelemetrySource::Win32GlobalMemoryStatusEx,
                    reason.clone(),
                )
            };
            (metric(), metric())
        }
    }
}

struct SelectedAdapter {
    adapter: IDXGIAdapter1,
    identity: GraphicsAdapterIdentity,
    dedicated_vram_bytes: u64,
}

fn select_adapter(selector: AdapterSelector) -> Result<SelectedAdapter, UnavailableReason> {
    // SAFETY: CreateDXGIFactory1 creates a reference-counted COM interface and
    // does not require caller-owned pointers.
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|error| win32_reason(&error))?;
    let mut candidates = Vec::new();
    for index in 0..64 {
        // SAFETY: DXGI owns the returned reference-counted adapter interface.
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(win32_reason(&error)),
        };
        let mut description = DXGI_ADAPTER_DESC1::default();
        // SAFETY: description is a valid writable structure for this call.
        unsafe { adapter.GetDesc1(&mut description) }.map_err(|error| win32_reason(&error))?;
        if description.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        let identity = GraphicsAdapterIdentity {
            description: utf16_nul_terminated(&description.Description),
            luid: AdapterLuid {
                low_part: description.AdapterLuid.LowPart,
                high_part: description.AdapterLuid.HighPart,
            },
            vendor_id: description.VendorId,
            device_id: description.DeviceId,
            subsystem_id: description.SubSysId,
            revision: description.Revision,
        };
        let dedicated_vram_bytes = u64::try_from(description.DedicatedVideoMemory)
            .map_err(|_| UnavailableReason::InconsistentMeasurement)?;
        candidates.push(SelectedAdapter {
            adapter,
            identity,
            dedicated_vram_bytes,
        });
    }
    match selector {
        AdapterSelector::LargestDedicatedMemory => candidates
            .into_iter()
            .max_by(|left, right| {
                left.dedicated_vram_bytes
                    .cmp(&right.dedicated_vram_bytes)
                    .then_with(|| right.identity.luid.cmp(&left.identity.luid))
            })
            .ok_or(UnavailableReason::AdapterNotFound),
        AdapterSelector::ExactLuid(luid) => candidates
            .into_iter()
            .find(|candidate| candidate.identity.luid == luid)
            .ok_or(UnavailableReason::AdapterNotFound),
    }
}

fn sample_dxgi_budget(
    selected: &SelectedAdapter,
    timestamps: CapturedTimestamps,
) -> (Observation<u64>, Observation<u64>) {
    let adapter3 = match selected.adapter.cast::<IDXGIAdapter3>() {
        Ok(adapter) => adapter,
        Err(_) => {
            let metric = || {
                unavailable(
                    timestamps,
                    TelemetrySource::DxgiProcessVideoMemoryInfo,
                    UnavailableReason::AdapterDoesNotSupportBudgetQuery,
                )
            };
            return (metric(), metric());
        }
    };
    let mut memory = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    // SAFETY: `memory` is a valid writable result structure; node zero is the
    // documented node for a single physical adapter.
    match unsafe { adapter3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut memory) }
    {
        Ok(()) if memory.Budget > 0 => (
            available(
                memory.Budget,
                timestamps,
                TelemetrySource::DxgiProcessVideoMemoryInfo,
            ),
            available(
                memory.CurrentUsage,
                timestamps,
                TelemetrySource::DxgiProcessVideoMemoryInfo,
            ),
        ),
        Ok(()) => {
            let metric = || {
                unavailable(
                    timestamps,
                    TelemetrySource::DxgiProcessVideoMemoryInfo,
                    UnavailableReason::InconsistentMeasurement,
                )
            };
            (metric(), metric())
        }
        Err(error) => {
            let reason = win32_reason(&error);
            let metric = || {
                unavailable(
                    timestamps,
                    TelemetrySource::DxgiProcessVideoMemoryInfo,
                    reason.clone(),
                )
            };
            (metric(), metric())
        }
    }
}

fn sample_process_working_set(pid: u32, timestamps: CapturedTimestamps) -> Observation<u64> {
    // SAFETY: OpenProcess receives a caller-provided PID and requests only query/read rights.
    let process =
        match unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) } {
            Ok(handle) => OwnedHandle(handle),
            Err(error) => {
                return unavailable(
                    timestamps,
                    TelemetrySource::Win32ProcessMemoryInfo,
                    win32_reason(&error),
                )
            }
        };
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: handle remains open, and counters points to its declared size.
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            process.0,
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    if ok.as_bool() {
        match u64::try_from(counters.WorkingSetSize) {
            Ok(value) => available(value, timestamps, TelemetrySource::Win32ProcessMemoryInfo),
            Err(_) => unavailable(
                timestamps,
                TelemetrySource::Win32ProcessMemoryInfo,
                UnavailableReason::InconsistentMeasurement,
            ),
        }
    } else {
        unavailable(
            timestamps,
            TelemetrySource::Win32ProcessMemoryInfo,
            win32_reason(&Error::from_win32()),
        )
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the non-null handle returned by OpenProcess.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn sample_nvml(
    selected: &SelectedAdapter,
    selected_game_pid: Option<u32>,
    timestamps: CapturedTimestamps,
) -> (Observation<u64>, Observation<u64>) {
    let library = match Nvml::load() {
        Ok(library) => library,
        Err(reason) => {
            return (
                unavailable(
                    timestamps,
                    TelemetrySource::NvmlDeviceMemoryInfo,
                    reason.clone(),
                ),
                unavailable(
                    timestamps,
                    TelemetrySource::NvmlRunningProcesses,
                    selected_game_pid.map_or(UnavailableReason::GameProcessNotSelected, |_| reason),
                ),
            )
        }
    };
    let device = match library.match_device(selected) {
        Ok(device) => device,
        Err(reason) => {
            return (
                unavailable(
                    timestamps,
                    TelemetrySource::NvmlDeviceMemoryInfo,
                    reason.clone(),
                ),
                unavailable(
                    timestamps,
                    TelemetrySource::NvmlRunningProcesses,
                    selected_game_pid.map_or(UnavailableReason::GameProcessNotSelected, |_| reason),
                ),
            )
        }
    };
    let total = match library.memory_info(device) {
        Ok(memory) if memory.total > 0 && memory.used <= memory.total => available(
            memory.used,
            timestamps,
            TelemetrySource::NvmlDeviceMemoryInfo,
        ),
        Ok(_) => unavailable(
            timestamps,
            TelemetrySource::NvmlDeviceMemoryInfo,
            UnavailableReason::InconsistentMeasurement,
        ),
        Err(reason) => unavailable(timestamps, TelemetrySource::NvmlDeviceMemoryInfo, reason),
    };
    let game = match selected_game_pid {
        Some(pid) => match library.graphics_process_memory(device, pid) {
            Ok(value) => available(value, timestamps, TelemetrySource::NvmlRunningProcesses),
            Err(reason) => unavailable(timestamps, TelemetrySource::NvmlRunningProcesses, reason),
        },
        None => unavailable(
            timestamps,
            TelemetrySource::NvmlRunningProcesses,
            UnavailableReason::GameProcessNotSelected,
        ),
    };
    (total, game)
}

type NvmlDevice = *mut c_void;
type NvmlReturn = u32;
type NvmlInit = unsafe extern "C" fn() -> NvmlReturn;
type NvmlShutdown = unsafe extern "C" fn() -> NvmlReturn;
type NvmlDeviceGetCount = unsafe extern "C" fn(*mut u32) -> NvmlReturn;
type NvmlDeviceGetHandleByIndex = unsafe extern "C" fn(u32, *mut NvmlDevice) -> NvmlReturn;
type NvmlDeviceGetName = unsafe extern "C" fn(NvmlDevice, *mut i8, u32) -> NvmlReturn;
type NvmlDeviceGetMemoryInfo = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> NvmlReturn;
type NvmlDeviceGetGraphicsRunningProcesses =
    unsafe extern "C" fn(NvmlDevice, *mut u32, *mut NvmlProcessInfo) -> NvmlReturn;

const NVML_SUCCESS: NvmlReturn = 0;
const NVML_ERROR_NOT_SUPPORTED: NvmlReturn = 3;
const NVML_ERROR_NO_PERMISSION: NvmlReturn = 4;
const NVML_ERROR_NOT_FOUND: NvmlReturn = 6;
const NVML_ERROR_INSUFFICIENT_SIZE: NvmlReturn = 7;
const NVML_VALUE_NOT_AVAILABLE: u64 = u64::MAX;
const MAX_NVML_DEVICES: u32 = 64;
const MAX_NVML_PROCESSES: u32 = 16_384;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvmlProcessInfo {
    pid: u32,
    used_gpu_memory: u64,
    gpu_instance_id: u32,
    compute_instance_id: u32,
}

struct Nvml {
    module: HMODULE,
    shutdown: NvmlShutdown,
    device_get_count: NvmlDeviceGetCount,
    device_get_handle_by_index: NvmlDeviceGetHandleByIndex,
    device_get_name: NvmlDeviceGetName,
    device_get_memory_info: NvmlDeviceGetMemoryInfo,
    device_get_graphics_processes: NvmlDeviceGetGraphicsRunningProcesses,
}

impl Nvml {
    fn load() -> Result<Self, UnavailableReason> {
        // SAFETY: the bare DLL name is searched only in System32, preventing
        // current-directory/PATH DLL preloading. The returned module is owned.
        let module = unsafe {
            LoadLibraryExW(
                w!("nvml.dll"),
                HANDLE::default(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }
        .map_err(|_| UnavailableReason::DriverLibraryNotFound)?;

        let loaded = (|| {
            // SAFETY: each symbol name and C ABI matches NVIDIA's documented NVML API.
            let init: NvmlInit = unsafe { load_symbol(module, s!("nvmlInit_v2")) }?;
            // SAFETY: documented NVML function signatures, as above.
            let shutdown = unsafe { load_symbol(module, s!("nvmlShutdown")) }?;
            // SAFETY: documented NVML function signatures, as above.
            let device_get_count = unsafe { load_symbol(module, s!("nvmlDeviceGetCount_v2")) }?;
            // SAFETY: documented NVML function signatures, as above.
            let device_get_handle_by_index =
                unsafe { load_symbol(module, s!("nvmlDeviceGetHandleByIndex_v2")) }?;
            // SAFETY: documented NVML function signatures, as above.
            let device_get_name = unsafe { load_symbol(module, s!("nvmlDeviceGetName")) }?;
            // SAFETY: v1 memory structure is stable and documented.
            let device_get_memory_info =
                unsafe { load_symbol(module, s!("nvmlDeviceGetMemoryInfo")) }?;
            // SAFETY: v3 process structure layout and symbol are documented.
            let device_get_graphics_processes =
                unsafe { load_symbol(module, s!("nvmlDeviceGetGraphicsRunningProcesses_v3")) }?;
            // SAFETY: init is a loaded NVML function with no pointer arguments.
            let result = unsafe { init() };
            if result != NVML_SUCCESS {
                return Err(nvml_reason(result));
            }
            Ok(Self {
                module,
                shutdown,
                device_get_count,
                device_get_handle_by_index,
                device_get_name,
                device_get_memory_info,
                device_get_graphics_processes,
            })
        })();
        if loaded.is_err() {
            // SAFETY: module was returned by LoadLibraryExW and has not been freed.
            let _ = unsafe { FreeLibrary(module) };
        }
        loaded
    }

    fn match_device(&self, selected: &SelectedAdapter) -> Result<NvmlDevice, UnavailableReason> {
        if selected.identity.vendor_id != NVIDIA_VENDOR_ID {
            return Err(UnavailableReason::AdapterDriverMismatch);
        }
        let mut count = 0_u32;
        // SAFETY: count is writable and the function pointer was validated at load.
        nvml_ok(unsafe { (self.device_get_count)(&mut count) })?;
        if count == 0 || count > MAX_NVML_DEVICES {
            return Err(UnavailableReason::AdapterDriverMismatch);
        }
        let expected_name = normalize_adapter_name(&selected.identity.description);
        let mut matches = Vec::new();
        for index in 0..count {
            let mut device = ptr::null_mut();
            // SAFETY: device is writable and index is below the reported count.
            nvml_ok(unsafe { (self.device_get_handle_by_index)(index, &mut device) })?;
            if device.is_null() {
                continue;
            }
            let name = self.device_name(device)?;
            if normalize_adapter_name(&name) == expected_name {
                matches.push(device);
            }
        }
        if matches.len() == 1 {
            Ok(matches[0])
        } else {
            Err(UnavailableReason::AdapterDriverMismatch)
        }
    }

    fn device_name(&self, device: NvmlDevice) -> Result<String, UnavailableReason> {
        let mut bytes = [0_i8; 128];
        // SAFETY: buffer is writable for its supplied length and device is from NVML.
        nvml_ok(unsafe { (self.device_get_name)(device, bytes.as_mut_ptr(), bytes.len() as u32) })?;
        let nul = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        let utf8: Vec<u8> = bytes[..nul].iter().map(|&byte| byte as u8).collect();
        String::from_utf8(utf8).map_err(|_| UnavailableReason::InconsistentMeasurement)
    }

    fn memory_info(&self, device: NvmlDevice) -> Result<NvmlMemory, UnavailableReason> {
        let mut memory = NvmlMemory::default();
        // SAFETY: memory is writable and device is from the initialized NVML library.
        nvml_ok(unsafe { (self.device_get_memory_info)(device, &mut memory) })?;
        Ok(memory)
    }

    fn graphics_process_memory(
        &self,
        device: NvmlDevice,
        pid: u32,
    ) -> Result<u64, UnavailableReason> {
        let mut count = 0_u32;
        // SAFETY: the null first-pass buffer is the documented NVML sizing call.
        let first =
            unsafe { (self.device_get_graphics_processes)(device, &mut count, ptr::null_mut()) };
        if first == NVML_SUCCESS && count == 0 {
            return Ok(0);
        }
        if first != NVML_ERROR_INSUFFICIENT_SIZE && first != NVML_SUCCESS {
            return Err(nvml_reason(first));
        }
        if count > MAX_NVML_PROCESSES {
            return Err(UnavailableReason::InconsistentMeasurement);
        }
        for _ in 0..2 {
            let capacity = count.saturating_add(16).min(MAX_NVML_PROCESSES);
            let mut processes = vec![NvmlProcessInfo::default(); capacity as usize];
            let mut written = capacity;
            // SAFETY: vector storage is writable for `written` entries and device is valid.
            let result = unsafe {
                (self.device_get_graphics_processes)(device, &mut written, processes.as_mut_ptr())
            };
            if result == NVML_ERROR_INSUFFICIENT_SIZE {
                if written > MAX_NVML_PROCESSES {
                    return Err(UnavailableReason::InconsistentMeasurement);
                }
                count = written;
                continue;
            }
            nvml_ok(result)?;
            processes.truncate((written as usize).min(processes.len()));
            if let Some(process) = processes.iter().find(|process| process.pid == pid) {
                return if process.used_gpu_memory == NVML_VALUE_NOT_AVAILABLE {
                    Err(UnavailableReason::DriverValueNotAvailable)
                } else {
                    Ok(process.used_gpu_memory)
                };
            }
            return Ok(0);
        }
        Err(UnavailableReason::DriverDoesNotSupportMetric)
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: shutdown belongs to the successful init call owned by this wrapper.
        let _ = unsafe { (self.shutdown)() };
        // SAFETY: module was loaded once and remains owned by this wrapper.
        let _ = unsafe { FreeLibrary(self.module) };
    }
}

unsafe fn load_symbol<T: Copy>(module: HMODULE, name: PCSTR) -> Result<T, UnavailableReason> {
    // SAFETY: caller guarantees module is live and `name` is a NUL-terminated static symbol.
    let symbol =
        unsafe { GetProcAddress(module, name) }.ok_or(UnavailableReason::DriverSymbolMissing)?;
    if size_of::<T>() != size_of_val(&symbol) {
        return Err(UnavailableReason::DriverSymbolMissing);
    }
    // SAFETY: the caller chooses T to match the documented symbol ABI and we checked size.
    Ok(unsafe { std::mem::transmute_copy(&symbol) })
}

fn nvml_ok(result: NvmlReturn) -> Result<(), UnavailableReason> {
    if result == NVML_SUCCESS {
        Ok(())
    } else {
        Err(nvml_reason(result))
    }
}

fn nvml_reason(result: NvmlReturn) -> UnavailableReason {
    match result {
        NVML_ERROR_NOT_SUPPORTED => UnavailableReason::DriverDoesNotSupportMetric,
        NVML_ERROR_NO_PERMISSION => UnavailableReason::PermissionDenied,
        NVML_ERROR_NOT_FOUND => UnavailableReason::AdapterDriverMismatch,
        code => UnavailableReason::DriverError { code },
    }
}

fn win32_reason(error: &Error) -> UnavailableReason {
    let code = error.code().0;
    match code as u32 & 0xffff {
        5 => UnavailableReason::PermissionDenied,
        87 | 1168 => UnavailableReason::ProcessNotFound,
        _ => UnavailableReason::OsError { code },
    }
}

fn utf16_nul_terminated(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end]).trim().to_owned()
}

fn normalize_adapter_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_name_matching_is_case_and_punctuation_insensitive() {
        assert_eq!(
            normalize_adapter_name("NVIDIA GeForce RTX 4080 Laptop GPU"),
            normalize_adapter_name("nvidia geforce rtx-4080 laptop gpu")
        );
    }

    #[test]
    fn live_probe_reports_valid_timestamped_windows_observations() {
        let snapshot = collect(
            TelemetryRequest {
                selected_game_pid: Some(std::process::id()),
                adapter: AdapterSelector::default(),
            },
            crate::capture_timestamps(),
        );
        snapshot
            .validate()
            .expect("live Windows telemetry must be structurally valid");
        assert!(
            snapshot
                .physical_ram_bytes
                .value()
                .copied()
                .unwrap_or_default()
                > 0
        );
        assert!(
            snapshot
                .available_ram_bytes
                .value()
                .copied()
                .unwrap_or_default()
                > 0
        );
        assert!(snapshot.adapter.value().is_some());
        assert!(
            snapshot
                .dedicated_vram_bytes
                .value()
                .copied()
                .unwrap_or_default()
                > 0
        );
        assert!(
            snapshot
                .os_local_vram_budget_bytes
                .value()
                .copied()
                .unwrap_or_default()
                > 0
        );
        assert!(snapshot.selected_game_working_set_bytes.value().is_some());
    }

    #[test]
    fn impossible_pid_is_not_reported_as_zero_working_set() {
        let timestamps = crate::capture_timestamps();
        let observation = sample_process_working_set(u32::MAX, timestamps);
        assert!(observation.value().is_none());
    }
}
