use crate::{LiveResourceSnapshotV1, ResourceAdmissionError, Sha256Digest};
use npc_system_telemetry::AdmissionResourceViewV1;

impl TryFrom<AdmissionResourceViewV1> for LiveResourceSnapshotV1 {
    type Error = ResourceAdmissionError;

    fn try_from(view: AdmissionResourceViewV1) -> Result<Self, Self::Error> {
        let device_fingerprint_sha256 = Sha256Digest::parse(view.device_fingerprint_sha256)
            .map_err(|_| ResourceAdmissionError::InvalidSnapshot)?;
        Ok(Self {
            captured_monotonic_millis: view.captured_monotonic_millis,
            device_fingerprint_sha256,
            physical_vram_bytes: view.physical_vram_bytes,
            os_vram_budget_bytes: view.os_vram_budget_bytes,
            desktop_resident_vram_bytes: view.desktop_resident_vram_bytes,
            game_resident_vram_bytes: view.game_resident_vram_bytes,
            game_reserve_vram_bytes: view.configured_game_reserve_vram_bytes,
            physical_ram_bytes: view.physical_ram_bytes,
            available_ram_bytes: view.available_ram_bytes,
            game_additional_reserve_ram_bytes: view.game_additional_reserve_ram_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_bridge_preserves_split_game_pressure_without_double_counting() {
        let source = AdmissionResourceViewV1 {
            captured_monotonic_millis: 19,
            device_fingerprint_sha256: "a".repeat(64),
            physical_vram_bytes: 12_000,
            os_vram_budget_bytes: 10_000,
            desktop_resident_vram_bytes: 2_000,
            game_resident_vram_bytes: 3_000,
            configured_game_reserve_vram_bytes: 5_000,
            physical_ram_bytes: 32_000,
            available_ram_bytes: 16_000,
            game_working_set_bytes: 4_000,
            game_additional_reserve_ram_bytes: 1_000,
        };
        let snapshot = LiveResourceSnapshotV1::try_from(source).expect("valid bridge");
        assert_eq!(snapshot.desktop_resident_vram_bytes, 2_000);
        assert_eq!(snapshot.game_resident_vram_bytes, 3_000);
        assert_eq!(snapshot.game_reserve_vram_bytes, 5_000);
    }
}
