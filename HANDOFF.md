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
- Review app: `E:\temp\InteractiveNPCs\review-app-v8\interactive-npcs-control.exe`
- Test game: `E:\temp\local-app-data\test-game\interactive-npcs-synthetic-target.exe`
- Accepted headless video: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-mara-v5\mara-magpie-moving-lipsync.mp4`
- Accepted report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-mara-v5\headless-proof.json`

The 2.0 overhaul has a local checkpoint at `3386392`. Preserve subsequent work,
verify status before editing, and do not reset, clean, publish, or rewrite
history without an explicit user request. Do not print, copy into logs, or
commit credentials.

## Proven result

The exact pinned OpenSeeFace MNV3+LM1 models and ONNX Runtime 1.22.1 CPU path
were exercised headlessly against a moving realistic Mara game-idle sequence. A real NVIDIA
Magpie API-generated 22,050 Hz mono PCM WAV drove the actual
`ReferenceMouthWorker` and current-frame compositor at 30 FPS.

- 42/42 residual frames
- 41 changed adjacent output frames; 42 distinct frame digests
- 26 changed adjacent source frames
- moving OpenSeeFace 25.013 ms p50 / 25.423 ms p95 at 15 Hz
- compositor 1.488 ms p95 at 30 FPS
- 169,332,736 process private bytes with all 42 source frames held by the proof harness
- 0 GPU VRAM
- audio -29.149 dBFS RMS / -11.832 dBFS peak
- 31 frames above the material motion gate; mouth mean absolute delta 0.019 closed / 4.273 maximum

Closed, opening, sustained, and return-to-closed frames were inspected. The
refined compositor preserves the upper lip, moves the lower jaw more strongly,
adds restrained exposure-matched teeth/tongue detail, and smooths PCM attack
and release. The review-app-v8 sidecar predates this refinement and the app
presentation path remains unaccepted; do not infer app delivery from the
headless artifact.

## Verification completed

- Tauri/Rust library: 203 passed
- Frontend: 124 passed
- TypeScript typecheck: passed
- Mouth worker portable suites: worker, product runtime, service protocol,
  landmark provider, and PE subsystem all passed
- Media broker non-GUI suites: core, simulated display matrix, input transport,
  playback transport, and presentation context all passed
- H.264/AAC output: 960x720, 30 FPS, 1.393 seconds

## Truth boundary

The headless visual core is accepted as efficient component evidence. It is a
causal energy-driven talking-mouth renderer, not a phoneme recognizer or full
SadTalker-style head generator. The desktop app/broker presentation path is not
accepted: the last live attempt did not play/present lip-sync, and no complete
native visual presentation receipt exists. Do not claim that the review app's
in-game lip-sync works until a future safe presentation test proves it.
