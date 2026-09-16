# Interactive LLM Powered NPCs 2.0 — headless visual-core handoff

> Current continuation: [NEXT_AGENT_HANDOFF_2026-09-16.md](NEXT_AGENT_HANDOFF_2026-09-16.md). The user reports v21 still cannot connect to the test game even when it is open. This older Sep 5 document is historical; use the newer handoff for current source/artifacts, feedback, investigation leads and remaining work.

Updated: 2026-09-05 IST

## Current decision

Desktop capture, recording, and visual app-driving are intentionally paused.
Do not reopen the broker Windows smoke tests: they previously produced unsafe
white/blue flashing and beeps. Continue with hidden, noninteractive commands and
headless artifacts unless the user explicitly asks to resume desktop testing.

The current visual direction is the v53 source-preserving prototype. It is
promising headless evidence, not an accepted app route. The procedural v32 and
broad-atlas v37/v5 renders are rejected. The general local visual route remains
disabled and must fail open to the untouched game frame plus audio/subtitles.

## Authoritative locations

- Checkout: `C:\Users\akshi\Desktop\Code Palace\interactive llm\Interactive-LLM-Powered-NPCs`
- Large build/model/cache root: `E:\temp\InteractiveNPCs`
- Native source mirror: `E:\temp\IPNbuild`
- Review app: `E:\temp\InteractiveNPCs\review-app-v13-stronger-mouth\interactive-npcs-control.exe`
- Test game: `E:\temp\InteractiveNPCs\review-test-game-v13\interactive-npcs-synthetic-target.exe`
- Current headless v53 video: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\mara-jason-imagegen-natural-aperture-v53.mp4`
- Current v53 source/output board: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\source-output-mouth-board.png`
- Current v53 all-frame mouth board: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\all-output-mouth-frames.png`
- Current v53 audit: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\mouth-quality-audit-v2.json`
- Current v53 render manifest: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\dense-observed-lip-proof.json`
- Generated open-mouth enrollment reference: `E:\temp\InteractiveNPCs\generated-enrollment\mara-imagegen-mouth-v1\mara-ah-open.png`
- Male voice report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\male-jason-v1\nvidia-nim.json`
- Historical rejected v32 evidence: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v32-wider-opening`
- Historical rejected v37/v5 evidence: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas`
- Local package manifest: `E:\temp\InteractiveNPCs\package-v13-stronger-mouth\20260904T080131Z-6f8a72b4495641edbc3b9b47ad788cbe\package-manifest.json`
- Local unchecked installer: `E:\temp\InteractiveNPCs\package-v13-stronger-mouth\20260904T080131Z-6f8a72b4495641edbc3b9b47ad788cbe\Interactive NPCs Response Console_2.0.0-alpha.1_x64-setup.exe`
- Installed reconciliation: `E:\temp\InteractiveNPCs\package-v9-refined\20260903T084130Z-d0670c5a63ea4dff9ff67d3ca1c917f0\installed-review-v9-manifest.json`

The review app and installer predate v53 and do not contain or qualify the v53
generated-reference path. Neither should be presented as working in-game
lip-sync evidence.

The 2.0 overhaul has retained lip-sync checkpoints at `236d5fe`, `32db853`,
`ff54adc`, and the experimental observed-mouth checkpoint `5f396da`. The latter
builds and passes component tests but its v66 realistic render is rejected.
Verify status before editing. Do not reset, clean, publish, rewrite history,
print credentials, or copy credentials into logs or commits.

## Current v53 result

The v53 headless render uses the moving Mara idle sequence, MediaPipe geometry,
the stock male `Magpie-Multilingual.EN-US.Jason` WAV, and a one-time generated
open-`ah` enrollment reference. It preserves current-frame lip exterior,
corners, beard, skin, pose, lighting, and idle motion while transferring only
the observed oral interior into continuously deformed current geometry.

The v2 artifact audit passed for the exact retained render:

- 33 material-motion frames;
- `0.194444` / `0.173333` median top/bottom edge-mode share;
- `0.581081` maximum residual height/width;
- `0.019956` median, `0.085686` p95, and `0.095730` maximum changed share
  outside the expanded lip contour;
