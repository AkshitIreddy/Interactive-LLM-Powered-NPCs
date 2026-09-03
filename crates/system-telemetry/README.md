# System telemetry

`npc-system-telemetry` samples the live Windows resource facts used before a
local-model loadout is admitted. Its public contract deliberately distinguishes
an unavailable observation from a measured zero.

The Windows implementation uses:

- `GlobalMemoryStatusEx` for physical and available RAM;
- DXGI adapter descriptions and `IDXGIAdapter3::QueryVideoMemoryInfo` for the
  adapter identity, physical dedicated VRAM, the current process's OS budget,
  and the current process's local-memory usage;
- `K32GetProcessMemoryInfo` for the selected game process working set; and
- an optional, safely loaded `nvml.dll` from `System32` for device-wide NVIDIA
  memory pressure and per-process VRAM when the driver exposes those values.

The crate never invokes `nvidia-smi`, never requires elevation, and never
substitutes a guessed value. NVML reports per-process VRAM as unavailable on
many Windows WDDM configurations; that is preserved as `Unavailable`, not zero.

## Admission semantics

`ResourceTelemetrySnapshotV1::admission_view` returns normalized inputs with
`desktop_resident_vram_bytes` and `game_resident_vram_bytes` split apart. The
model manager should protect:

```text
desktop_resident + max(game_resident, configured_game_reserve)
```

This prevents the currently measured game allocation from being counted both
inside total device pressure and again as the configured game reserve. If a
selected game's VRAM cannot be measured, the conversion fails closed.

The view uses the same resource field names as
`model_manager::LiveResourceSnapshotV1` and carries a canonical 64-digit
hardware fingerprint. Model Manager owns the small `TryFrom` bridge, so the
telemetry crate stays independent of admission policy.
