# Interactive LLM Powered NPCs 2.0 — headless visual-core handoff

Updated: 2026-09-04 IST

## Current decision

Desktop capture, recording, and visual app-driving are intentionally paused.
Do not reopen the broker Windows smoke tests: they previously produced unsafe
white/blue flashing and beeps. Continue with hidden, noninteractive commands and
headless artifacts unless the user explicitly asks to resume desktop testing.

## Authoritative locations

- Checkout: `C:\Users\akshi\Desktop\Code Palace\interactive llm\Interactive-LLM-Powered-NPCs`
- Large build/model/cache root: `E:\temp\InteractiveNPCs`
- Native source mirror: `E:\temp\IPNbuild`
- Review app: `E:\temp\InteractiveNPCs\review-app-v11-upper-lip-protected\interactive-npcs-control.exe`
- Test game: `E:\temp\local-app-data\test-game\interactive-npcs-synthetic-target.exe`
- Current headless video candidate: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v21-upper-lip-protected\pexels-man-jason-upper-lip-protected.mp4`
- Current headless report: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v21-upper-lip-protected\headless-proof.json`
- Current source/rejected/fixed comparison: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v21-upper-lip-protected\visual-review\source-rejected-fixed.png`
- Male voice report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\male-jason-v1\nvidia-nim.json`
- Rejected v8 evidence: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final`
- Rejected v16 evidence: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v16-jason-source-preserving`
- Local package manifest: `E:\temp\InteractiveNPCs\package-v9-refined\20260903T084130Z-d0670c5a63ea4dff9ff67d3ca1c917f0\package-manifest.json`
- Installed reconciliation: `E:\temp\InteractiveNPCs\package-v9-refined\20260903T084130Z-d0670c5a63ea4dff9ff67d3ca1c917f0\installed-review-v9-manifest.json`

The 2.0 overhaul has a local checkpoint at `3386392`. Preserve subsequent work,
verify status before editing, and do not reset, clean, publish, or rewrite
history without an explicit user request. Do not print, copy into logs, or
commit credentials.

## Proven result and withdrawn evidence

The exact pinned OpenSeeFace MNV3+LM1 models and ONNX Runtime 1.22.1 CPU path
were exercised headlessly against license-safe real-person motion. A real NVIDIA
Magpie API-generated 22,050 Hz mono PCM WAV using the explicitly discovered
stock male voice `Magpie-Multilingual.EN-US.Jason` drove the actual
`ReferenceMouthWorker` and current-frame compositor at 30 FPS.

- 40/40 residual frames
- 39 changed adjacent output frames; 40 distinct frame digests
- 32 changed adjacent source frames
- moving OpenSeeFace 27.303 ms p50 / 58.284 ms p95 at adaptive 10 Hz
- compositor 1.847 ms p95 at 30 FPS
- 169,140,224 process private bytes with all source frames held by the proof harness
- 0 GPU VRAM
- audio RMS 0.045705 / peak 0.373505; zero clipped samples
- 18 frames above the material motion gate; mouth mean absolute delta 0.018 closed / 1.378 maximum
- 0.000 maximum protected-upper-lip darkened fraction across all 40 frames

Closed, opening, sustained, and return-to-closed frames were inspected. User
review rejected v16 because its geometric seam and symmetric cavity cut a dark
hole through the upper lip. The current compositor finds the contact seam from
current-frame pixels, keeps the upper lip stationary, opens downward with the
lower jaw, and does not paint procedural teeth or tongue. The harness now gates
protected-upper-lip darkening separately so corrupt pixels cannot inflate the
whole-mouth motion score. The earlier v8 render also remains rejected for its
detached dark slit, flat white anatomy, and female Aria fixture on a male subject.

Review-app-v11 is a local copy of the reconciled v9 review tree with the new
warning-as-error-built and PE-audited mouth sidecar plus matching nested manifests.
It has not been launched and is not a newly reconciled installer. The older v9
package remains useful package evidence but contains the rejected compositor.

## Verification completed

- Tauri/Rust library: 203 passed
- Frontend: 124 passed
- TypeScript typecheck: passed
- Mouth worker portable suites: worker, product runtime, service protocol,
  landmark provider, and PE subsystem all passed
- Media broker non-GUI suites: core, simulated display matrix, input transport,
  playback transport, and presentation context all passed
- H.264/AAC output: 960x720, 30 FPS, 1.300 seconds, male Jason audio
- Refined local Debug package: source-clean at `cb43a7f`, unsigned,
  `unchecked-development-package`, no publication/update-feed action
- Silent install reconciliation: 756 installed files, 755 manifested payload
  files, exact four-sidecar and legal-resource reconciliation passed
- Unsafe broker display/playback/input/identity smoke tests were not run

## Truth boundary

The v21 headless visual core is the current efficient component candidate. It is a
causal energy-driven talking-mouth renderer, not a phoneme recognizer or full
SadTalker-style head generator. The desktop app/broker presentation path is not
accepted: the last live attempt did not play/present lip-sync, and no complete
native visual presentation receipt exists. Do not claim that the review app's
in-game lip-sync works until a future safe presentation test proves it. Do not
restore v16 as accepted evidence; it failed the user's upper-lip visual review.
