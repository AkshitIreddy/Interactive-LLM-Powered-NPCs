# Headless realistic lip-sync proof — 2026-09-03

## Verdict

The isolated local visual core passes its current headless efficiency and motion
gate on the reference Windows laptop. The proof uses the exact pinned
OpenSeeFace MNV3 detector and LM1 landmark model through ONNX Runtime 1.22.1 on
CPU, a realistic 960x720 game-framed character sequence, and a real NVIDIA
Magpie API-generated PCM WAV. It produces a bounded procedural mouth residual
at 30 FPS without loading an image-generation model during speech. OpenSeeFace
runs at its admitted 15 Hz signal rate and the accepted geometry is rebound to
the intervening exact source frame for 30 FPS presentation.

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
| OpenSeeFace moving inference | 25.013 ms p50; 25.423 ms p95; 21 samples at 15 Hz |
| Procedural compositor | 1.488 ms p95 |
| Provider/model load | 233.831 ms |
| Process private bytes | 169,332,736 bytes, including 42 decoded source frames held by the proof harness |
| GPU VRAM | 0 bytes |
| Source motion | 26 changed adjacent source frames |
| Output motion | 41 changed adjacent frames; 42 distinct frame digests |
| Visible mouth motion gate | 31 frames; mean absolute delta 0.019 closed / 4.273 maximum |
| Final tracking confidence | detector 0.782; landmarks 0.915; visibility 1.000 |
| Magpie WAV | PCM s16le, 22,050 Hz mono, 1.393 seconds |
| Audio level | -29.149 dBFS RMS; -11.832 dBFS peak |

The v3 gate compares every composited mouth patch with its own moving source
frame. This demonstrates a material voiced change and a near-source closed
state rather than accepting camera/idle motion as lip-sync. Representative
closed, opening, sustained, and return-to-closed frames and a six-phase mouth
contact sheet were inspected. The render caps noisy landmark seam slope,
preserves the upper lip more strongly than the lower jaw, adds a restrained
exposure-matched upper-teeth band and tongue floor only for deep openings, and
keeps the residual tied to the measured mouth rectangle.

## Efficiency design

- OpenSeeFace runs at 15 Hz on moving source frames. The last accepted geometry
  carries for one intervening 30 FPS frame; it is rebound to that exact frame
  and still passes actor, track, confidence, pose, freshness, and deadline gates.
- A causal dB-scaled PCM envelope drives the mouth. Hosted speech around normal
  -30 dBFS levels remains expressive without look-ahead or speaker calibration.
  Per-stream attack/release smoothing removes single-frame chatter and resets
  on segment, track, generation, or continuity changes.
- The compositor separates the measured lips, synthesizes only a tapered oral
  cavity/anatomy layer, and leaves the rest of the source frame untouched.
- Every residual retains exact frame, actor, track, epoch, generation, lease,
  confidence, pose, occlusion, freshness, and deadline gates.
- The implementation is CPU-only and therefore reserves GPU capacity for the
  game and any independently selected local conversation models.

## Evidence and reproduction

- Report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-mara-v5\headless-proof.json`
- Reviewed video: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-mara-v5\mara-magpie-moving-lipsync.mp4`
- Frame sequence: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-mara-v5\frames`
- Full-frame contact sheet: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-mara-v5\full-contact-sheet.png`
- Mouth contact sheet: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\moving-mara-v5\mouth-contact-sheet.png`
- Video SHA-256: `edd38e2d126b293eef1c5911c26855e9136e8821ab2d3be55157160222e84846`
- Report SHA-256: `8824e20dc501e1a74d47d94b0b0b92a9079972bb1107c3e72c1d087161e09b55`
- Harness: `native/mouth-worker/tools/headless_realistic_proof.cpp`

The harness verifies the exact model/runtime hashes, runs three warmups and
twenty measured warm-path inferences, then performs 21 paced moving-frame
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

A redistribution-safe CC0/MIT Godot character source was also tested from
[VitruvianGodot](https://github.com/ibrews/VitruvianGodot). Its distant and
close-up frames were rejected by the unchanged production adapter at landmark
confidence 0.756 and 0.742 respectively. That is useful negative evidence: the
system fails open to the untouched frame for unsupported stylized faces instead
of lowering its confidence floor to make a showcase pass.

Primary references: [OpenSeeFace](https://github.com/emilianavt/OpenSeeFace),
[MuseTalk](https://github.com/TMElyralab/MuseTalk),
[LivePortrait](https://github.com/KlingAIResearch/LivePortrait),
[Rhubarb Lip Sync](https://github.com/DanielSWolf/rhubarb-lip-sync),
[HeadAudio](https://github.com/met4citizen/HeadAudio), and
[Emma Skin](https://github.com/ryanhuge/emma-skin).

## Remaining gate

This is a lightweight talking-mouth renderer, not a phoneme recognizer or a
neural talking-head generator. The current moving source is a deterministic
game-idle transform of the project-owned Mara portrait, not captured commercial
gameplay; live-game generality remains unproven. TTS/provider visemes can
improve articulation when available; the PCM path is the deterministic fallback. The desktop
capture/presentation route must still produce and retain an accepted native
presentation receipt before the app may advertise working in-game lip-sync.
