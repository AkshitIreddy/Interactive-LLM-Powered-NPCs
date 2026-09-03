# Headless realistic lip-sync proof — 2026-09-03

## Verdict

The isolated local visual core passes its current headless efficiency and motion
gate on the reference Windows laptop. The proof uses the exact pinned
OpenSeeFace MNV3 detector and LM1 landmark model through ONNX Runtime 1.22.1 on
CPU, a realistic 960x720 portrait, and a real NVIDIA Magpie API-generated PCM
WAV. It produces a bounded procedural mouth residual at 30 FPS without loading
an image-generation model during speech.

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
| OpenSeeFace inference | 25.789 ms p50; 30.210 ms p95 |
| Procedural compositor | 1.657 ms p95 |
| Provider/model load | 246.192 ms |
| Process private bytes | 54,706,176 bytes |
| GPU VRAM | 0 bytes |
| Motion | 34 changed adjacent frames; 32 distinct frame digests |
| Visible mouth motion gate | 28 frames; mean absolute delta 0.020 closed / 5.015 maximum |
| Tracking confidence | detector 0.756; landmarks 0.912; visibility 1.000 |
| Magpie WAV | PCM s16le, 22,050 Hz mono, 1.393 seconds |
| Audio level | -29.149 dBFS RMS; -11.832 dBFS peak |

The mouth-region crop measured about 35.66–36.09 dB PSNR against the static
source during voiced frames and 73.27 dB in closed phases. This demonstrates a
material voiced change and a near-source closed state rather than accepting
frame-count changes alone. Representative closed, opening, sustained, and
return-to-closed frames were inspected. An initial diagonal-cavity artifact was
rejected; the accepted render caps noisy landmark seam slope while keeping the
residual tied to the measured mouth rectangle.

## Efficiency design

- OpenSeeFace runs once to establish face and mouth geometry for the static
  character view; the geometry is cached until source/track invalidation.
- A causal dB-scaled PCM envelope drives the mouth. Hosted speech around normal
  -30 dBFS levels remains expressive without look-ahead or speaker calibration.
- The compositor separates the measured lips, synthesizes only a tapered oral
  cavity, and leaves the rest of the source frame untouched.
- Every residual retains exact frame, actor, track, epoch, generation, lease,
  confidence, pose, occlusion, freshness, and deadline gates.
- The implementation is CPU-only and therefore reserves GPU capacity for the
  game and any independently selected local conversation models.

## Evidence and reproduction

- Report: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\headless-realistic-game-framing-v5\headless-proof.json`
- Reviewed video: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\headless-realistic-game-framing-v5\mara-magpie-headless-lipsync.mp4`
- Frame sequence: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\headless-realistic-game-framing-v5\frames`
- Review frames: `E:\temp\InteractiveNPCs\voice-lipsync-20260903\headless-realistic-game-framing-v5\review-frames`
- Harness: `native/mouth-worker/tools/headless_realistic_proof.cpp`

The harness verifies the exact model/runtime hashes, runs three warmups and
twenty measured inferences, applies the unchanged product tracking gates, feeds
the real PCM in 30 FPS causal windows through `ReferenceMouthWorker`, writes the
actual composited frames, and fails if performance, residual count, or motion
diversity falls outside the bounded criteria.

## Remaining gate

This is a lightweight talking-mouth renderer, not a phoneme recognizer or a
neural talking-head generator. TTS/provider visemes can improve articulation
when available; the PCM path is the deterministic fallback. The desktop
capture/presentation route must still produce and retain an accepted native
presentation receipt before the app may advertise working in-game lip-sync.
