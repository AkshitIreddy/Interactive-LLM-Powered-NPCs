use crate::{
    AdmissionViewError, Observation, ResourceTelemetrySnapshotV1, TelemetrySource,
    RESOURCE_TELEMETRY_SCHEMA_V1,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdmissionResourceViewV1 {
    pub captured_monotonic_millis: u64,
    pub device_fingerprint_sha256: String,
    pub physical_vram_bytes: u64,
    pub os_vram_budget_bytes: u64,
    /// Total device pressure with the selected game's measured allocation removed.
    pub desktop_resident_vram_bytes: u64,
    pub game_resident_vram_bytes: u64,
    pub configured_game_reserve_vram_bytes: u64,
    pub physical_ram_bytes: u64,
    pub available_ram_bytes: u64,
    pub game_working_set_bytes: u64,
    pub game_additional_reserve_ram_bytes: u64,
}

impl AdmissionResourceViewV1 {
    pub fn protected_desktop_and_game_vram_bytes(&self) -> Option<u64> {
        self.desktop_resident_vram_bytes.checked_add(
            self.game_resident_vram_bytes
                .max(self.configured_game_reserve_vram_bytes),
        )
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SnapshotValidationError {
    #[error("unsupported resource telemetry schema: {0}")]
    UnsupportedSchema(String),
    #[error("snapshot timestamp is zero")]
    MissingTimestamp,
    #[error("observation provenance does not match snapshot timestamps")]
    ProvenanceTimestampMismatch,
    #[error("observation provenance source does not match the metric")]
    ProvenanceSourceMismatch,
    #[error("physical or budget capacity is zero")]
    ZeroCapacity,
    #[error("available RAM exceeds physical RAM")]
    AvailableRamExceedsPhysical,
    #[error("OS VRAM budget exceeds dedicated VRAM")]
    VramBudgetExceedsDedicated,
    #[error("selected-game metrics are available without a selected PID")]
    GameMetricWithoutPid,
}

pub(crate) fn validate_snapshot(
    snapshot: &ResourceTelemetrySnapshotV1,
) -> Result<(), SnapshotValidationError> {
    if snapshot.schema != RESOURCE_TELEMETRY_SCHEMA_V1 {
        return Err(SnapshotValidationError::UnsupportedSchema(
            snapshot.schema.clone(),
        ));
    }
    if snapshot.captured_unix_millis == 0 || snapshot.captured_monotonic_millis == 0 {
        return Err(SnapshotValidationError::MissingTimestamp);
    }
    let observations = [
        observation_timestamps(&snapshot.physical_ram_bytes),
        observation_timestamps(&snapshot.available_ram_bytes),
        observation_timestamps(&snapshot.adapter),
        observation_timestamps(&snapshot.device_fingerprint_sha256),
        observation_timestamps(&snapshot.dedicated_vram_bytes),
        observation_timestamps(&snapshot.os_local_vram_budget_bytes),
        observation_timestamps(&snapshot.current_process_local_vram_bytes),
        observation_timestamps(&snapshot.total_device_pressure_vram_bytes),
        observation_timestamps(&snapshot.selected_game_working_set_bytes),
        observation_timestamps(&snapshot.selected_game_vram_bytes),
    ];
    if observations.iter().any(|&(unix, monotonic)| {
        unix != snapshot.captured_unix_millis || monotonic != snapshot.captured_monotonic_millis
    }) {
        return Err(SnapshotValidationError::ProvenanceTimestampMismatch);
    }
    validate_source(
        &snapshot.physical_ram_bytes,
        TelemetrySource::Win32GlobalMemoryStatusEx,
    )?;
    validate_source(
        &snapshot.available_ram_bytes,
        TelemetrySource::Win32GlobalMemoryStatusEx,
    )?;
    validate_source(&snapshot.adapter, TelemetrySource::DxgiAdapterDescription)?;
    validate_source(
        &snapshot.device_fingerprint_sha256,
        TelemetrySource::DxgiAdapterDescription,
    )?;
    validate_source(
        &snapshot.dedicated_vram_bytes,
        TelemetrySource::DxgiAdapterDescription,
    )?;
    validate_source(
        &snapshot.os_local_vram_budget_bytes,
        TelemetrySource::DxgiProcessVideoMemoryInfo,
    )?;
    validate_source(
        &snapshot.current_process_local_vram_bytes,
        TelemetrySource::DxgiProcessVideoMemoryInfo,
    )?;
    validate_source(
        &snapshot.total_device_pressure_vram_bytes,
        TelemetrySource::NvmlDeviceMemoryInfo,
    )?;
    validate_source(
        &snapshot.selected_game_working_set_bytes,
        TelemetrySource::Win32ProcessMemoryInfo,
    )?;
    validate_source(
        &snapshot.selected_game_vram_bytes,
        TelemetrySource::NvmlRunningProcesses,
    )?;
    if snapshot.physical_ram_bytes.value() == Some(&0)
        || snapshot.dedicated_vram_bytes.value() == Some(&0)
        || snapshot.os_local_vram_budget_bytes.value() == Some(&0)
    {
        return Err(SnapshotValidationError::ZeroCapacity);
    }
    if let (Some(available), Some(physical)) = (
        snapshot.available_ram_bytes.value(),
        snapshot.physical_ram_bytes.value(),
    ) {
        if available > physical {
            return Err(SnapshotValidationError::AvailableRamExceedsPhysical);
        }
    }
    if let (Some(budget), Some(dedicated)) = (
        snapshot.os_local_vram_budget_bytes.value(),
        snapshot.dedicated_vram_bytes.value(),
    ) {
        if budget > dedicated {
            return Err(SnapshotValidationError::VramBudgetExceedsDedicated);
        }
    }
    if snapshot.selected_game_pid.is_none()
        && (snapshot.selected_game_working_set_bytes.value().is_some()
            || snapshot.selected_game_vram_bytes.value().is_some())
    {
        return Err(SnapshotValidationError::GameMetricWithoutPid);
    }
    Ok(())
}

pub(crate) fn admission_view(
    snapshot: &ResourceTelemetrySnapshotV1,
    configured_game_reserve_vram_bytes: u64,
    game_additional_reserve_ram_bytes: u64,
) -> Result<AdmissionResourceViewV1, AdmissionViewError> {
    snapshot.validate()?;
    let device_fingerprint = snapshot
        .device_fingerprint_sha256
        .value()
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .cloned()
        .ok_or(AdmissionViewError::RequiredMetricUnavailable(
            "device_fingerprint_sha256",
        ))?;
    let physical_vram = required(&snapshot.dedicated_vram_bytes, "dedicated_vram_bytes")?;
    let budget = required(
        &snapshot.os_local_vram_budget_bytes,
        "os_local_vram_budget_bytes",
    )?;
    let total_pressure = required(
        &snapshot.total_device_pressure_vram_bytes,
        "total_device_pressure_vram_bytes",
    )?;
    let physical_ram = required(&snapshot.physical_ram_bytes, "physical_ram_bytes")?;
    let available_ram = required(&snapshot.available_ram_bytes, "available_ram_bytes")?;

    let (game_vram, game_working_set) = if snapshot.selected_game_pid.is_some() {
        (
            required(
                &snapshot.selected_game_vram_bytes,
                "selected_game_vram_bytes",
            )?,
            required(
                &snapshot.selected_game_working_set_bytes,
                "selected_game_working_set_bytes",
            )?,
        )
    } else {
        (0, 0)
    };
    let non_game_pressure = total_pressure
        .checked_sub(game_vram)
        .ok_or(AdmissionViewError::GameVramExceedsTotalPressure)?;
    Ok(AdmissionResourceViewV1 {
        captured_monotonic_millis: snapshot.captured_monotonic_millis,
        device_fingerprint_sha256: device_fingerprint,
        physical_vram_bytes: physical_vram,
        os_vram_budget_bytes: budget,
        desktop_resident_vram_bytes: non_game_pressure,
        game_resident_vram_bytes: game_vram,
        configured_game_reserve_vram_bytes,
        physical_ram_bytes: physical_ram,
        available_ram_bytes: available_ram,
        game_working_set_bytes: game_working_set,
        game_additional_reserve_ram_bytes,
    })
}

fn observation_timestamps<T>(observation: &Observation<T>) -> (u64, u64) {
    let provenance = observation.provenance();
    (
        provenance.captured_unix_millis,
        provenance.captured_monotonic_millis,
    )
}

fn validate_source<T>(
    observation: &Observation<T>,
    expected: TelemetrySource,
) -> Result<(), SnapshotValidationError> {
    let actual = &observation.provenance().source;
    if actual == &expected || actual == &TelemetrySource::UnsupportedPlatform {
        Ok(())
    } else {
        Err(SnapshotValidationError::ProvenanceSourceMismatch)
    }
}

fn required(observation: &Observation<u64>, name: &'static str) -> Result<u64, AdmissionViewError> {
    observation
        .value()
        .copied()
        .ok_or(AdmissionViewError::RequiredMetricUnavailable(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AdapterLuid, GraphicsAdapterIdentity, ObservationProvenance, TelemetrySource,
        UnavailableReason,
    };

    fn provenance(source: TelemetrySource) -> ObservationProvenance {
        ObservationProvenance {
            captured_unix_millis: 50,
            captured_monotonic_millis: 10,
            source,
        }
    }

    fn available<T>(value: T, source: TelemetrySource) -> Observation<T> {
        Observation::Available {
            value,
            provenance: provenance(source),
        }
    }

    fn snapshot() -> ResourceTelemetrySnapshotV1 {
        ResourceTelemetrySnapshotV1 {
            schema: RESOURCE_TELEMETRY_SCHEMA_V1.to_owned(),
            captured_unix_millis: 50,
            captured_monotonic_millis: 10,
            selected_game_pid: Some(91),
            physical_ram_bytes: available(32_000, TelemetrySource::Win32GlobalMemoryStatusEx),
            available_ram_bytes: available(12_000, TelemetrySource::Win32GlobalMemoryStatusEx),
            adapter: available(
                GraphicsAdapterIdentity {
                    description: "GPU".to_owned(),
                    luid: AdapterLuid {
                        low_part: 7,
                        high_part: 8,
                    },
                    vendor_id: 0x10de,
                    device_id: 1,
                    subsystem_id: 2,
                    revision: 3,
                },
                TelemetrySource::DxgiAdapterDescription,
            ),
            device_fingerprint_sha256: available(
                "a".repeat(64),
                TelemetrySource::DxgiAdapterDescription,
            ),
            dedicated_vram_bytes: available(12_000, TelemetrySource::DxgiAdapterDescription),
            os_local_vram_budget_bytes: available(
                10_000,
                TelemetrySource::DxgiProcessVideoMemoryInfo,
            ),
            current_process_local_vram_bytes: available(
                0,
                TelemetrySource::DxgiProcessVideoMemoryInfo,
            ),
            total_device_pressure_vram_bytes: available(
                6_000,
                TelemetrySource::NvmlDeviceMemoryInfo,
            ),
            selected_game_working_set_bytes: available(
                1_000,
                TelemetrySource::Win32ProcessMemoryInfo,
            ),
            selected_game_vram_bytes: available(2_000, TelemetrySource::NvmlRunningProcesses),
        }
    }

    #[test]
    fn zero_usage_is_valid_and_distinct_from_unavailable() {
        let mut value = snapshot();
        value.selected_game_vram_bytes = available(0, TelemetrySource::NvmlRunningProcesses);
        assert_eq!(value.selected_game_vram_bytes.value(), Some(&0));
        value.selected_game_vram_bytes = Observation::Unavailable {
            reason: UnavailableReason::DriverValueNotAvailable,
            provenance: provenance(TelemetrySource::NvmlRunningProcesses),
        };
        assert_eq!(value.selected_game_vram_bytes.value(), None);
        assert!(value.validate().is_ok());
    }

    #[test]
    fn admission_view_subtracts_game_once_then_applies_reserve_floor() {
        let view = snapshot().admission_view(4_000, 900).expect("valid view");
        assert_eq!(view.desktop_resident_vram_bytes, 4_000);
        assert_eq!(view.game_resident_vram_bytes, 2_000);
        assert_eq!(view.protected_desktop_and_game_vram_bytes(), Some(8_000));
    }

    #[test]
    fn admission_fails_closed_when_selected_game_vram_is_unknown() {
        let mut value = snapshot();
        value.selected_game_vram_bytes = Observation::Unavailable {
            reason: UnavailableReason::DriverValueNotAvailable,
            provenance: provenance(TelemetrySource::NvmlRunningProcesses),
        };
        assert_eq!(
            value.admission_view(4_000, 900),
            Err(AdmissionViewError::RequiredMetricUnavailable(
                "selected_game_vram_bytes"
            ))
        );
    }

    #[test]
    fn admission_rejects_inconsistent_game_greater_than_total() {
        let mut value = snapshot();
        value.selected_game_vram_bytes = available(7_000, TelemetrySource::NvmlRunningProcesses);
        assert_eq!(
            value.admission_view(4_000, 900),
            Err(AdmissionViewError::GameVramExceedsTotalPressure)
        );
    }

    #[test]
    fn validation_rejects_mismatched_observation_timestamp() {
        let mut value = snapshot();
        value.available_ram_bytes = Observation::Available {
            value: 2,
            provenance: ObservationProvenance {
                captured_unix_millis: 51,
                captured_monotonic_millis: 10,
                source: TelemetrySource::Win32GlobalMemoryStatusEx,
            },
        };
        assert_eq!(
            value.validate(),
            Err(SnapshotValidationError::ProvenanceTimestampMismatch)
        );
    }

    #[test]
    fn validation_rejects_a_spoofed_metric_source() {
        let mut value = snapshot();
        value.total_device_pressure_vram_bytes =
            available(6_000, TelemetrySource::DxgiProcessVideoMemoryInfo);
        assert_eq!(
            value.validate(),
            Err(SnapshotValidationError::ProvenanceSourceMismatch)
        );
    }
}
