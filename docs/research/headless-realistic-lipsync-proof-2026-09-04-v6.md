# Source-preserving moving-frame lip-sync proof v6

Date: 2026-09-04
Current artifact: v53
Status: **headless visual prototype; rendered and audited, not product-qualified**

## Decision

The v53 experiment is the strongest current visual direction, but it is not an
accepted lip-sync route. It preserves the newest moving source frame as the
truth for the face, beard, pose, lighting, idle motion, lip exterior, and mouth
corners. It deforms that current lip geometry and transfers only the oral
interior from a one-time enrollment reference. This avoids the broad pasted
mouth and synthetic black-hole failures seen in earlier candidates.

The retained 1.3-second Jason fixture supplies only an RMS-derived aperture
curve. It contains no provider viseme events, so v53 demonstrates continuous
mouth opening and closing, containment, and source preservation—not phoneme-
accurate synchronization or cross-utterance speech quality.

## What was rejected before v53

- v32 painted a rigid cavity and flat enamel strip beneath a nearly static
  upper lip. Its older numeric pass did not survive visual review.
- v37/v5 pasted a broad observed mouth region. It passed the then-current audit
  but looked distorted and insufficiently source-preserving; that qualification
  is withdrawn.
- v47 narrowed transfer to the oral interior but retained an unnaturally flat
  teacher-teeth strip and a visible upper seam.
- v48 through v51 explored full observed-lip, rigid-lip, appearance-delta, and
  flow-delta transfer. Visual inspection rejected them for painted or oversized
  lips, ghost contours, and doubled edges.

Those failures remain useful regression examples. Passing a pixel-change or
opening threshold alone is not acceptance evidence.

## v53 method

The headless prototype combines:

1. 40 frames from the existing Mara moving-idle source sequence;
2. MediaPipe landmark tracks for source and enrollment references;
3. the current source frame's exterior lips, corners, facial hair, pose, and
   surrounding skin;
4. one generated, identity-matched natural open-`ah` enrollment reference; and
5. a continuously scaled oral-interior transfer inside a tracked lip contour.

The generated reference is not pasted as a face or whole mouth. Its observed
teeth and cavity pixels fill only anatomy that the closed source image cannot
reveal. The outer mouth remains source-derived. The proof contains no
crossfaded photographed atlas states and returns to the untouched source at
silence.

The source and generated reference were accepted by the recorded MediaPipe
landmark pass. This is fixture-specific evidence only. It does not establish
robust tracking under occlusion, large yaw, unusual facial proportions,
stylized characters, low resolution, HDR, or arbitrary games.

## Enrollment cost versus gameplay cost

Heavy generation belongs outside the conversation loop:

```text
one-time enrollment                   every rendered gameplay frame
-------------------                   -----------------------------
capture/confirm character identity    read cached tiny mouth reference
optional image or video teacher       consume provider cue or local PCM
inspect and approve references        deform current lip geometry
build/hash the tiny local pack         composite bounded oral residual
teacher exits and releases GPU        no resident neural model or GPU VRAM
```

MuseTalk remains unsuitable as a resident renderer on the measured laptop: the
retained teacher run took `87.873 s` and used about `7.8 GiB` of GPU memory for
a short clip. Image generation is likewise enrollment-only. Neither operation
runs after the user asks an NPC a question, so neither belongs in the 5–8 second
STT → LLM → first-audio interaction budget. A future product path may also use
approved mouth observations captured from the same character instead of a
generative teacher.

Enrollment output must be identity-bound, user-reviewed, hashed, small enough
to keep in RAM, and unloadable. No model or generated pack is bundled,
downloaded, selected, or activated automatically.

## Recorded v53 audit

The v2 artifact audit reported `passed` for this exact render:

| Check | Recorded result |
| --- | ---: |
| Material-motion frames | 33 |
| Median top/bottom edge-mode share | `0.194444` / `0.173333` |
| Maximum residual height/width | `0.581081` |
| Median / p95 / maximum changed share outside expanded lip contour | `0.019956` / `0.085686` / `0.095730` |
| Requested openness vs. rendered aperture Spearman | `0.996700` |
| Maximum rendered aperture relative to mouth width | `0.210151` |
| Output/source corner-width ratio min / p05 / median / p95 / max | `0.981984` / `0.984095` / `0.996150` / `1.010667` / `1.018927` |
| Mouth-roll error median / p95 / max | `0.266491°` / `0.619820°` / `0.717212°` |
| Output-landmark coverage | `1.0` for 32 eligible active frames |

