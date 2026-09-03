# Headless realistic lip-sync proof — 2026-09-03

## Verdict

**REJECTED.** This run satisfied its mechanical motion and CPU-efficiency
thresholds, but close-up visual review found a detached procedural dark mouth
slit and a flat synthetic teeth strip that did not preserve the source lips.
Its NVIDIA Magpie fixture also used the female stock voice Aria on a male
subject. The artifacts are retained only as performance and failure evidence;
they do not qualify the compositor or visual core. See the
[source-preserving replacement](headless-realistic-lipsync-proof-2026-09-03-v2.md).

The rejected proof used the exact pinned
OpenSeeFace MNV3 detector and LM1 landmark model through ONNX Runtime 1.22.1 on
CPU, two license-safe 960x720 real-person motion sequences, and a real NVIDIA
Magpie API-generated PCM WAV. It produces a bounded procedural mouth residual
at 30 FPS without loading an image-generation model during speech. OpenSeeFace
runs at an admitted 10 or 15 Hz signal rate and the measured geometry is rebound
to each intervening exact source frame for 30 FPS presentation.

This does **not** qualify the desktop app's WGC/broker/DirectComposition
presentation path. The last live app attempt did not present lip-sync, and that
capture loop was intentionally paused. There is no claim of an accepted
in-game overlay, a presenter receipt, a selectable visual pack, or a full
SadTalker-style head animation.

## Measured result

| Measurement | Result |
| --- | ---: |
| Output | 42 frames, 960x720, 30 FPS, H.264/AAC |
| Duration | 1.393 seconds |
| OpenSeeFace moving inference | 26.989 ms p50; 28.825 ms p95; 14 samples at 10 Hz |
| Procedural compositor | 2.027 ms p95 |
| Provider/model load | 257.272 ms |
| Process private bytes | 168,960,000 bytes, including 42 decoded source frames held by the proof harness |
| GPU VRAM | 0 bytes |
| Source motion | 34 changed adjacent source frames |
| Output motion | 41 changed adjacent frames; 42 distinct frame digests |
| Visible mouth motion gate | 31 frames; mean absolute delta 0.018 closed / 6.757 maximum |
| Final tracking confidence | detector 0.783; landmarks 0.861; visibility 1.000 |
| Magpie WAV | PCM s16le, 22,050 Hz mono, 1.393 seconds |
| Audio level | -29.149 dBFS RMS; -11.832 dBFS peak |

The v4 gate compares every composited mouth patch with its own moving source
frame, so the recorded numbers establish that pixels changed independently of
camera/idle motion. They do not establish acceptable appearance. Later
close-up review showed that the synthetic cavity and teeth logic created
exactly the detached black slit and white-band artifact the numeric gate failed
to detect. Visual acceptance is withdrawn.

## Efficiency design

- OpenSeeFace runs at 15 Hz when its paced p95 fits 40 ms or at 10 Hz when it
  fits 60 ms. Both modes reserve 40% of the signal period for the game and
  other work. After acquisition, the harness carries a context-preserving
  expansion of the last accepted face ROI instead of repeating a nearly
  full-frame search. Accepted geometry is rebound to each intervening exact
  frame and still passes actor, track, confidence, pose, freshness, and
  deadline gates.
- A causal dB-scaled PCM envelope drives the mouth. Hosted speech around normal
  -30 dBFS levels remains expressive without look-ahead or speaker calibration.
  Per-stream attack/release smoothing removes single-frame chatter and resets
  on segment, track, generation, or continuity changes.
- The rejected compositor separated the measured lips and synthesized a tapered
  cavity/anatomy layer. That generated anatomy is the failure under review.
- Every residual retains exact frame, actor, track, epoch, generation, lease,
  confidence, pose, occlusion, freshness, and deadline gates.
- The implementation is CPU-only and therefore reserves GPU capacity for the
  game and any independently selected local conversation models.

## Evidence and reproduction

