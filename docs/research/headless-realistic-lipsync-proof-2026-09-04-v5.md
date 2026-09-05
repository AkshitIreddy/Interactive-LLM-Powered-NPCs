# Identity-observed moving-frame lip-sync proof v5 (historical)

Date: 2026-09-04
Status: **superseded and rejected as visual-quality evidence**

This document preserves the v37 experiment as historical evidence. It is not a
current qualification result. The current source-preserving experiment is
documented in the [v6 report](headless-realistic-lipsync-proof-2026-09-04-v6.md).

## Why the earlier candidates were rejected

The v32 procedural result was visually unacceptable even though its older
numeric gates passed. It held the upper lip nearly static, expanded a dark
source seam into a rigid cavity, and synthesized a flat enamel reflection. The
result looked like a rectangular hole below the real mouth.

The v37 experiment replaced invented oral anatomy with five mouth observations
from a Pexels speaker and passed the then-current flat-edge/opening audit. User
review later found that the broad observed-mouth transfer still looked pasted,
distorted, and insufficiently source-preserving. Its pass therefore exposed a
gap in that audit rather than qualifying the renderer. The old audit did not
measure changed pixels outside the tracked lip contour, output/source corner
width drift, mouth-roll drift, requested/rendered aperture correlation, or
state-transition shape popping.

## Historical architecture and measurements

The experiment used the freely reusable Pexels clip
[Close-up View of a Man Talking](https://www.pexels.com/video/close-up-view-of-a-man-talking-4994154/),
five observed mouth states, and the explicitly selected stock male NVIDIA
Magpie voice `Magpie-Multilingual.EN-US.Jason`. MediaPipe Face Mesh 0.10.21 was
used for this experiment after OpenSeeFace accepted only 7 of 95 enrollment
frames and 2 of 48 moving-source frames. That tracker result remains useful
evidence that face geometry providers must be replaceable; it is not evidence
that MediaPipe is universally reliable.

The v37 run produced 40 frames at 540×960 and 30 FPS. Its historical audit
reported:

- 35 material-motion frames;
- `0.094340` median top-edge mode share;
- `0.086538` median bottom-edge mode share;
- `0.678947` maximum residual height-to-width; and
- `0.220000` maximum adjacent openness change.

The portable Python renderer measured `29.030 ms` mean / `39.483 ms` p95.
Separate native 1920×1080 atlas selector/compositor runs measured
`4.549-4.724 ms` mean / `5.101-6.692 ms` p95 across 250 iterations. Those
measurements remain historical performance observations, but they do not
overrule the failed visual review and do not establish parity with later
source-preserving rendering.

## Historical evidence paths

- Video: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\pexels-talking-man-jason-observed-atlas-lipsync.mp4`
- Source/output board: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\source-output-mouth-board.png`
- All-frame mouth board: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\all-output-mouth-frames.png`
- Historical audit: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\mouth-quality-audit.json`
- Historical render manifest: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-talking-man-v37-qualified-atlas\observed-atlas-proof.json`

The directory name contains `qualified-atlas` because that was its contemporaneous
classification; the classification is withdrawn. Do not cite v37 as accepted
component, desktop, live-game, installer, or release evidence.
