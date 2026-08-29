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
6. Tracked viseme/mouth-warp baseline versus MuseTalk 1.5 under 12 GB contention.
7. Conditional NVIDIA AR SDK LipSync spike only after private access/license validation; record its fixed pre-roll rather than hiding it in end-to-end timing.

## Release gates

Use the numeric acceptance thresholds in `docs/requirements/traceability.md`. Any result that is absent is reported as not measured, never replaced by simulated or vendor data. Synthetic Eclipse Harbor performance HUD values are labeled illustrative unless sourced from a stored benchmark manifest.
