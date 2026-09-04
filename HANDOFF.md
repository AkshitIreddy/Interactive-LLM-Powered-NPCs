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
- Review app: `E:\temp\InteractiveNPCs\review-app-v12-sample-clock-visemes\interactive-npcs-control.exe`
- Test game: `E:\temp\InteractiveNPCs\review-test-game-v12\interactive-npcs-synthetic-target.exe`
- Current headless video candidate: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v32-wider-opening\pexels-man-jason-wider-opening-lipsync.mp4`
- Current headless report: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v32-wider-opening\headless-proof.json`
- Current mouth-detail review: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v32-wider-opening\review\mouth-detail-sequence.png`
- Male voice report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\male-jason-v1\nvidia-nim.json`
- Rejected v8 evidence: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final`
- Rejected v16 evidence: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v16-jason-source-preserving`
- Local package manifest: `E:\temp\InteractiveNPCs\package-v12-viseme-cues\20260904T074412Z-d0a9875c90c54bbf934ffb0a2036dbea\package-manifest.json`
- Local unchecked installer: `E:\temp\InteractiveNPCs\package-v12-viseme-cues\20260904T074412Z-d0a9875c90c54bbf934ffb0a2036dbea\Interactive NPCs Response Console_2.0.0-alpha.1_x64-setup.exe`
- Installed reconciliation: `E:\temp\InteractiveNPCs\package-v9-refined\20260903T084130Z-d0670c5a63ea4dff9ff67d3ca1c917f0\installed-review-v9-manifest.json`

The 2.0 overhaul has current lip-sync checkpoints at `236d5fe` and `32db853`.
Preserve subsequent work,
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
- 38 changed adjacent output frames; 39 distinct frame digests
- 32 changed adjacent source frames
- moving OpenSeeFace 27.404 ms p50 / 34.817 ms p95 at adaptive 10 Hz
- compositor 3.043 ms p95 at 30 FPS
- 168,382,464 process private bytes with all source frames held by the proof harness
- 0 GPU VRAM
- audio RMS 0.045705 / peak 0.373505; zero clipped samples
- 28 frames above the material motion gate; mouth mean absolute delta 0.000 closed / 13.356 maximum
- 0.192 maximum articulated rows relative to mouth width
- 0.000 maximum protected-upper-lip darkened fraction across all 40 frames

Closed, opening, sustained, and return-to-closed frames were inspected. User
review rejected v16 because its geometric seam and symmetric cavity cut a dark
hole through the upper lip. The current compositor preserves the complete
18-point mouth contour, finds the contact seam from current-frame pixels, locks
the exact upper-lip source surface, and opens a tapered curve through the lower
jaw. A conservative source-colour cavity plus gated tongue/enamel hints add depth
without claiming to reconstruct unseen identity-specific anatomy. The harness
gates protected-upper-lip darkening separately so corrupt pixels cannot inflate
the whole-mouth motion score. The earlier v8 render also remains rejected for its
detached dark slit, flat white anatomy, and female Aria fixture on a male subject.

Review-app-v12 is a local copy of the reconciled v9 review tree with the clean
`bf97079` Debug-review control/runtime, warning-as-error-built Release native
sidecars, and matching nested manifests. Its loose app matches the package
manifest's pre-bundle SHA-256; all four sidecars match by SHA-256 and size, and
all five executables passed GUI-subsystem/import audit. It has not been launched
or installed and is not newly reconciled installer evidence. The older v9
package remains useful historical package evidence but contains a rejected
compositor.

## Verification completed

- Runtime core: 48 passed
- Runtime host: 94 passed, 2 explicit live tests ignored
- Tauri focused command-29 cue/coarticulation tests: 6 passed
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

The v32 headless visual core is the current efficient component candidate. It
prefers exact provider viseme cues and otherwise uses a causal broad-spectrum
PCM classifier; it is not ASR or a full SadTalker-style head generator. The
retained Jason WAV did not contain provider viseme events, so the v32 video
demonstrates the PCM fallback while cross-language tests prove the exact cue
path. The desktop app/broker presentation path is not
accepted: the last live attempt did not play/present lip-sync, and no complete
native visual presentation receipt exists. Do not claim that the review app's
in-game lip-sync works until a future safe presentation test proves it. Do not
restore v16 as accepted evidence; it failed the user's upper-lip visual review.
