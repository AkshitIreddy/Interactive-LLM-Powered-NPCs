# Interactive LLM Powered NPCs 2.0 — headless visual-core handoff

Updated: 2026-09-03 IST

## Current decision

Desktop capture, recording, and visual app-driving are intentionally paused.
Do not reopen the broker Windows smoke tests: they previously produced unsafe
white/blue flashing and beeps. Continue with hidden, noninteractive commands and
headless artifacts unless the user explicitly asks to resume desktop testing.

## Authoritative locations

- Checkout: `C:\Users\akshi\Desktop\Code Palace\interactive llm\Interactive-LLM-Powered-NPCs`
- Large build/model/cache root: `E:\temp\InteractiveNPCs`
- Native source mirror: `E:\temp\IPNbuild`
- Review app: `E:\temp\InteractiveNPCs\review-app-v9-refined\interactive-npcs-control.exe`
- Test game: `E:\temp\local-app-data\test-game\interactive-npcs-synthetic-target.exe`
- Accepted headless video: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final\pexels-man-magpie-moving-lipsync.mp4`
- Accepted report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final\headless-proof.json`
- Local package manifest: `E:\temp\InteractiveNPCs\package-v9-refined\20260903T084130Z-d0670c5a63ea4dff9ff67d3ca1c917f0\package-manifest.json`
- Installed reconciliation: `E:\temp\InteractiveNPCs\package-v9-refined\20260903T084130Z-d0670c5a63ea4dff9ff67d3ca1c917f0\installed-review-v9-manifest.json`

The 2.0 overhaul has a local checkpoint at `3386392`. Preserve subsequent work,
verify status before editing, and do not reset, clean, publish, or rewrite
history without an explicit user request. Do not print, copy into logs, or
commit credentials.

## Proven result

The exact pinned OpenSeeFace MNV3+LM1 models and ONNX Runtime 1.22.1 CPU path
were exercised headlessly against two license-safe real-person motion clips. A real NVIDIA
Magpie API-generated 22,050 Hz mono PCM WAV drove the actual
`ReferenceMouthWorker` and current-frame compositor at 30 FPS.

- 42/42 residual frames
- 41 changed adjacent output frames; 42 distinct frame digests
- 34 changed adjacent source frames in the unobstructed primary proof
- moving OpenSeeFace 26.989 ms p50 / 28.825 ms p95 at adaptive 10 Hz
- compositor 2.027 ms p95 at 30 FPS
- 168,960,000 process private bytes with all 42 source frames held by the proof harness
- 0 GPU VRAM
- audio -29.149 dBFS RMS / -11.832 dBFS peak
- 31 frames above the material motion gate; mouth mean absolute delta 0.018 closed / 6.757 maximum

Closed, opening, sustained, and return-to-closed frames were inspected. The
refined compositor preserves the upper lip, moves the lower jaw more strongly,
softens shallow cavity transitions, exposes a narrow exposure-matched teeth
line, and smooths PCM attack and release. Review-app-v9 contains the exact
refined four-sidecar set and passed closed-world installed-file reconciliation,
but its desktop presentation path remains unaccepted; do not infer app delivery
from the headless artifact.

## Verification completed

- Tauri/Rust library: 203 passed
- Frontend: 124 passed
- TypeScript typecheck: passed
- Mouth worker portable suites: worker, product runtime, service protocol,
  landmark provider, and PE subsystem all passed
- Media broker non-GUI suites: core, simulated display matrix, input transport,
  playback transport, and presentation context all passed
- H.264/AAC output: 960x720, 30 FPS, 1.393 seconds
- Refined local Debug package: source-clean at `cb43a7f`, unsigned,
  `unchecked-development-package`, no publication/update-feed action
- Silent install reconciliation: 756 installed files, 755 manifested payload
  files, exact four-sidecar and legal-resource reconciliation passed
- Unsafe broker display/playback/input/identity smoke tests were not run

## Truth boundary

The headless visual core is accepted as efficient component evidence. It is a
causal energy-driven talking-mouth renderer, not a phoneme recognizer or full
SadTalker-style head generator. The desktop app/broker presentation path is not
accepted: the last live attempt did not play/present lip-sync, and no complete
native visual presentation receipt exists. Do not claim that the review app's
in-game lip-sync works until a future safe presentation test proves it.