Temporal discrete-state popping is not reported because v53 uses one continuous
open reference plus neutral rather than switching among multiple mouth-state
shapes. This avoids a false pass, but it also means v53 does not prove a rounded,
spread, labiodental, or full eleven-viseme atlas.

The stricter audit detects important failure families, but it is not a perceptual
metric and cannot replace human visual review. The manifest itself correctly
records `rendered-not-qualified`.

## Performance evidence is deliberately separated

| Path | Result | What it proves |
| --- | --- | --- |
| v53 Python inspection renderer | `54.907 ms` mean, `64.963 ms` p50, `73.153 ms` p95, `77.164 ms` max; 0 GPU VRAM | Reproducible headless visual experiment; not the product hot path |
| Native C++ geometric path, 1920×1080, 250 iterations | `6.402 ms` mean; CPU, 0 GPU VRAM | Current-frame geometric-path performance on this machine |
| Native C++ direct-atlas path, 1920×1080, 250 iterations | `6.323 ms` mean; CPU, 0 GPU VRAM | Direct source-preserving atlas composition performance on this machine |
| Native C++ atlas worker select-and-compose, 1920×1080, 250 iterations | `4.612 ms` mean, `4.682 ms` p50, `7.004 ms` p95, `7.264 ms` p99; CPU, 0 GPU VRAM | Millisecond worker contract including state selection and composition |
| Native CTest | 6/6 suites passed | Current source-preserving validation, actor/generation safety, service lifecycle, and GUI-subsystem checks |

The native benchmark is not a claim that the C++ output is pixel-identical to
v53. The generated-reference format and v53 deformation/compositing behavior
have not yet been ported and visually compared through the native worker. The
native result therefore demonstrates feasibility and a performance envelope,
while v53 demonstrates the current visual prototype.

An attempted native port after v53 is preserved at commit `5f396da`. Its v66
moving-source render corrected OpenSeeFace's unusual corner topology, compiled,
and passed all six CTest suites, but failed the realistic quality gate with
`0.532468` maximum upper-lip darkening. Enlarged frames still show a dark
oval/hole, missing teeth, and distorted lip surfaces. It is rejected evidence,
not generated-reference parity. See
`E:\temp\InteractiveNPCs\voice-lipsync-20260905\moving-mara-v66-corrected-openseeface-topology`.

## Evidence paths

- Video: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\mara-jason-imagegen-natural-aperture-v53.mp4`
- All-frame mouth board: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\all-output-mouth-frames.png`
- Source/output mouth board: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\source-output-mouth-board.png`
- Output landmarks: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\output-landmarks-mediapipe.json`
- Audit: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\mouth-quality-audit-v2.json`
- Render manifest: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-mara-v53-imagegen-natural-aperture\dense-observed-lip-proof.json`
- Enrollment reference: `E:\temp\InteractiveNPCs\generated-enrollment\mara-imagegen-mouth-v1\mara-ah-open.png`
- Enrollment-reference landmarks: `E:\temp\InteractiveNPCs\generated-enrollment\mara-imagegen-mouth-v1\mara-ah-open-mediapipe.json`
- Moving source frames: `E:\temp\InteractiveNPCs\sources\mara-game-idle-source-v1`
- Moving-source landmarks: `E:\temp\InteractiveNPCs\sources\mara-game-idle-source-v1-mediapipe.json`
- Male Jason audio: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\male-jason-v1\nvidia-magpie-fixture.wav`

The generated enrollment image is local development evidence, not a repository
asset or redistributable model dependency.

## Unqualified product gates

Before the general visual route can be enabled, it still needs:

- native generated-reference parity rendered and visually compared against v53;
- provider-viseme and causal PCM shape tests across multiple utterances, voices,
  languages, and frame/audio rates;
- several approved mouth shapes without visible switching, ghosting, or seams;
- tracker coverage for pose, motion blur, partial occlusion, facial hair,
  stylized faces, and low-resolution game capture;
- authenticated WGC/broker/DirectComposition presentation and HDR validation;
- safe desktop review, a real-game compatibility matrix, and representative
  game-load latency/resource measurements;
- per-character enrollment, approval, removal, and identity-revision UX; and
- package, installer, clean-VM, signing, and release qualification.

Until those gates pass, v53 remains headless prototype evidence. The local
visual route stays disabled and fails open to the untouched game frame while
audio and subtitles continue.
