# Benchmark and architecture-spike strategy

## Principle

Measure the complete user-visible pipeline under constrained remaining resources. Isolated model speed and vendor claims select candidates; they do not establish release performance.

## Standard scenarios

- Reference machines: API-only/low-end; 6–8 GB GPU; 12 GB GPU + 32 GB RAM canonical; AMD and Intel GPU configurations.
- This PC: RTX 4080 Laptop GPU 12 GB, i9-13980HX, 16 GB RAM, recorded as one target rather than the universal baseline.
- Execution: configured hosted API routes and Offline control; optional local lip-sync measured separately.
- Load: idle desktop, synthetic CPU/RAM load, GPU compute load, VRAM pressure near configured budget, and representative real game when available.
- Audio: clean English, accents, quiet/loud microphones and 10 dB game-noise SNR.
- Display: required single/multi-monitor/DPI/resolution/HDR/mode matrix.
- Turns: short interruption, long response, ambiguous/offscreen identity, provider timeout, device loss and worker crash.

## Instrumentation

Use QPC timestamps at capture, VAD endpoint, STT first/final, identity/retrieval, LLM first token/clause/end, TTS request/first PCM, playback start/end, visual onset/end and cancellation-to-silence. Record median/p95, CPU, GPU engine utilization, DXGI budgets, process/private RAM, load time, audio underruns and game average/1%-low frame impact.

Fixtures, exact hosted model/provider/catalog revisions, optional lip-sync model/pack revision, settings, warm/cold state, driver/OS, power mode and command/tool version accompany every result. Hosted network region/latency and provider-side volatility are disclosed.

## Candidate spikes before subsystem commitment

1. Tauri minimized/active release overhead and UI/runtime reconnect.
2. Named-pipe Protobuf throughput/backpressure/cancellation and shared PCM ring.
3. WGC + DirectComposition on the full display matrix and device recovery.
4. Worker kill/restart/quarantine at every turn stage.
5. NSIS install/update interruption/rollback on clean VMs.
6. Audio2Face-3D regression v2.3 coefficient-to-2D-residual baseline under 12 GB contention, including tracking, immutable-frame residual validation and fail-open latency. No native game rig participates.
7. Conditional NVIDIA Maxine AR SDK LipSync direct-video spike only after access/license validation; measure startup/pre-roll and isolate the mouth residual rather than presenting vendor numbers as product results.
8. Keep MuseTalk as an offline comparator: the existing ~102 s wall time for 1.579 s output is a failed live-admission result, not a baseline to optimize around.
9. Revisit EfficientSync/FlashLips only when public immutable code/weights and licenses can be evaluated; reproduce all paper speed/quality claims locally.

Local conversation models are outside the current product policy. If that policy changes, add a separate co-residency spike that combines live game reserve, each model's warm and p99 workspace, load/unload latency and incompatible-stage serialization. A configuration passes only when admission can classify it without silent device/provider switching.

## Release gates

Use the numeric acceptance thresholds in `docs/requirements/traceability.md`. Any result that is absent is reported as not measured, never replaced by simulated or vendor data. Synthetic Eclipse Harbor performance HUD values are labeled illustrative unless sourced from a stored benchmark manifest.
