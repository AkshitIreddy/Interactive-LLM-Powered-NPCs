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
- Accepted headless video: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\headless-realistic-game-framing-v5\mara-magpie-headless-lipsync.mp4`
- Accepted report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\headless-realistic-game-framing-v5\headless-proof.json`

The worktree is the ongoing uncommitted 2.0 overhaul. Preserve all existing
changes. Do not reset, clean, commit, publish, or rewrite history without an
explicit user request. Do not print, copy into logs, or commit credentials.

## Proven result

The exact pinned OpenSeeFace MNV3+LM1 models and ONNX Runtime 1.22.1 CPU path
were exercised headlessly against the realistic Mara portrait. A real NVIDIA
Magpie API-generated 22,050 Hz mono PCM WAV drove the actual
`ReferenceMouthWorker` and current-frame compositor at 30 FPS.

- 42/42 residual frames
- 34 changed adjacent frames; 32 distinct frame digests
- OpenSeeFace 25.789 ms p50 / 30.210 ms p95
- compositor 1.657 ms p95
- 54,706,176 process private bytes
- 0 GPU VRAM
- audio -29.149 dBFS RMS / -11.832 dBFS peak
- 28 frames above the material motion gate; mouth mean absolute delta 0.020 closed / 5.015 maximum

Closed, opening, sustained, and return-to-closed frames were inspected. An
initial diagonal-cavity artifact was rejected and corrected. The accepted
worker binary is `184320` bytes with SHA-256
`b9aeea95256e433b4297a3682978ca496aa227cf56638b53c24165ff7272a396` and
uses the Windows GUI subsystem; the same bytes are in the source sidecar slot
and beside review-app-v8.

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