- Rejected report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final\headless-proof.json`
- Rejected video: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final\pexels-man-magpie-moving-lipsync.mp4`
- Rejected frame sequence: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final\frames`
- Rejected full-frame contact sheet: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final\full-contact-sheet.png`
- Rejected mouth contact sheet: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-man-v8-final\mouth-contact-sheet.png`
- Video SHA-256: `06633e55d74d816db24dcdd76f5ced93814b40ed45bf6fc475f3a1d3e59c17c7`
- Report SHA-256: `adb837b649a858cd1649c28a5cea5307f77d81898b0a89ec58d90dfc5d84ec0f`
- Original source SHA-256: `f313f7c04bb1cb7c588b9309c4d831c5a271695ba9e2e65c66dd1186f2a858de`
- Secondary wind/idle proof: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-pexels-wind-v3-final`
- Harness: `native/mouth-worker/tools/headless_realistic_proof.cpp`

The harness verifies the exact model/runtime hashes, runs three warmups and
twenty measured warm-path inferences, then performs paced 10 or 15 Hz moving-frame
inferences, applies the unchanged product tracking gates, feeds the real PCM in
30 FPS causal windows through `ReferenceMouthWorker`, writes the actual
composited frames, and fails if performance, residual count, source motion, or
mouth-motion diversity falls outside the bounded criteria.

## Moving-footage qualification notes

Research covered heavyweight general portrait generators (MuseTalk,
LivePortrait, Teller, Wav2Lip), lightweight audio/viseme drivers (Rhubarb,
uLipSync/HeadAudio and the Meta 15-viseme convention), and sprite-over-idle
video systems such as Emma Skin. The resident path deliberately follows the
last category: source video carries breathing, blink, hair and body motion;
the bounded residual carries speech. Neural generators remain optional packs
because their resident GPU cost conflicts with a running game.

The primary real-person source is [Pexels video 6682450 by Kampus
Production](https://www.pexels.com/video/person-looking-at-camera-6682450/).
The independent wind/idle stress source is [Pexels video 7956869 by Artem
Podrez](https://www.pexels.com/video/a-woman-looking-in-the-camera-7956869/).
Both were downloaded and transformed under the current [Pexels
license](https://www.pexels.com/license/). The source videos and derived frames
remain under `E:\temp`; no identifiable-person footage is committed to the
repository and no endorsement is implied.

A redistribution-safe CC0/MIT Godot character source was also tested from
[VitruvianGodot](https://github.com/ibrews/VitruvianGodot). Its distant and
close-up frames were rejected by the unchanged production adapter at landmark
confidence 0.756 and 0.742 respectively. That is useful negative evidence: the
system fails open to the untouched frame for unsupported stylized faces instead
of lowering its confidence floor to make a showcase pass.

Two additional real-person sources also failed safely. An extreme close-up
blink portrait could not produce a valid in-frame landmark packet. A darker
neutral studio portrait passed detection but measured landmark confidence
0.809-0.814, below the unchanged 0.82 floor. The mechanically passing wind/idle
source has hair crossing the lower face; it is retained as a visual stress case, not as
occlusion-gate evidence, because the offline harness supplies fixed appearance
evidence rather than classifying hair blockers.

Primary references: [OpenSeeFace](https://github.com/emilianavt/OpenSeeFace),
[MuseTalk](https://github.com/TMElyralab/MuseTalk),
[LivePortrait](https://github.com/KlingAIResearch/LivePortrait),
[Rhubarb Lip Sync](https://github.com/DanielSWolf/rhubarb-lip-sync),
[HeadAudio](https://github.com/met4citizen/HeadAudio), and
[Emma Skin](https://github.com/ryanhuge/emma-skin).

## Remaining gate

This v8 visual result is rejected. The replacement proof must address the
source-preservation failure separately from the existing desktop presentation,
broader face/camera quality, phoneme articulation, and live-game gates. The
desktop capture/presentation route must still produce and retain an accepted
native presentation receipt before the app may advertise working in-game
lip-sync.
