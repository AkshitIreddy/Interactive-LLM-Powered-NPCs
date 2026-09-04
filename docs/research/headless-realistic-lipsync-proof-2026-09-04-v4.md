# Full-contour, spectrally driven headless lip-sync proof

Date: 2026-09-04
Status: accepted isolated CPU component candidate; desktop and live-game gates remain open

## What changed

The prior renderer retained a source-derived contact seam but still collapsed
OpenSeeFace mouth geometry and ordinary PCM into overly similar open/closed
poses. The current revision:

- preserves all 18 OpenSeeFace lip-contour points instead of four anchors;
- uses the correct OpenSeeFace 66-point inner-mouth layout;
- opens along a curved lower-jaw surface while keeping the exact current-frame
  upper-lip surface locked;
- separates open, rounded, spread, closure, and consonant coefficient shapes;
- uses a bounded seven-band, pre-emphasized PCM spectral fallback so pitch alone
  cannot make every male-voice frame an exaggerated rounded vowel;
- retains exact TTS viseme durations and carries canonical sample-clock cues
  through the authenticated native playback broker when a provider supplies
  them;
- uses 50 ms bounded anticipation and 80 ms release overlap, with decisive
  bilabial closure;
- adds only a conservative source-colour oral cavity, tongue tint, and gated
  upper-enamel reflection when the full contour is available.

The renderer is still not a generative talking-head model. When the current
face has never exposed real interior-mouth texture, it cannot reconstruct
identity-correct unseen anatomy.

## Inputs

| Item | Value |
| --- | --- |
| Moving source | License-safe 42-frame Pexels idle sequence |
| Source resolution | 960 x 720 |
| Speech | 1.300 s NVIDIA Magpie stock male Jason WAV |
| Audio format | 22,050 Hz, mono |
| Landmarks | OpenSeeFace MNV3 + LM1, ONNX Runtime 1.22.1 CPU |
| Output | 40 frames at 30 FPS |
| GPU use | none |

## Accepted run

The final v32 headless run passed every harness gate:

| Measurement | Result |
| --- | ---: |
| Residual frames | 40 / 40 |
| Changed adjacent output frames | 38 |
| Changed adjacent source frames | 32 |
| Distinct frame digests | 39 |
| Frames above material mouth-motion gate | 28 |
| Maximum mouth mean absolute delta | 13.356 |
| Maximum protected-upper-lip darkening | 0.000 |
| Maximum articulated rows / mouth width | 0.192 |
| OpenSeeFace moving p50 / p95 at 10 Hz | 27.404 / 34.817 ms |
| Compositor p95 at 30 FPS | 3.043 ms |
| Process private bytes | 168,382,464 |
| GPU VRAM | 0 bytes |

The 19.2% aperture result exceeds the permanent 10% minimum while preserving
zero protected-upper-lip darkening. The tighter mouth-detail contact sheet was
visually inspected: the upper-lip hole did not recur, the cavity begins below
the visible upper lip, the opening follows a tapered curve rather than a flat
rectangle, and the source idle/head motion remains visible.

## Verification

- Six native mouth-worker CTests passed.
- Same-level 260 Hz, 780 Hz, and 2,300 Hz fixtures select distinct rounded,
  open, and spread coefficient profiles.
- Forty-eight runtime-core tests and ninety-four runtime-host tests passed.
- Producer tests prove cue packets consume zero source frames and malformed,
  unknown, overlapping, or excess visual metadata cannot alter audio accounting.
- Five portable/headless media-broker suites passed, including exact cue wire,
  lease, ordering, lifecycle, and command-29 IPC round-trip tests.
- Control tests passed for exact active-cue selection, bounded anticipation,
  release, bilabial dominance, no-cue amplitude fallback, and stale suppression.
- The H.264/AAC review file was probed as 960 x 720, 30 FPS, 1.300 seconds,
  22,050 Hz mono audio.

Interactive Windows capture/presentation smokes are excluded from this proof.
They are not required to inspect the renderer and remain unsafe to rerun on the
maintainer desktop because earlier attempts produced flashing and audible output.

## Local artifacts

- Report: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v32-wider-opening\headless-proof.json`
- Video: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v32-wider-opening\pexels-man-jason-wider-opening-lipsync.mp4`
- Mouth sequence: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v32-wider-opening\review\all-40-mouth-frames.png`
- Mouth-detail sequence: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v32-wider-opening\review\mouth-detail-sequence.png`
- Video SHA-256: `e198b17453a9f4e2c4345534e945c4888ef98bfeefe27c00e12f49be78e5fa80`

## Truth boundary

This proves a headless component: real moving frames, real provider-generated
male speech, real OpenSeeFace CPU tracking, the production reference worker,
and deterministic residual composition. The retained WAV did not include an
exact provider viseme timeline, so v32 specifically demonstrates the new local
spectral fallback. Exact provider cue behavior is established by cross-language
protocol and sample-clock tests, not by this video.

It does not prove that the desktop app presents the residual, that a commercial
game is supported, that every face/pose/lighting condition looks natural, that
an installer is reconciled, or that unseen teeth are reconstructed. Audio and
subtitles remain the independent fail-safe until those gates pass.