- `0.996700` requested-openness/rendered-aperture Spearman correlation;
- `0.210151` maximum rendered aperture relative to mouth width;
- corner-width ratios from `0.981984` to `1.018927`; and
- `0.619820°` p95 / `0.717212°` maximum mouth-roll error.

This is an aperture-and-source-preservation proof only. The retained WAV has no
provider viseme events and the manifest records `rms-aperture-only`, so it does
not prove phoneme-accurate lip synchronization or a full viseme set. The
manifest correctly says `rendered-not-qualified`.

## Latest native experiment is rejected

The v66 C++ experiment corrected the OpenSeeFace 66-point mouth topology and
ran the real moving-source proof, but visual inspection still showed a dark
oval/hole, missing teeth, and distorted lip surfaces. The strict proof failed
with `0.532468` maximum upper-lip darkening and `0.4` articulated rows over
mouth width. Preserve it only as regression evidence:

- board: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v66-corrected-openseeface-topology\mouth-board.png`
- report: `E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v66-corrected-openseeface-topology\headless-proof.json`

Do not continue the native atlas method by default. Compare it against current
alternatives and retain it only if a new render actually beats v53 under the
quality and resource gates.

## Enrollment/runtime split

Image generation or MuseTalk may act as an optional one-time teacher during
character enrollment. The retained MuseTalk teacher run took `87.873 s` and
about `7.8 GiB` of GPU memory, so it is explicitly rejected from the gameplay
hot path. Heavy teachers must exit and release resources after producing a
small, reviewed, identity-bound mouth pack. They never run after the user asks
an NPC a question and do not count against the 5–8 second conversation budget.

The intended gameplay path reads a cached tiny pack and runs deterministic
native CPU composition without a resident neural model. It reserves zero GPU
VRAM in the measured native benchmark.

## Native evidence

The current Release native build passed all 6 CTest suites. At 1920×1080 over
250 CPU iterations, the latest verified run measured:

- `6.402 ms` geometric mean;
- `6.323 ms` direct-atlas mean;
- `4.612 ms` atlas worker select-and-compose mean;
- `4.682 ms` atlas worker p50;
- `7.004 ms` atlas worker p95;
- `7.264 ms` atlas worker p99; and
- `0` GPU VRAM.

This proves the current source-preserving native contract and a millisecond
performance envelope on this machine. It does **not** prove generated-reference
parity with v53: the v53 image-reference format and exact deformation/composite
path have not been ported, rendered through the native worker, and visually
compared. The v53 Python inspection renderer itself measured `54.907 ms` mean /
`73.153 ms` p95 and is not the product hot path.

## Superseded and withdrawn evidence

- v8: rejected for a detached dark slit, flat white anatomy, and female Aria
  audio on a male subject.
- v16: rejected because the geometric seam and symmetric cavity cut a dark hole
  through the upper lip.
- v32: rejected because a source-colour procedural cavity and enamel hints still
  produced a rigid hole-like mouth despite passing older gates.
- v37/v5: qualification withdrawn after the broad observed-mouth transfer looked
  pasted and distorted. Its older audit lacked contour containment, corner
  width, roll, aperture-correlation, and temporal-state checks.
- v47 through v51: rejected during headless visual iteration for flat teeth,
  upper seams, painted or oversized lips, ghost contours, or double edges.
- v55 through v66: rejected native-port experiments. Sparse radial, analytic,
  triangle-mesh, and oral-normalization variants produced scallops, rectangular
  smears, seams, dark holes, missing teeth, or excessive upper-lip damage.

Do not restore any rejected candidate as accepted evidence merely because an
older numeric report or directory name says `qualified`.

## Remaining truth boundary

Generated-reference parity, several stable viseme shapes, provider-cue and PCM
cross-utterance synchronization, broad tracker/pose/occlusion coverage,
WGC/broker/DirectComposition presentation, HDR behavior, representative game-
load performance, safe desktop review, real-game certification, per-character
enrollment UX, packaging, installer reconciliation, clean-VM qualification, and
signing all remain open.

The review app and synthetic test game are retained for future safe review, but
they have not been launched for this evidence and must not be described as
showing v53 lip-sync. The detailed current report is
[`docs/research/headless-realistic-lipsync-proof-2026-09-04-v6.md`](docs/research/headless-realistic-lipsync-proof-2026-09-04-v6.md).
