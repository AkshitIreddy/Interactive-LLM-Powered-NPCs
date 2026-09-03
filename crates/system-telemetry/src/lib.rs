//! Timestamped, fail-closed resource telemetry for local model admission.
//!
//! Zero is a valid measurement for process usage. Missing permission, missing
//! APIs, unsupported drivers, and unsupported platforms remain explicit
//! [`Observation::Unavailable`] values and are never silently converted to zero.

mod validation;

#[cfg(windows)]
mod windows;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::OnceLock,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

pub use validation::{AdmissionResourceViewV1, SnapshotValidationError};

pub const RESOURCE_TELEMETRY_SCHEMA_V1: &str = "npc.system-telemetry/resource-snapshot-v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetrySource {
    Win32GlobalMemoryStatusEx,
    DxgiAdapterDescription,
    DxgiProcessVideoMemoryInfo,
    Win32ProcessMemoryInfo,
    NvmlDeviceMemoryInfo,
    NvmlRunningProcesses,
    UnsupportedPlatform,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObservationProvenance {
    pub captured_unix_millis: u64,
    pub captured_monotonic_millis: u64,
    pub source: TelemetrySource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    UnsupportedPlatform,
    GameProcessNotSelected,
    ApiUnavailable,
    PermissionDenied,
    ProcessNotFound,
    AdapterNotFound,
    AdapterDoesNotSupportBudgetQuery,
    DriverLibraryNotFound,
    DriverSymbolMissing,
    DriverNotInitialized,
    DriverDoesNotSupportMetric,
    DriverValueNotAvailable,
    AdapterDriverMismatch,
    InconsistentMeasurement,
    OsError { code: i32 },
    DriverError { code: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum Observation<T> {
    Available {
        value: T,
        provenance: ObservationProvenance,
    },
    Unavailable {
        reason: UnavailableReason,
        provenance: ObservationProvenance,
    },
}

impl<T> Observation<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Available { value, .. } => Some(value),
            Self::Unavailable { .. } => None,
        }
    }

    pub fn provenance(&self) -> &ObservationProvenance {
        match self {
            Self::Available { provenance, .. } | Self::Unavailable { provenance, .. } => provenance,
        }
    }

    pub fn unavailable_reason(&self) -> Option<&UnavailableReason> {
        match self {
            Self::Available { .. } => None,
            Self::Unavailable { reason, .. } => Some(reason),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct AdapterLuid {
    pub low_part: u32,
    pub high_part: i32,
}

impl AdapterLuid {
    /// Windows and NVML expose the LUID as the same native 8-byte value.
    pub fn native_bytes(self) -> [u8; 8] {
        let mut result = [0_u8; 8];
        result[..4].copy_from_slice(&self.low_part.to_ne_bytes());
        result[4..].copy_from_slice(&self.high_part.to_ne_bytes());
        result
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GraphicsAdapterIdentity {
    pub description: String,
    pub luid: AdapterLuid,
    pub vendor_id: u32,
    pub device_id: u32,
    pub subsystem_id: u32,
    pub revision: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterSelector {
    /// Select the non-software DXGI adapter with the greatest dedicated VRAM.
    #[default]
    LargestDedicatedMemory,
    ExactLuid(AdapterLuid),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TelemetryRequest {
    pub selected_game_pid: Option<u32>,
    pub adapter: AdapterSelector,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResourceTelemetrySnapshotV1 {
    pub schema: String,
    pub captured_unix_millis: u64,
    pub captured_monotonic_millis: u64,
    pub selected_game_pid: Option<u32>,
    pub physical_ram_bytes: Observation<u64>,
    pub available_ram_bytes: Observation<u64>,
    pub adapter: Observation<GraphicsAdapterIdentity>,
    /// Stable hardware fingerprint over vendor/device/subsystem/revision and
    /// dedicated memory. The transient adapter LUID is intentionally excluded.
    pub device_fingerprint_sha256: Observation<String>,
    pub dedicated_vram_bytes: Observation<u64>,
    /// OS budget assigned to this process for the adapter's local segment.
    pub os_local_vram_budget_bytes: Observation<u64>,
    /// This process's current local-segment usage, not system-wide usage.
    pub current_process_local_vram_bytes: Observation<u64>,
    /// Device-wide used VRAM. This may include driver/system reservations.
    pub total_device_pressure_vram_bytes: Observation<u64>,
    pub selected_game_working_set_bytes: Observation<u64>,
    pub selected_game_vram_bytes: Observation<u64>,
}

pub fn device_fingerprint_sha256(
    identity: &GraphicsAdapterIdentity,
    dedicated_vram_bytes: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"npc.system-telemetry/device-fingerprint-v1\0");
    hasher.update(identity.vendor_id.to_le_bytes());
    hasher.update(identity.device_id.to_le_bytes());
    hasher.update(identity.subsystem_id.to_le_bytes());
    hasher.update(identity.revision.to_le_bytes());
    hasher.update(dedicated_vram_bytes.to_le_bytes());
    hex::encode(hasher.finalize())
}

impl ResourceTelemetrySnapshotV1 {
    pub fn validate(&self) -> Result<(), SnapshotValidationError> {
        validation::validate_snapshot(self)
    }

    /// Normalize telemetry for model admission without counting the game's live
    /// VRAM both in total device pressure and in its configured reserve.
    pub fn admission_view(
        &self,
        configured_game_reserve_vram_bytes: u64,
        game_additional_reserve_ram_bytes: u64,
    ) -> Result<AdmissionResourceViewV1, AdmissionViewError> {
        validation::admission_view(
            self,
            configured_game_reserve_vram_bytes,
            game_additional_reserve_ram_bytes,
        )
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AdmissionViewError {
    #[error("resource telemetry snapshot is invalid: {0}")]
    InvalidSnapshot(#[from] SnapshotValidationError),
    #[error("required admission metric is unavailable: {0}")]
    RequiredMetricUnavailable(&'static str),
    #[error("device-wide VRAM pressure is smaller than selected-game VRAM")]
    GameVramExceedsTotalPressure,
}

pub fn collect(request: TelemetryRequest) -> ResourceTelemetrySnapshotV1 {
    let timestamps = capture_timestamps();
    #[cfg(windows)]
    {
        windows::collect(request, timestamps)
    }
    #[cfg(not(windows))]
    {
        unsupported_snapshot(request, timestamps)
    }
}

#[derive(Clone, Copy)]
struct CapturedTimestamps {
    unix_millis: u64,
    monotonic_millis: u64,
}

impl CapturedTimestamps {
    fn provenance(self, source: TelemetrySource) -> ObservationProvenance {
        ObservationProvenance {
            captured_unix_millis: self.unix_millis,
            captured_monotonic_millis: self.monotonic_millis,
            source,
        }
    }
}

fn capture_timestamps() -> CapturedTimestamps {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let elapsed = ORIGIN.get_or_init(Instant::now).elapsed().as_millis();
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    CapturedTimestamps {
        unix_millis: u64::try_from(unix).unwrap_or(u64::MAX),
        // Admission snapshots use zero as an invalid/uninitialized sentinel.
        monotonic_millis: u64::try_from(elapsed).unwrap_or(u64::MAX).max(1),
    }
}

fn unavailable<T>(
    timestamps: CapturedTimestamps,
    source: TelemetrySource,
    reason: UnavailableReason,
) -> Observation<T> {
    Observation::Unavailable {
        reason,
        provenance: timestamps.provenance(source),
    }
}

#[cfg(not(windows))]
fn unsupported_snapshot(
    request: TelemetryRequest,
    timestamps: CapturedTimestamps,
) -> ResourceTelemetrySnapshotV1 {
    let metric = || {
        unavailable(
            timestamps,
            TelemetrySource::UnsupportedPlatform,
            UnavailableReason::UnsupportedPlatform,
        )
    };
    ResourceTelemetrySnapshotV1 {
        schema: RESOURCE_TELEMETRY_SCHEMA_V1.to_owned(),
        captured_unix_millis: timestamps.unix_millis,
        captured_monotonic_millis: timestamps.monotonic_millis,
        selected_game_pid: request.selected_game_pid,
        physical_ram_bytes: metric(),
        available_ram_bytes: metric(),
        adapter: metric(),
        device_fingerprint_sha256: metric(),
        dedicated_vram_bytes: metric(),
        os_local_vram_budget_bytes: metric(),
        current_process_local_vram_bytes: metric(),
        total_device_pressure_vram_bytes: metric(),
        selected_game_working_set_bytes: metric(),
        selected_game_vram_bytes: metric(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_never_turns_missing_metrics_into_zero() {
        #[cfg(not(windows))]
        {
            let snapshot = collect(TelemetryRequest::default());
            assert_eq!(snapshot.physical_ram_bytes.value(), None);
            assert_eq!(
                snapshot.physical_ram_bytes.unavailable_reason(),
                Some(&UnavailableReason::UnsupportedPlatform)
            );
            assert!(snapshot.captured_unix_millis > 0);
            assert!(snapshot.captured_monotonic_millis > 0);
        }
    }

    #[test]
    fn luid_native_bytes_round_trip_parts() {
        let luid = AdapterLuid {
            low_part: 0x0403_0201,
            high_part: 0x0807_0605,
        };
        let bytes = luid.native_bytes();
        assert_eq!(
            u32::from_ne_bytes(bytes[..4].try_into().expect("four bytes")),
            luid.low_part
        );
        assert_eq!(
            i32::from_ne_bytes(bytes[4..].try_into().expect("four bytes")),
            luid.high_part
        );
    }

    #[test]
    fn device_fingerprint_is_stable_across_transient_luid_changes() {
        let mut identity = GraphicsAdapterIdentity {
            description: "NVIDIA GPU".to_owned(),
            luid: AdapterLuid {
                low_part: 1,
                high_part: 2,
            },
            vendor_id: 0x10de,
            device_id: 10,
            subsystem_id: 11,
            revision: 12,
        };
        let before = device_fingerprint_sha256(&identity, 12_000);
        identity.luid.low_part = 999;
        assert_eq!(device_fingerprint_sha256(&identity, 12_000), before);
        identity.device_id += 1;
        assert_ne!(device_fingerprint_sha256(&identity, 12_000), before);
    }
}
