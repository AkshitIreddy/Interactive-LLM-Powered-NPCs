# Product rework verification ledger

Date: 2026-08-29  
Status: development vertical slice; not an installer/release approval

## Implemented evidence

- The control shell is a Windows GUI-subsystem executable in Debug as well as
  Release. `scripts/assert-pe-subsystem.ps1` reads the PE optional header, and
  the locally rebuilt `interactive-npcs-control.exe` passed as `Windows Gui`.
- Native first-run state now owns onboarding visibility. A missing or incomplete
  onboarding snapshot opens the four-step System → World → Voice → Test flow
  without a query flag; progress and completion use `save_onboarding`.
- World selection calls the debug-only synthetic-target command. Until that
  command returns PID and frame evidence, the rail says `not selected` and the
  world says `configured test world`.
- Mara Venn is the only visible character and is scoped to Eclipse Harbor. The
  identity authority is explicit selection; no face or demographic inference
  is claimed.
- ElevenLabs credential entry uses the native OS-owned prompt. The WebView sees
  only credential-reference status. The approved route is the allowlisted stock
  Sarah voice and `eleven_flash_v2_5` model.
- A live turn is sent only after a separate one-turn authorization checkbox.
  Normal turns remain deterministic fixtures. Completion events carry
  `runtimeFixtureOnly`, so fixture text cannot be labeled as audible delivery.
- The Windows Debug runtime route now connects the existing ElevenLabs stream
  bridge to the developer WASAPI sink. Success requires nonzero source and
  device submission, source completion, endpoint drain, and one matching receipt
  per delivered sentence. The result remains explicit that OS callback
  submission is not physical-audibility proof and that lip sync is unavailable.
- Conversation content is a session-local delivered-only ledger. No quest,
  relationship, save-state, or historical dialogue fixture remains in normal
  navigation.
- Diagnostics calls runtime doctor, broker diagnostics, and diagnostic summary.
  Browser preview renders dashes and `not run`; it invents no CPU, GPU, frame,
  provider, or latency measurement.
- Settings exposes only three supported persisted preferences and real setup or
  diagnostics navigation. Local visual packs remain unavailable, with no fake
  install control.

## Automated checks

| Check | Result |
| --- | --- |
| Control TypeScript typecheck | passed |
| Control frontend suite | 43 passed |
| Focused new product contract | 14 passed |
| Tauri control library | 64 passed |
| Runtime-host library, default features | 48 passed, 2 intentionally ignored |
| Runtime-host library, dev WASAPI feature | 52 passed, 2 intentionally ignored |
| Runtime-host integration | 8 passed |
| Runtime-host release feature check | passed |
| PE subsystem assertion | Debug control binary verified `Windows Gui` |
| Diff whitespace check | passed |

## Provider qualification

A bounded ElevenLabs stock-voice request was run without printing or persisting
the key in source/logs. Authentication returned HTTP 200 in 627.1 ms. The
13-character TTS request returned 24,703 bytes in 2,735.2 ms and decoded to
24 kHz mono, 1.486 s, peak 0.758392, RMS 0.100684, zero clipping, non-silent.
The temporary media was deleted. SHA-256 evidence:

- MP3: `5a3e702c74c8f95ff265608601a0800c0ae9289bcfe2fa2c55f20399e0ec8b45`
- WAV: `df2392903c94b0cabf4d0f3ffdeb300f4f2c704dd250a17143fbe919a3253822`

This proves the provider route, not installed-app speaker audibility.

## Rendered review

New full-frame evidence is under `artifacts/product-rework-new/` for Session,
World, Voice & models, Diagnostics, first-run onboarding, and a 700 px narrow
Session view. The rendered review checked hierarchy, first-viewport action,
selected navigation, honest empty/disabled states, text wrapping, rail density,
and narrow reflow. The second pass corrected the false `selected` capture label
found during visual inspection.

## Remaining release blockers

No installer was produced. The following are still required before one can be
a user-review candidate:

1. Run the exact native four-step onboarding against a clean app-data root,
   including an already-running packaged synthetic target and advancing frame
   deltas.
2. Enter the provider key through the native prompt and capture one complete
   app-owned live-TTS turn with cancellation coverage.
3. Add and verify microphone/device selection and PTT capture evidence; the
   current PTT control is a bounded rehearsal prompt, not live STT.
4. Implement broker-owned production PCM transport and a rigorous process-
   scoped WASAPI loopback check before claiming physical audibility.
5. Implement the subtitle preset/font/license/DPI/HDR/RTL matrix and actor-
   tracking fallback. The current UI specifies the safe-area decision but does
   not yet render an in-game subtitle overlay.
6. Add timestamped CPU/GPU/VRAM/RAM/frame-impact benchmarks before restoring a
   Performance surface.
7. Repeat rendered review at 150% and 200% and capture 4–6 close-ups for each
   final retained native screen.
