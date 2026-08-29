# Synthetic game-load harness

This Windows-only D3D12 tool creates a deterministic, visible game-like scene and
bounded CPU, RAM, GPU-compute, and VRAM pressure. It exists to measure Response
Console behavior under repeatable background load without starting or automating a
commercial game.

The executable is deliberately inert by default. Running `game-load.exe` prints a
dry-run plan. It performs live work only when `--run` is present.

## Power-profile contract

The harness never reads, changes, or restores G-Helper modes, CPU boost, fan curves,
Windows power plans, GPU modes, or clock settings. There is no code path or linked
API for doing so. `--power-profile-metadata` records an arbitrary user-supplied label
in the JSON result, for example:

```powershell
game-load.exe --profile typical --run `
  --power-profile-metadata "G-Helper Silent; CPU boost disabled; other agents active"
```

Treat results recorded while unrelated workloads are active as *scenario results*,
not canonical hardware benchmarks. For a release benchmark, close unrelated work,
allow temperatures to settle, record the chosen profile, and run at least three
trials. CPU boost does not need to remain enabled for development or functional
tests; only use a consistent, explicitly recorded configuration when comparing
performance numbers.

## Safety behavior

- Dry-run is the default; `--run` is mandatory for pressure.
- VRAM requests fail closed if `IDXGIAdapter3::QueryVideoMemoryInfo` is unavailable.
- Allocations are reduced or denied to preserve both an absolute headroom reserve
  and a maximum fraction of the current DXGI budget.
- RAM is committed and page-touched only after commit-limit and available-physical
  headroom checks. It is released on every normal or exceptional exit.
- CPU workers run below normal priority, leave logical processors free by default,
  probe cancellation frequently, and cap requested duty at 90%.
- GPU compute is calibrated from D3D12 timestamp queries toward a bounded fraction
  of the target frame budget; a per-frame controller limits changes to 25%.
- Escape and window close are immediate stop inputs.
- The loop aborts on ACPI thermal limit (when readable), required-but-missing thermal
  sensors, low system commit, low physical memory, low DXGI budget, D3D device
  removal/reset, or a bounded fence timeout.
- Heavy mode requires a readable ACPI thermal sensor before pressure starts. Other
  modes are short and use best-effort thermal monitoring by default because many
  laptop firmware implementations do not expose a usable ACPI WMI zone.
- The process does not raise thread/process priority, request high-performance power
  policy, overclock, disable TDR, or attempt to recover a removed GPU by retrying load.

Synthetic load is still real load. Save work before a live run and use dry-run while
other intensive jobs are active.

## Profiles

| Profile | Scene | GPU duty | CPU duty | RAM request | VRAM request | Thermal policy |
|---|---:|---:|---:|---:|---:|---|
| `idle` | 1280×720 | 3% | 0% | 0 MiB | 0 MiB | best effort |
| `typical` | 1920×1080 | 35% | 25% | 512 MiB | 1024 MiB | best effort |
| `heavy` | 2560×1440 | 65% | 60% | 1536 MiB | 3072 MiB | sensor required |
| `constrained` | 1280×720 | 20% | 15% | 256 MiB | 384 MiB | best effort, larger reserves |

Requests are ceilings, not promises. The live allocation decision is recomputed from
current memory state and recorded alongside requested, approved, and committed bytes.

## Build and test

Requirements: Windows 10 22H2 or Windows 11, Visual Studio 2022 with Desktop C++, the
Windows 10/11 SDK, and CMake 3.24+.

```powershell
./tools/game-load/scripts/smoke.ps1
```

The Windows wrapper keeps Visual Studio intermediates in a repository-keyed short
path beneath `%LOCALAPPDATA%\InteractiveNPCs\build\gl`. This prevents FileTracker
path failures in deeply nested clean checkouts and keeps generated files out of the
source tree. The `portable-tests` preset remains available on non-Windows hosts.

The policy library and tests contain no Windows headers and can be built on any C++20
host with CMake/Ninja using `portable-tests`. This exercises profile defaults,
allocation clamps, safety abort precedence, calibration bounds, statistics, and
configuration validation without applying load.

The Windows smoke script builds, runs tests, then verifies a dry-run manifest:

```powershell
./tools/game-load/scripts/smoke.ps1
```

It does not apply load. `./tools/game-load/scripts/smoke.ps1 -Live` additionally performs a visible,
three-second idle run and should only be used intentionally.

## Examples

```powershell
# Inspect the typical plan; no D3D12 device or workload is created.
game-load.exe --profile typical

# Write a machine-readable dry-run manifest.
game-load.exe --profile constrained --dry-run --output out/constrained-plan.json

# Explicit live scenario on the current silent profile.
game-load.exe --profile typical --run --duration-seconds 30 `
  --power-profile-metadata "G-Helper Silent; boost disabled" `
  --label "hybrid conversation under silent-mode pressure" `
  --output out/silent-typical.json

# Carefully customized lower-pressure run.
game-load.exe --profile constrained --run --gpu-duty 15 --cpu-duty 10 `
  --ram-mib 128 --vram-mib 256 --temperature-limit-c 82 `
  --output out/constrained-custom.json
```

Run `game-load.exe --help` for every bound and override.

## Measurements and PresentMon

Each frame uses QPC around command recording/queue/Present and four GPU timestamp
queries around the full GPU frame and compute dispatch. The JSON stores min, mean,
p50, p95, p99, and max summaries. PresentMon can observe the real flip-model swap
chain directly. The tool also emits `EventWriteString` markers under the stable ETW
provider `{2D7F3C2B-337D-4F6A-89E7-97B31C4D4B5A}` for workload, frame, and safety
boundaries, and names CPU pressure threads `GameLoadHarness.CpuPressure`.

The scene evolves from a fixed frame index and fixed shader constants, so identical
settings have identical submitted content. Wall-clock scheduling, temperatures,
driver behavior, and competing processes remain real environmental variables.

## Result manifest

Live runs always write a JSON result (`game-load-result.json` unless `--output` is
specified). Writes use a temporary sibling and atomic replacement. The manifest
includes:

- exact requested configuration and user-supplied power-profile metadata;
- the invariant `settings_changed_by_harness: false`;
- OS, CPU, logical processor count, adapter identity, driver and timestamp frequency;
- initial commit/physical/DXGI budgets;
- requested, approved, and actually committed RAM/VRAM;
- frame and compute timing summaries plus sampled thermal/memory observations;
- completion/abort reason, safety events, and reduced-capability warnings.

An aborted run is useful diagnostic evidence but is not a benchmark pass. Never merge
numbers from different power-profile metadata, active-workload conditions, resolutions,
drivers, or harness versions into one comparison.
