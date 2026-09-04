# Upper-lip-protected headless lip-sync proof — 2026-09-04

## Verdict

**Passes the isolated headless component gates and pixel review; remains a
provisional visual candidate pending user review.** The prior v16 proof is
rejected because its symmetric cavity cut through the upper lip. The replacement
derives the closed-mouth contact seam from current-frame pixels, preserves the
upper lip, and applies amplified motion only below that seam through the lower
jaw.

This is still an energy-driven talking-mouth fallback rather than phoneme
recognition or neural full-head generation. It does not qualify desktop broker
presentation, selectable-pack activation, live-game behavior, or broad face and
camera generality.

## Measured result

| Measurement | Result |
| --- | ---: |
| Output | 40 frames, 960x720, 30 FPS, H.264/AAC |
| Duration | 1.300 seconds |
| OpenSeeFace moving inference | 27.303 ms p50; 58.284 ms p95; 14 samples at 10 Hz |
| Procedural compositor | 1.847 ms p95 |
| Provider/model load | 247.617 ms |
| Process private bytes | 169,140,224 bytes, including decoded source frames held by the harness |
| GPU VRAM | 0 bytes |
| Source motion | 32 changed adjacent source frames |
| Output motion | 39 changed adjacent frames; 40 distinct frame digests |
| Visible mouth motion gate | 18 frames; mean absolute delta 0.018 closed / 1.378 maximum |
| Protected upper-lip darkening | 0.000 maximum fraction across all frames |
| Final tracking confidence | detector 0.783; landmarks 0.861; visibility 1.000 |
| Magpie voice | `Magpie-Multilingual.EN-US.Jason`; cloning disabled |

## Defect and correction

The v16 renderer positioned its cavity around the geometric midpoint of outer
upper- and lower-lip landmarks. On this moustached subject that midpoint lay
inside the upper lip. Symmetric displacement then widened the dark strip upward,
creating the reported hole.

The replacement:

- searches the lower half of the tracked outer-lip span for the darkest
  source-derived contact seam;
- excludes the moustache and upper-lip shadow from that seam search;
- leaves the upper lip stationary and confines cavity expansion to 2% above the
  selected seam versus 155% below it;
- amplifies causal audio motion through the lower jaw so the safer deformation
  remains visible;
- retains source pixels and colour variation instead of generating teeth or
  tongue; and
- records a protected-upper-lip darkening metric independently of whole-mouth
  motion, preventing the old artifact from helping the acceptance score.

A synthetic regression fixture fails the old symmetric compositor and now
requires both zero darkened pixels in the upper-lip band and visible motion in
the lower-mouth band.

## Evidence

- Report: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v21-upper-lip-protected\headless-proof.json`
- Video: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v21-upper-lip-protected\pexels-man-jason-upper-lip-protected.mp4`
- Mouth sequence: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v21-upper-lip-protected\visual-review\mouth-sequence.png`
- Source/rejected/fixed comparison: `E:\temp\InteractiveNPCs\voice-lipsync-20260904\moving-pexels-man-v21-upper-lip-protected\visual-review\source-rejected-fixed.png`
- Video SHA-256: `effce63993844105dd3a84cdb5f457219a835583789cb55d7b215739d2fc8e3b`
- Report SHA-256: `ab5d97f724bb42ae9ed4c0ee448f5a72c5f07551be4392d065bbd65e34de6b5d`

The primary source remains the license-safe
[Pexels video 6682450 by Kampus Production](https://www.pexels.com/video/person-looking-at-camera-6682450/).
Source footage and generated evidence remain under `E:\temp`; no
identifiable-person media is committed to the repository and no endorsement is
implied.

## Remaining gates

The qualified scope remains one short moving male source, one stock-voice
utterance, CPU landmarks, and headless composition. Longer and phoneme-aware
speech, diverse identities and facial hair, occlusion, profiles, HDR, WGC
transport, broker presentation, and live-game behavior remain unqualified. Any
failure continues to fall back to the untouched source frame and
audio/subtitles.
